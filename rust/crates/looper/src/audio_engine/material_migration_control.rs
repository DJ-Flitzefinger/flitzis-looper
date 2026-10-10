//! Bounded off-thread material preparation and held, actual source adoption phases.
use super::cold_store::CommittedColdLease;
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

#[pyclass]
pub struct MaterialMigrationPreparation {
    result: Arc<Mutex<Option<PreparationOutcome>>>,
    cancelled: Arc<AtomicBool>,
    admissions: Mutex<Vec<Arc<std::sync::atomic::AtomicU8>>>,
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
        if admissions
            .iter()
            .any(|phase| phase.load(Ordering::Acquire) != 3)
        {
            return Err(PyValueError::new_err(
                "migration source claim or acknowledgement is unresolved",
            ));
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
        match self.admitted.adoption.load(Ordering::Acquire) {
            0 => Ok("pending"),
            1 => Ok("claimed"),
            2 => Ok("acknowledged"),
            3 => Ok("rejected"),
            _ => Err(PyRuntimeError::new_err(
                "unknown native migration adoption phase",
            )),
        }
    }
    pub fn cancel_unclaimed(&self) -> bool {
        if self
            .admitted
            .adoption
            .compare_exchange(0, 3, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.ownership
                .cancel_cold(self.id, self.admitted.request_id);
            true
        } else {
            self.admitted.adoption.load(Ordering::Acquire) == 3
        }
    }
    /// Feedback/request advancement cannot substitute for current source identity.
    pub fn is_current(&self) -> PyResult<bool> {
        let _requests = self
            .requests
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
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
        Ok(self.admitted.adoption.load(Ordering::Acquire) == 2
            && generations[self.id] == (self.admitted.request_id, self.rate)
            && digests[self.id].as_deref() == original
            && cache[self.id].as_ref().is_some_and(|sample| {
                self.ownership.source_generation(self.id, sample, self.rate)
                    == Some(self.admitted.request_id)
                    && sample.frame_count() == self.material.sample.frame_count()
                    && sample.channels == self.material.sample.channels
            }))
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
    admissions.push(admitted.adoption.clone());
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
    ids: Vec<usize>,
    ownership: Arc<super::input_runtime_binding::InputRuntimeOwnership>,
}

#[pymethods]
impl MaterialMigrationHold {
    pub fn release(&self) -> PyResult<()> {
        if self
            .ids
            .iter()
            .any(|&id| self.ownership.migration_hold(id) != self.owner)
        {
            return Err(PyValueError::new_err("migration hold owner changed"));
        }
        for &id in &self.ids {
            self.ownership
                .release_migration(id, self.owner)
                .map_err(PyRuntimeError::new_err)?;
        }
        Ok(())
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
    if ids
        .iter()
        .any(|&id| engine.input_runtime_ownership.migration_hold(id) != 0)
    {
        return Err(PyValueError::new_err("pad already held by a migration"));
    }
    let owner = NEXT_HOLD
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |owner| {
            owner.checked_add(1)
        })
        .map_err(|_| PyRuntimeError::new_err("migration hold generation exhausted"))?;
    for &id in &ids {
        engine.input_runtime_ownership.hold_migration(id, owner);
    }
    drop(producer);
    drop(requests);
    Ok(MaterialMigrationHold {
        owner,
        ids,
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
        .acquire_pin(root, &material.old_original)
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
    Ok(MaterialMigrationPreparation {
        result: Arc::new(Mutex::new(Some(Ok(Arc::new(material))))),
        cancelled: Arc::new(AtomicBool::new(false)),
        admissions: Mutex::new(Vec::new()),
        _old_original_pin: Arc::new(pin),
    })
}
