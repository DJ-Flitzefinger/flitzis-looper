//! Bounded off-thread material preparation and held, actual source adoption phases.
use super::cold_store::CommittedColdLease;
use super::content_placement::{ContentCell, PlacementObservation};
use super::material_migration::PreparedMigrationMaterial;
pub use super::material_migration_journal::MaterialMigrationJournalStore;
use super::{AudioEngine, cold_load, material_migration};
use crate::messages::SampleBuffer;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

type PreparationOutcome = Result<Arc<PreparedMigrationMaterial>, String>;

struct MigrationAdmission {
    adoption: Arc<std::sync::atomic::AtomicU8>,
    publication: Arc<Mutex<Option<super::input_runtime_binding::SourcePublication>>>,
    cancellation_settled: Arc<AtomicBool>,
}

#[pyclass]
pub struct MaterialMigrationPreparation {
    result: Arc<Mutex<Option<PreparationOutcome>>>,
    cancelled: Arc<AtomicBool>,
    admissions: Mutex<Vec<MigrationAdmission>>,
    requests: Arc<Mutex<Vec<u64>>>,
    _old_original_pin: Arc<super::project_assets::ProjectAssetLease>,
}

impl MaterialMigrationPreparation {
    pub(super) fn material(&self) -> PyResult<Arc<PreparedMigrationMaterial>> {
        let result = self
            .result
            .lock()
            .map_err(|_| PyRuntimeError::new_err("migration preparation lock poisoned"))?;
        match result.as_ref() {
            Some(Ok(material)) => Ok(material.clone()),
            Some(Err(error)) => Err(PyValueError::new_err(error.clone())),
            None => Err(PyValueError::new_err(
                "material migration is still preparing",
            )),
        }
    }
}

#[pymethods]
impl MaterialMigrationPreparation {
    pub fn status(&self) -> PyResult<&'static str> {
        let result = self
            .result
            .lock()
            .map_err(|_| PyRuntimeError::new_err("migration preparation lock poisoned"))?;
        Ok(match result.as_ref() {
            None => "preparing",
            Some(Ok(_)) => "ready",
            Some(Err(_)) => "failed",
        })
    }
    pub fn metadata_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.material()?.metadata())
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))
    }
    pub fn error(&self) -> PyResult<Option<String>> {
        let result = self
            .result
            .lock()
            .map_err(|_| PyRuntimeError::new_err("migration preparation lock poisoned"))?;
        Ok(result
            .as_ref()
            .and_then(|outcome| outcome.as_ref().err().cloned()))
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    /// Before any source admission only the producer can roll back its exclusive creations.
    pub fn abort_unpublished(&self) -> PyResult<()> {
        let admissions = self
            .admissions
            .lock()
            .map_err(|_| PyRuntimeError::new_err("migration admission lock poisoned"))?;
        if !admissions.is_empty() {
            return Err(PyValueError::new_err(
                "admitted migration requires actual source-phase settlement",
            ));
        }
        self.cancelled.store(true, Ordering::Release);
        let mut result = self
            .result
            .lock()
            .map_err(|_| PyRuntimeError::new_err("migration preparation lock poisoned"))?;
        if matches!(result.as_ref(), Some(Ok(_))) {
            if let Some(Ok(material)) = result.take() {
                material.lease.rollback_unadopted_original();
                material.lease.rollback_unadopted_cache();
            }
            *result = Some(Err("material migration preparation cancelled".into()));
        }
        Ok(())
    }
    pub fn abort_rejected(&self) -> PyResult<()> {
        let admissions = self
            .admissions
            .lock()
            .map_err(|_| PyRuntimeError::new_err("migration admission lock poisoned"))?;
        let _requests = self
            .requests
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        for admission in admissions.iter() {
            let publication = admission
                .publication
                .lock()
                .map_err(|_| PyRuntimeError::new_err("cold publication lock poisoned"))?;
            if admission.adoption.load(Ordering::Acquire) != 3
                || (publication.is_some()
                    && !admission.cancellation_settled.load(Ordering::Acquire))
            {
                return Err(PyValueError::new_err(
                    "migration source claim or acknowledgement is unresolved",
                ));
            }
        }
        let material = self.material()?;
        material.lease.rollback_unadopted_original();
        material.lease.rollback_unadopted_cache();
        Ok(())
    }
    /// Remove exactly the temporary preparation assignment, never a subscriber or ACK.
    pub fn release_preparation(&self) -> PyResult<()> {
        self.material()?.lease.release_preparation_assignment();
        Ok(())
    }
}

#[pyclass]
pub struct MaterialMigrationSourceTicket {
    admitted: cold_load::AdmittedMaterial,
    id: usize,
    rate: u32,
    material: Arc<PreparedMigrationMaterial>,
    requests: Arc<Mutex<Vec<u64>>>,
    generations: Arc<Mutex<Vec<(u64, u32)>>>,
    digests: Arc<Mutex<Vec<Option<String>>>>,
    cache: Arc<Mutex<Vec<Option<SampleBuffer>>>>,
    ownership: Arc<super::input_runtime_binding::InputRuntimeOwnership>,
    // These are lifetime pins of the old assignment, not cloned subscriber permissions.
    _old_source: Option<SampleBuffer>,
    _old_lease: Option<CommittedColdLease>,
    _old_original_pin: Arc<super::project_assets::ProjectAssetLease>,
}

#[pymethods]
impl MaterialMigrationSourceTicket {
    #[getter]
    pub fn request_id(&self) -> u64 {
        self.admitted.request_id
    }
    #[getter]
    pub fn sample_id(&self) -> usize {
        self.id
    }
    pub fn phase(&self) -> PyResult<&'static str> {
        let publication = self
            .admitted
            .publication
            .lock()
            .map_err(|_| PyRuntimeError::new_err("cold publication lock poisoned"))?;
        match self.admitted.adoption.load(Ordering::Acquire) {
            0 => Ok("pending"),
            1 => Ok("claimed"),
            2 => Ok("acknowledged"),
            3 if publication.is_some()
                && !self.admitted.cancellation_settled.load(Ordering::Acquire) =>
            {
                Ok("pending")
            }
            3 => Ok("rejected"),
            _ => Err(PyRuntimeError::new_err(
                "unknown native migration adoption phase",
            )),
        }
    }
    pub fn cancel_unclaimed(&self) -> bool {
        cold_load::cancel_unclaimed_material(
            &self.requests,
            &self.admitted.publication,
            &self.admitted.adoption,
            &self.admitted.cancellation_settled,
            &self.ownership,
        )
    }
    /// Feedback/request advancement cannot substitute for current source identity.
    pub fn is_current(&self) -> PyResult<bool> {
        let _requests = self
            .requests
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        let publication = self
            .admitted
            .publication
            .lock()
            .map_err(|_| PyRuntimeError::new_err("cold publication lock poisoned"))?;
        let Some(publication) = publication.as_ref() else {
            return Ok(false);
        };
        if !Arc::ptr_eq(&publication.ownership, &self.ownership) {
            return Ok(false);
        }
        let Some(key) = publication.key() else {
            return Ok(false);
        };
        let id = match self.ownership.observe_control_key(key) {
            PlacementObservation::Current(snapshot) => snapshot.slot(),
            PlacementObservation::Pending => return Ok(false),
            PlacementObservation::Stale | PlacementObservation::Retired => return Ok(false),
        };
        let cache = self
            .cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("source cache lock poisoned"))?;
        let generations = self
            .generations
            .lock()
            .map_err(|_| PyRuntimeError::new_err("source generation lock poisoned"))?;
        let digests = self
            .digests
            .lock()
            .map_err(|_| PyRuntimeError::new_err("source digest lock poisoned"))?;
        let original =
            self.material.lease.manifest.descriptor["decoder"]["original"]["sha256"].as_str();
        let current = self.admitted.adoption.load(Ordering::Acquire) == 2
            && generations[id] == (self.admitted.request_id, self.rate)
            && digests[id].as_deref() == original
            && cache[id].as_ref().is_some_and(|sample| {
                // Retain the original publication key/binding. Its physical slot
                // snapshot expires on Pair, while its cell/source/window fences survive.
                let binding = publication.binding(sample, self.rate);
                self.ownership.binding_source_generation(id, binding)
                    == Some(publication.source_generation)
                    && self.ownership.binding_window_current(id, binding)
                    && sample.frame_count() == self.material.sample.frame_count()
                    && sample.channels == self.material.sample.channels
            });
        match self.ownership.observe_control_key(key) {
            PlacementObservation::Current(snapshot) if snapshot.slot() == id => Ok(current),
            PlacementObservation::Pending => Ok(false),
            _ => Ok(false),
        }
    }
}

pub(super) fn prepare(
    engine: &AudioEngine,
    source: String,
) -> PyResult<MaterialMigrationPreparation> {
    let stream = engine
        .stream_handle
        .as_ref()
        .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
    let root = std::env::current_dir()
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?
        .join("samples");
    let path = super::material_paths::resolve(&root, Path::new(&source))
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
    if !matches!(path.kind, super::material_paths::AssetKind::Original { .. }) {
        return Err(PyValueError::new_err(
            "migration requires a typed project original",
        ));
    }
    let source_pin = Arc::new(
        engine
            .project_assets
            .acquire_pin(&root, &path.path)
            .map_err(|error| PyValueError::new_err(error.to_string()))?,
    );
    let held_source_pin = source_pin.clone();
    let reservation = engine
        .cold_jobs
        .reserve()
        .map_err(PyRuntimeError::new_err)?;
    let result = Arc::new(Mutex::new(None));
    let cancelled = Arc::new(AtomicBool::new(false));
    let output = result.clone();
    let cancellation = cancelled.clone();
    let shutdown = engine.cold_cancelled.clone();
    let rate = stream.output_sample_rate;
    let channels = stream.output_channels;
    engine
        .cold_jobs
        .submit(reservation, move || {
            let _pin = source_pin;
            let cancelled =
                || cancellation.load(Ordering::Acquire) || shutdown.load(Ordering::Acquire);
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                material_migration::prepare_material(&root, &path.path, rate, channels, &cancelled)
            }))
            .unwrap_or_else(|_| Err("material migration worker failed".into()));
            if let Ok(mut result) = output.lock() {
                let outcome = if cancelled() {
                    if let Ok(material) = &outcome {
                        material.lease.rollback_unadopted_original();
                        material.lease.rollback_unadopted_cache();
                    }
                    Err("material migration preparation cancelled".into())
                } else {
                    outcome
                };
                *result = Some(outcome.map(Arc::new));
            }
        })
        .map_err(PyRuntimeError::new_err)?;
    Ok(MaterialMigrationPreparation {
        result,
        cancelled,
        admissions: Mutex::new(Vec::new()),
        requests: engine.pad_request_ids.clone(),
        _old_original_pin: held_source_pin,
    })
}

pub(super) fn adopt(
    engine: &AudioEngine,
    id: usize,
    preparation: &MaterialMigrationPreparation,
) -> PyResult<MaterialMigrationSourceTicket> {
    let stream = engine
        .stream_handle
        .as_ref()
        .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
    adopt_for_format(
        engine,
        id,
        preparation,
        stream.producer.clone(),
        (
            stream.output_channels,
            stream.output_sample_rate,
            std::env::current_dir()
                .map_err(|error| PyRuntimeError::new_err(error.to_string()))?
                .join("samples"),
        ),
    )
}

pub(super) fn adopt_for_format(
    engine: &AudioEngine,
    id: usize,
    preparation: &MaterialMigrationPreparation,
    producer: Arc<Mutex<rtrb::Producer<crate::messages::ControlMessage>>>,
    output: (usize, u32, std::path::PathBuf),
) -> PyResult<MaterialMigrationSourceTicket> {
    let mut admissions = preparation
        .admissions
        .lock()
        .map_err(|_| PyRuntimeError::new_err("migration admission lock poisoned"))?;
    if preparation.cancelled.load(Ordering::Acquire) {
        return Err(PyValueError::new_err("migration preparation cancelled"));
    }
    admissions
        .try_reserve(1)
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    let material = preparation.material()?;
    let source = material.metadata()["new_reference"]
        .as_str()
        .ok_or_else(|| PyRuntimeError::new_err("migration original reference missing"))?
        .to_owned();
    let admitted = cold_load::admit_selected_material(
        engine,
        id,
        source,
        (false, false, false, None),
        producer,
        output.clone(),
        "restore",
        Some(material.clone()),
    )?;
    admissions.push(MigrationAdmission {
        adoption: admitted.adoption.clone(),
        publication: admitted.publication.clone(),
        cancellation_settled: admitted.cancellation_settled.clone(),
    });
    Ok(MaterialMigrationSourceTicket {
        _old_source: admitted.old_source.clone(),
        _old_lease: admitted.old_lease.clone(),
        _old_original_pin: preparation._old_original_pin.clone(),
        admitted,
        id,
        rate: output.1,
        material,
        requests: engine.pad_request_ids.clone(),
        generations: engine.loaded_source_generations.clone(),
        digests: engine.loaded_source_digests.clone(),
        cache: engine.sample_cache.clone(),
        ownership: engine.input_runtime_ownership.clone(),
    })
}

#[pyclass]
pub struct MaterialMigrationHold {
    owner: u64,
    cells: Vec<(usize, ContentCell)>,
    ownership: Arc<super::input_runtime_binding::InputRuntimeOwnership>,
}

#[pymethods]
impl MaterialMigrationHold {
    /// Transfer existing native holds without reopening a launch revision.
    pub fn take(&mut self, ids: Vec<usize>) -> PyResult<Self> {
        if ids.is_empty()
            || ids.iter().collect::<std::collections::HashSet<_>>().len() != ids.len()
            || ids.iter().any(|id| {
                self.cells
                    .iter()
                    .find(|(admitted, _)| admitted == id)
                    .is_none_or(|(_, cell)| {
                        self.ownership.migration_hold_for_cell(*cell) != self.owner
                    })
            })
        {
            return Err(PyValueError::new_err(
                "migration hold transfer is outside its actual owner set",
            ));
        }
        let cells = self
            .cells
            .iter()
            .copied()
            .filter(|(id, _)| ids.contains(id))
            .collect();
        self.cells.retain(|(id, _)| !ids.contains(id));
        Ok(Self {
            owner: self.owner,
            cells,
            ownership: self.ownership.clone(),
        })
    }
    pub fn release(&self) -> PyResult<()> {
        if self.cells.is_empty() {
            return Ok(());
        }
        self.ownership
            .release_migration_hold_cells(
                &self.cells.iter().map(|(_, cell)| *cell).collect::<Vec<_>>(),
                self.owner,
            )
            .map_err(PyRuntimeError::new_err)
    }
}

pub(super) fn hold(engine: &AudioEngine, ids: Vec<usize>) -> PyResult<MaterialMigrationHold> {
    let producer = engine
        .stream_handle
        .as_ref()
        .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?
        .producer
        .clone();
    hold_for_producer(engine, ids, &producer)
}

pub(super) fn hold_for_producer(
    engine: &AudioEngine,
    ids: Vec<usize>,
    producer: &Arc<Mutex<rtrb::Producer<crate::messages::ControlMessage>>>,
) -> PyResult<MaterialMigrationHold> {
    static NEXT_HOLD: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    if ids.is_empty()
        || ids.len() > super::constants::NUM_SAMPLES
        || ids.iter().any(|&id| id >= super::constants::NUM_SAMPLES)
        || ids.iter().collect::<std::collections::HashSet<_>>().len() != ids.len()
    {
        return Err(PyValueError::new_err("invalid migration pad set"));
    }
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let producer = producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?;
    let owner = NEXT_HOLD
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |owner| {
            owner.checked_add(1)
        })
        .map_err(|_| PyRuntimeError::new_err("migration hold generation exhausted"))?;
    let cells = ids
        .iter()
        .map(|id| {
            let snapshot = engine
                .input_runtime_ownership
                .slot_snapshot(*id)
                .ok_or_else(|| PyRuntimeError::new_err("migration hold placement is pending"))?;
            if engine
                .input_runtime_ownership
                .control_slot_for_cell(snapshot.cell())
                != *id
            {
                return Err(PyRuntimeError::new_err(
                    "migration hold control placement is pending",
                ));
            }
            Ok((*id, snapshot.cell()))
        })
        .collect::<PyResult<Vec<_>>>()?;
    engine
        .input_runtime_ownership
        .commit_migration_holds(&ids, owner, true)
        .map_err(PyRuntimeError::new_err)?;
    drop(producer);
    drop(requests);
    Ok(MaterialMigrationHold {
        owner,
        cells,
        ownership: engine.input_runtime_ownership.clone(),
    })
}

#[pyclass]
pub struct MaterialMigrationStemPreparation {
    result:
        Arc<Mutex<Option<Result<super::material_migration_stems::CopiedStemGeneration, String>>>>,
    cancelled: Arc<AtomicBool>,
}

#[pymethods]
impl MaterialMigrationStemPreparation {
    pub fn status(&self) -> PyResult<&'static str> {
        let result = self
            .result
            .lock()
            .map_err(|_| PyRuntimeError::new_err("migration stem preparation lock poisoned"))?;
        Ok(match result.as_ref() {
            None => "preparing",
            Some(Ok(_)) => "ready",
            Some(Err(_)) => "failed",
        })
    }
    pub fn cache_reference(&self) -> PyResult<String> {
        let result = self
            .result
            .lock()
            .map_err(|_| PyRuntimeError::new_err("migration stem preparation lock poisoned"))?;
        match result.as_ref() {
            Some(Ok(generation)) => Ok(generation.cache_reference.clone()),
            Some(Err(error)) => Err(PyValueError::new_err(error.clone())),
            None => Err(PyValueError::new_err("stem migration is still preparing")),
        }
    }
    pub fn created(&self) -> PyResult<bool> {
        let result = self
            .result
            .lock()
            .map_err(|_| PyRuntimeError::new_err("migration stem preparation lock poisoned"))?;
        match result.as_ref() {
            Some(Ok(generation)) => Ok(generation.created),
            Some(Err(error)) => Err(PyValueError::new_err(error.clone())),
            None => Err(PyValueError::new_err("stem migration is still preparing")),
        }
    }
    pub fn error(&self) -> PyResult<Option<String>> {
        let result = self
            .result
            .lock()
            .map_err(|_| PyRuntimeError::new_err("migration stem preparation lock poisoned"))?;
        Ok(result
            .as_ref()
            .and_then(|outcome| outcome.as_ref().err().cloned()))
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

pub(super) fn prepare_stems(
    engine: &AudioEngine,
    preparation: &MaterialMigrationPreparation,
    old_cache: String,
    old_source_version: String,
    new_source_version: String,
    generation_id: String,
) -> PyResult<MaterialMigrationStemPreparation> {
    let material = preparation.material()?;
    let root = std::env::current_dir()
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?
        .join("samples");
    let cache = super::material_paths::resolve(&root, Path::new(&old_cache))
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
    if !matches!(
        cache.kind,
        super::material_paths::AssetKind::StemDirectory { .. }
    ) {
        return Err(PyValueError::new_err(
            "migration cache is not a typed stem directory",
        ));
    }
    let pin = engine
        .project_assets
        .acquire_pin(&root, &cache.path)
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
    let reservation = engine
        .cold_jobs
        .reserve()
        .map_err(PyRuntimeError::new_err)?;
    let result = Arc::new(Mutex::new(None));
    let output = result.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancellation = cancelled.clone();
    let shutdown = engine.cold_cancelled.clone();
    engine
        .cold_jobs
        .submit(reservation, move || {
            let _pin = pin;
            let cancelled =
                || cancellation.load(Ordering::Acquire) || shutdown.load(Ordering::Acquire);
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                super::material_migration_stems::copy_stem_generation(
                    &root,
                    &material,
                    &cache.path,
                    &old_source_version,
                    &new_source_version,
                    &generation_id,
                    &cancelled,
                )
            }))
            .unwrap_or_else(|_| Err("material stem migration worker failed".into()));
            if let Ok(mut result) = output.lock() {
                *result = Some(outcome);
            }
        })
        .map_err(PyRuntimeError::new_err)?;
    Ok(MaterialMigrationStemPreparation { result, cancelled })
}

#[cfg(test)]
pub(super) fn prepared_for_test(
    engine: &AudioEngine,
    material: PreparedMigrationMaterial,
    root: &Path,
) -> PyResult<MaterialMigrationPreparation> {
    let pin = engine
        .project_assets
        .acquire_pin(
            root,
            &super::material_paths::resolve(
                root,
                std::path::Path::new(
                    material.metadata()["old_reference"]
                        .as_str()
                        .expect("verified old reference"),
                ),
            )
            .map_err(|error| PyValueError::new_err(error.to_string()))?
            .path,
        )
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
    Ok(MaterialMigrationPreparation {
        result: Arc::new(Mutex::new(Some(Ok(Arc::new(material))))),
        cancelled: Arc::new(AtomicBool::new(false)),
        admissions: Mutex::new(Vec::new()),
        requests: engine.pad_request_ids.clone(),
        _old_original_pin: Arc::new(pin),
    })
}
