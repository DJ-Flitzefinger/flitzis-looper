//! Productive bounded cold preparation and request-serialized publication.

use super::cold_jobs::PCM_LIMIT_BYTES;
use super::cold_residency::{self, ResidentLoadGuard, ResidentLoadHint};
use super::cold_store::{ColdTransaction, CommittedColdLease};
use super::content_placement::{PlacementObservation, SlotSnapshot};
use super::input_runtime_binding::{InputRuntimeOwnership, SourcePublication};
use super::progress::{LoadProgressStage, ProgressReporter};
use super::sample_loader::{SampleLoadProgress, SampleLoadSubtask};
use super::{
    AudioEngine, LoadedSourcePublication, PadRequestAdvance, analyze_sample, pad_request_matches,
    publish_loaded_sample_with_arm,
};
use crate::messages::{ControlMessage, LoaderEvent, SampleBuffer};
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyInt};
use rtrb::Producer;
use std::collections::HashSet;
use std::path::{Component, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    mpsc::Sender,
};

type PreparedColdSample = (
    CommittedColdLease,
    SampleBuffer,
    Option<crate::messages::SampleAnalysis>,
    Option<f64>,
    Option<String>,
    Option<crate::messages::IncomingLoadDefaults>,
    Option<Arc<AtomicBool>>,
);

/// Initial bank settings are source-owned scalars, never a performer attack.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum SourcePitchInitialization {
    #[default]
    FreshNeutral,
    RestoredTuple {
        pitch: super::pitch_intent::PitchIntent,
        key_lock: Option<bool>,
    },
    PreserveEffective,
}

impl SourcePitchInitialization {
    pub(crate) fn resolve(
        self,
        effective: super::pitch_intent::PitchIntent,
        key_lock: bool,
    ) -> (super::pitch_intent::PitchIntent, bool) {
        match self {
            Self::FreshNeutral => (super::pitch_intent::PitchIntent::default(), key_lock),
            Self::RestoredTuple {
                pitch,
                key_lock: saved,
            } => (pitch, saved.unwrap_or(key_lock)),
            Self::PreserveEffective => (effective, key_lock),
        }
    }
    fn native_geometry_required(self) -> bool {
        match self {
            Self::FreshNeutral => false,
            Self::RestoredTuple { pitch, key_lock } => {
                key_lock.unwrap_or(false) || pitch.total_semitones() != 0
            }
            // Migration proves the same finite extent for either actual effective mode.
            // It never derives audible settings from a pending Python requested shadow.
            Self::PreserveEffective => true,
        }
    }
}

pub(super) fn parse_initial_pitch(
    source_intent: &str,
    base: Option<&Bound<'_, PyAny>>,
    extra: Option<&Bound<'_, PyAny>>,
    key_lock: Option<&Bound<'_, PyAny>>,
) -> PyResult<SourcePitchInitialization> {
    let base = base.filter(|value| !value.is_none());
    let extra = extra.filter(|value| !value.is_none());
    let key_lock = key_lock.filter(|value| !value.is_none());
    if base.is_none() && extra.is_none() && key_lock.is_none() {
        return Ok(SourcePitchInitialization::FreshNeutral);
    }
    if source_intent != "restore" {
        return Err(PyValueError::new_err(
            "initial pitch settings require explicit restore",
        ));
    }
    let integer = |value: Option<&Bound<'_, PyAny>>| -> PyResult<i32> {
        let Some(value) = value else {
            return Ok(0);
        };
        if value.is_instance_of::<PyBool>() || !value.is_instance_of::<PyInt>() {
            return Err(PyValueError::new_err(
                "initial pitch shifts must be strict integers",
            ));
        }
        value
            .extract::<i32>()
            .map_err(|_| PyValueError::new_err("initial pitch shift is outside its integer domain"))
    };
    let pitch = super::pitch_intent::PitchIntent::new(integer(base)?, integer(extra)?)
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
    let key_lock = key_lock
        .map(|value| {
            if !value.is_instance_of::<PyBool>() {
                return Err(PyValueError::new_err(
                    "initial_key_lock must be a strict boolean",
                ));
            }
            value.extract::<bool>()
        })
        .transpose()?;
    Ok(SourcePitchInitialization::RestoredTuple { pitch, key_lock })
}

#[cfg(test)]
struct AdoptionWaitGate {
    entered: AtomicBool,
    released: AtomicBool,
}

#[cfg(test)]
thread_local! {
    static ADOPTION_WAIT_GATE: std::cell::RefCell<Option<Arc<AdoptionWaitGate>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
struct AdoptionWaitGateGuard(Arc<AdoptionWaitGate>);

#[cfg(test)]
impl Drop for AdoptionWaitGateGuard {
    fn drop(&mut self) {
        self.0.released.store(true, Ordering::Release);
        ADOPTION_WAIT_GATE.with(|slot| {
            let installed = slot
                .borrow()
                .as_ref()
                .is_some_and(|gate| Arc::ptr_eq(gate, &self.0));
            if installed {
                slot.borrow_mut().take();
            }
        });
    }
}

#[cfg(test)]
fn gate_next_adoption_wait() -> AdoptionWaitGateGuard {
    let gate = Arc::new(AdoptionWaitGate {
        entered: AtomicBool::new(false),
        released: AtomicBool::new(false),
    });
    ADOPTION_WAIT_GATE.with(|slot| {
        assert!(slot.borrow().is_none(), "adoption gate already installed");
        *slot.borrow_mut() = Some(gate.clone());
    });
    AdoptionWaitGateGuard(gate)
}

struct LoadingGuard {
    id: usize,
    request_id: u64,
    slots: Arc<Vec<AtomicU64>>,
    loading: Arc<Mutex<HashSet<usize>>>,
}

/// Any outcome without metadata Success is an orphan assignment. Its files can
/// retire only after the registry's queued/bank/voice/job PCM readers disappear.
struct PublicationLease {
    lease: CommittedColdLease,
    adopted: bool,
    assets: Arc<super::project_assets::ProjectAssets>,
    _pcm: Arc<[f32]>,
    cleanup: ColdPublicationCleanup,
}

impl Drop for PublicationLease {
    fn drop(&mut self) {
        if !self.adopted && !self.cleanup.preserve_artifacts() {
            self.assets.orphan_cold(&self.lease);
            self.lease.rollback_unadopted_original();
            self.lease.rollback_unadopted_cache();
        }
    }
}

/// The worker's panic boundary retains cancellation authority even after its work unwinds.
#[derive(Clone)]
struct ColdPublicationCleanup {
    adoption: Arc<AtomicU8>,
    publication: Arc<Mutex<Option<SourcePublication>>>,
    cancellation_settled: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<u64>>>,
    ownership: Arc<InputRuntimeOwnership>,
    lease: Arc<Mutex<Option<CommittedColdLease>>>,
    assets: Arc<super::project_assets::ProjectAssets>,
}

impl ColdPublicationCleanup {
    fn cancel_unclaimed(&self) -> bool {
        cancel_unclaimed_material(
            &self.requests,
            &self.publication,
            &self.adoption,
            &self.cancellation_settled,
            &self.ownership,
        )
    }

    fn preserve_artifacts(&self) -> bool {
        let phase = self.adoption.load(Ordering::Acquire);
        if phase == 3 && self.cancellation_settled.load(Ordering::Acquire) {
            return false;
        }
        if matches!(phase, 1 | 2) || self.ownership.placement_pending() {
            return true;
        }
        match self.publication.lock() {
            Ok(publication) => {
                publication.is_some() && !self.cancellation_settled.load(Ordering::Acquire)
            }
            Err(_) => true,
        }
    }

    fn rollback_rejected_artifacts(&self) {
        if !self.cancellation_settled.load(Ordering::Acquire)
            || self.adoption.load(Ordering::Acquire) != 3
        {
            return;
        }
        let lease = self.lease.lock().ok().and_then(|lease| lease.clone());
        if let Some(lease) = lease {
            self.assets.orphan_cold(&lease);
            lease.rollback_unadopted_original();
            lease.rollback_unadopted_cache();
        }
    }
}

impl Drop for LoadingGuard {
    fn drop(&mut self) {
        if self.slots[self.id]
            .compare_exchange(self.request_id, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        if let Ok(mut loading) = self.loading.lock()
            && self.slots[self.id].load(Ordering::Acquire) == 0
        {
            loading.remove(&self.id);
        }
    }
}

struct PreviousSourceState {
    generation: (u64, u32),
    digest: Option<String>,
    intent: TimingIntent,
}

struct ColdLoad {
    placement: SlotSnapshot,
    id: usize,
    request_id: u64,
    base_request: u64,
    base_epoch: u64,
    proposal: super::resident_proposal::ResidentProposal,
    resident: super::resident_proposal::ResidentReadContext,
    adoption: Arc<AtomicU8>,
    publication: Arc<Mutex<Option<SourcePublication>>>,
    cancellation_settled: Arc<AtomicBool>,
    cleanup_lease: Arc<Mutex<Option<CommittedColdLease>>>,
    metadata_superseded: AtomicBool,
    #[cfg(test)]
    adoption_wait_gate: Option<Arc<AdoptionWaitGate>>,
    prepared_material: Option<Arc<super::material_migration::PreparedMigrationMaterial>>,
    path: PathBuf,
    restore: bool,
    initial_pitch: SourcePitchInitialization,
    samples_root: PathBuf,
    output_channels: usize,
    output_rate: u32,
    run_analysis: bool,
    restore_automatic: bool,
    replace_assignment: bool,
    resident_hint: Option<ResidentLoadHint>,
    resident_guard: Option<Arc<ResidentLoadGuard>>,
    incoming: Option<Arc<super::incoming_load::IncomingLoad>>,
    intents: Arc<Mutex<Vec<TimingIntent>>>,
    initial_source: Option<SampleBuffer>,
    requests: Arc<Mutex<Vec<u64>>>,
    epoch: Arc<AtomicU64>,
    expected_epoch: u64,
    guard_epoch: bool,
    cache: Arc<Mutex<Vec<Option<SampleBuffer>>>>,
    generations: Arc<Mutex<Vec<(u64, u32)>>>,
    digests: Arc<Mutex<Vec<Option<String>>>>,
    leases: Arc<Mutex<Vec<Option<CommittedColdLease>>>>,
    lease_generations: Arc<Vec<AtomicU64>>,
    pcm_history: Arc<super::project_assets::PcmHistory>,
    assets: Arc<super::project_assets::ProjectAssets>,
    engine_asset_owner: Arc<()>,
    _source_pin: Option<super::project_assets::ProjectAssetLease>,
    ownership: Arc<InputRuntimeOwnership>,
    cancelled: Arc<AtomicBool>,
    events: Sender<LoaderEvent>,
}

pub(super) fn admit(
    engine: &AudioEngine,
    id: usize,
    path: String,
    run_analysis: bool,
    restore_automatic: bool,
    replace_assignment: bool,
    resident_hint: Option<ResidentLoadHint>,
    source_intent: &str,
) -> PyResult<u64> {
    admit_with_initial_pitch(
        engine,
        id,
        path,
        run_analysis,
        restore_automatic,
        replace_assignment,
        resident_hint,
        source_intent,
        SourcePitchInitialization::FreshNeutral,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn admit_with_initial_pitch(
    engine: &AudioEngine,
    id: usize,
    path: String,
    run_analysis: bool,
    restore_automatic: bool,
    replace_assignment: bool,
    resident_hint: Option<ResidentLoadHint>,
    source_intent: &str,
    initial_pitch: SourcePitchInitialization,
) -> PyResult<u64> {
    // The saved source tuple owns the actual mode. The residency hint still
    // describes the same range, and its cancellation guard tracks that mode.
    let resident_hint = match initial_pitch {
        SourcePitchInitialization::RestoredTuple {
            key_lock: Some(key_lock),
            ..
        } => resident_hint.map(|hint| ResidentLoadHint { key_lock, ..hint }),
        _ => resident_hint,
    };
    #[cfg(test)]
    if let Some((producer, _)) = &engine.test_control_channels {
        // An explicitly installed headless command/feedback pair exercises this
        // real PyO3 route without constructing an App, CPAL stream or device.
        return admit_for_format_selected_intent_with_pitch(
            engine,
            id,
            path,
            (
                run_analysis,
                restore_automatic,
                replace_assignment,
                resident_hint,
            ),
            producer.clone(),
            (
                2,
                48_000,
                std::env::current_dir()
                    .map_err(|error| PyRuntimeError::new_err(error.to_string()))?
                    .join("samples"),
            ),
            source_intent,
            initial_pitch,
        );
    }
    let handle = engine
        .stream_handle
        .as_ref()
        .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
    admit_for_format_selected_intent_with_pitch(
        engine,
        id,
        path,
        (
            run_analysis,
            restore_automatic,
            replace_assignment,
            resident_hint,
        ),
        handle.producer.clone(),
        (
            handle.output_channels,
            handle.output_sample_rate,
            std::env::current_dir()
                .map_err(|error| PyRuntimeError::new_err(error.to_string()))?
                .join("samples"),
        ),
        source_intent,
        initial_pitch,
    )
}

// Kept separate so actual productive admission/preparation can be tested with a
// virtual command consumer without creating an audio device or a CPAL stream.
#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(super) fn admit_for_format(
    engine: &AudioEngine,
    id: usize,
    path: String,
    run_analysis: bool,
    restore_automatic: bool,
    replace_assignment: bool,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    output_channels: usize,
    output_rate: u32,
    samples_root: PathBuf,
) -> PyResult<u64> {
    admit_for_format_selected(
        engine,
        id,
        path,
        (run_analysis, restore_automatic, replace_assignment, None),
        producer,
        (output_channels, output_rate, samples_root),
    )
}

#[cfg(test)]
pub(super) fn admit_for_format_selected(
    engine: &AudioEngine,
    id: usize,
    path: String,
    selection: (bool, bool, bool, Option<ResidentLoadHint>),
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    output: (usize, u32, PathBuf),
) -> PyResult<u64> {
    admit_for_format_selected_intent(engine, id, path, selection, producer, output, "auto")
}

#[cfg(test)]
fn admit_for_format_selected_intent(
    engine: &AudioEngine,
    id: usize,
    path: String,
    selection: (bool, bool, bool, Option<ResidentLoadHint>),
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    output: (usize, u32, PathBuf),
    source_intent: &str,
) -> PyResult<u64> {
    admit_for_format_selected_intent_with_pitch(
        engine,
        id,
        path,
        selection,
        producer,
        output,
        source_intent,
        SourcePitchInitialization::FreshNeutral,
    )
}

#[allow(clippy::too_many_arguments)]
fn admit_for_format_selected_intent_with_pitch(
    engine: &AudioEngine,
    id: usize,
    path: String,
    selection: (bool, bool, bool, Option<ResidentLoadHint>),
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    output: (usize, u32, PathBuf),
    source_intent: &str,
    initial_pitch: SourcePitchInitialization,
) -> PyResult<u64> {
    admit_selected_material_incoming(
        engine,
        id,
        path,
        selection,
        producer,
        output,
        source_intent,
        None,
        None,
        initial_pitch,
    )
    .map(|admitted| admitted.request_id)
}

pub(super) struct AdmittedMaterial {
    pub request_id: u64,
    pub adoption: Arc<AtomicU8>,
    pub publication: Arc<Mutex<Option<SourcePublication>>>,
    pub cancellation_settled: Arc<AtomicBool>,
    pub old_source: Option<SampleBuffer>,
    pub old_lease: Option<CommittedColdLease>,
}

/// Request serialization separates a private preparation cancellation from cancellation of
/// the exact Native incarnation. Pending placement retains the handle for a later retry.
pub(super) fn cancel_unclaimed_material(
    requests: &Mutex<Vec<u64>>,
    publication: &Mutex<Option<SourcePublication>>,
    adoption: &AtomicU8,
    cancellation_settled: &AtomicBool,
    ownership: &InputRuntimeOwnership,
) -> bool {
    let Ok(_requests) = requests.lock() else {
        return false;
    };
    let Ok(publication) = publication.lock() else {
        return false;
    };
    let settled = if let Some(publication) = publication.as_ref() {
        ownership
            .cancel_cold_with_adoption_fixed(publication, adoption)
            .unwrap_or(false)
    } else {
        adoption
            .compare_exchange(0, 3, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            || adoption.load(Ordering::Acquire) == 3
    };
    if settled {
        cancellation_settled.store(true, Ordering::Release);
    }
    settled
}

#[allow(clippy::too_many_arguments)]
pub(super) fn admit_selected_material(
    engine: &AudioEngine,
    id: usize,
    path: String,
    selection: (bool, bool, bool, Option<ResidentLoadHint>),
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    output: (usize, u32, PathBuf),
    source_intent: &str,
    prepared_material: Option<Arc<super::material_migration::PreparedMigrationMaterial>>,
) -> PyResult<AdmittedMaterial> {
    let initial_pitch = if prepared_material.is_some() && !selection.2 {
        SourcePitchInitialization::PreserveEffective
    } else {
        SourcePitchInitialization::FreshNeutral
    };
    admit_selected_material_incoming(
        engine,
        id,
        path,
        selection,
        producer,
        output,
        source_intent,
        prepared_material,
        None,
        initial_pitch,
    )
}

pub(super) fn admit_incoming(
    engine: &AudioEngine,
    id: usize,
    path: String,
    key_lock: bool,
) -> PyResult<u64> {
    let producer = engine.control_producer()?.clone();
    #[cfg(test)]
    let output = if engine.test_control_channels.is_some() {
        (2, 48_000)
    } else {
        let handle = engine
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        (handle.output_channels, handle.output_sample_rate)
    };
    #[cfg(not(test))]
    let output = {
        let handle = engine
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        (handle.output_channels, handle.output_sample_rate)
    };
    let samples = std::env::current_dir()
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?
        .join("samples");
    admit_selected_material_incoming(
        engine,
        id,
        path,
        (true, false, true, None),
        producer,
        (output.0, output.1, samples),
        "import",
        None,
        Some(super::incoming_load::IncomingIntent::initial(key_lock)),
        SourcePitchInitialization::FreshNeutral,
    )
    .map(|admitted| admitted.request_id)
}

#[allow(clippy::too_many_arguments)]
fn admit_selected_material_incoming(
    engine: &AudioEngine,
    id: usize,
    path: String,
    selection: (bool, bool, bool, Option<ResidentLoadHint>),
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    output: (usize, u32, PathBuf),
    source_intent: &str,
    prepared_material: Option<Arc<super::material_migration::PreparedMigrationMaterial>>,
    incoming_intent: Option<super::incoming_load::IncomingIntent>,
    initial_pitch: SourcePitchInitialization,
) -> PyResult<AdmittedMaterial> {
    let (output_channels, output_rate, samples_root) = output;
    let (run_analysis, restore_automatic, replace_assignment, resident_hint) = selection;
    if id >= super::constants::NUM_SAMPLES {
        return Err(PyValueError::new_err("id out of range"));
    }
    let requested_path = PathBuf::from(&path);
    if requested_path
        .to_string_lossy()
        .split(['/', '\\'])
        .any(|part| [".", ".."].contains(&part))
    {
        return Err(PyValueError::new_err("source path contains traversal"));
    }
    let managed = if samples_root.exists() {
        super::material_paths::resolve(&samples_root, &requested_path).ok()
    } else {
        None
    };
    if managed.as_ref().is_some_and(|asset| {
        !matches!(
            asset.kind,
            super::material_paths::AssetKind::Original { .. }
        )
    }) || (managed.is_none()
        && samples_root.exists()
        && super::project_assets::owned_path(&samples_root, &requested_path).is_ok())
    {
        return Err(PyValueError::new_err(
            "managed source reference must name a typed original",
        ));
    }
    let restore = match source_intent {
        "import" => false,
        "restore" => {
            if !managed.as_ref().is_some_and(|asset| {
                matches!(
                    asset.kind,
                    super::material_paths::AssetKind::Original { .. }
                )
            }) {
                return Err(PyValueError::new_err(
                    "restore requires a typed project original",
                ));
            }
            true
        }
        "auto" => managed.as_ref().is_some_and(|asset| {
            matches!(
                asset.kind,
                super::material_paths::AssetKind::Original { .. }
            )
        }),
        _ => {
            return Err(PyValueError::new_err(
                "source_intent must be import, restore, or auto",
            ));
        }
    };
    if !restore
        && !requested_path.is_absolute()
        && requested_path.components().next() == Some(Component::Normal("samples".as_ref()))
    {
        // A malformed managed reference cannot become an external import.
        if managed.is_none() {
            return Err(PyValueError::new_err("invalid managed source reference"));
        }
    }
    let source_path = if restore {
        managed
            .as_ref()
            .expect("restore validated above")
            .path
            .clone()
    } else {
        requested_path
    };
    let mut requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    engine
        .input_runtime_ownership
        .ensure_no_copy_reservation(id)?;
    let reservation = engine
        .cold_jobs
        .reserve()
        .map_err(PyRuntimeError::new_err)?;
    let mut loading = engine
        .loading_sample_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("loading lock poisoned"))?;
    let mut resident_guards = engine
        .resident_restore_guards
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident intent lock poisoned"))?;
    if loading.contains(&id) {
        return Err(PyValueError::new_err("sample is already loading"));
    }
    let base_request = requests[id];
    let placement = engine
        .input_runtime_ownership
        .slot_snapshot(id)
        .ok_or_else(|| PyRuntimeError::new_err("content placement is pending"))?;
    let epoch = engine
        .prepared_source_epoch(id)
        .map_err(PyRuntimeError::new_err)?
        .clone();
    if !Arc::ptr_eq(
        &epoch,
        &engine.prepared_source_epochs[placement.cell().index()],
    ) {
        return Err(PyRuntimeError::new_err("content epoch capture changed"));
    }
    let base_epoch = epoch.load(Ordering::Acquire);
    let expected_epoch = {
        let advance = PadRequestAdvance::prepare(&mut requests[id], &epoch)
            .map_err(PyRuntimeError::new_err)?;
        advance.next_epoch
    };
    let (proposal, request_id) = engine
        .resident_proposals
        .begin_source_for_cell(id, placement.cell(), base_request, base_epoch)
        .map_err(PyRuntimeError::new_err)?;
    if request_id > u64::MAX / 4 {
        return Err(PyRuntimeError::new_err(
            "cold acknowledgement generation exhausted",
        ));
    }
    let initial_source = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?[id]
        .clone();
    // Material preparation carries an identity seed, not an ALL playback intent.
    // Derive its subscriber region from this request-protected source snapshot
    // so migration preserves actual finite residency without a second cache read.
    let resident_hint = resident_hint.or_else(|| {
        prepared_material.as_ref()?;
        initial_source
            .as_ref()
            .filter(|sample| {
                sample.resident_start() != 0 || sample.resident_end() != sample.frame_count()
            })
            .map(|sample| ResidentLoadHint {
                start_s: sample.resident_start() as f64 / f64::from(output_rate),
                end_s: sample.resident_end() as f64 / f64::from(output_rate),
                key_lock: false,
            })
    });
    let source_pin = if samples_root.exists() {
        super::project_assets::owned_path(&samples_root, &source_path)
            .ok()
            .map(|(_, owned)| engine.project_assets.acquire_pin(&samples_root, &owned))
            .transpose()
            .map_err(|error| PyValueError::new_err(error.to_string()))?
    } else {
        None
    };
    let resident_guard = resident_hint.map(|hint| {
        Arc::new(ResidentLoadGuard {
            request: request_id,
            hint,
            rate: output_rate,
            cancelled: Arc::new(AtomicBool::new(false)),
        })
    });
    let admitted = AdmittedMaterial {
        request_id,
        adoption: Arc::new(AtomicU8::new(0)),
        publication: Arc::new(Mutex::new(None)),
        cancellation_settled: Arc::new(AtomicBool::new(false)),
        old_source: initial_source.clone(),
        old_lease: engine
            .cold_leases
            .lock()
            .map_err(|_| PyRuntimeError::new_err("cold lease lock poisoned"))?[id]
            .clone(),
    };
    let incoming = incoming_intent.map(|intent| {
        Arc::new(super::incoming_load::IncomingLoad::new(
            request_id,
            intent,
            admitted.adoption.clone(),
        ))
    });
    let mut incoming_slots = engine
        .incoming_loads
        .lock()
        .map_err(|_| PyRuntimeError::new_err("incoming intent lock poisoned"))?;
    let work = ColdLoad {
        placement,
        id,
        request_id,
        base_request,
        base_epoch,
        proposal,
        resident: super::resident_proposal::ResidentReadContext::new(
            engine.project_assets.clone(),
            engine.resident_account.clone(),
        )
        .with_history(engine.cold_pcm_history.clone(), id),
        adoption: admitted.adoption.clone(),
        publication: admitted.publication.clone(),
        cancellation_settled: admitted.cancellation_settled.clone(),
        cleanup_lease: Arc::new(Mutex::new(None)),
        metadata_superseded: AtomicBool::new(false),
        #[cfg(test)]
        adoption_wait_gate: ADOPTION_WAIT_GATE.with(|gate| gate.borrow_mut().take()),
        prepared_material,
        path: source_path,
        restore,
        initial_pitch,
        samples_root,
        output_channels,
        output_rate,
        run_analysis,
        restore_automatic,
        replace_assignment,
        resident_hint,
        resident_guard: resident_guard.clone(),
        incoming: incoming.clone(),
        guard_epoch: run_analysis || restore_automatic || initial_source.is_some(),
        initial_source,
        epoch,
        expected_epoch,
        intents: engine.timing_intents.clone(),
        requests: engine.pad_request_ids.clone(),
        cache: engine.sample_cache.clone(),
        generations: engine.loaded_source_generations.clone(),
        digests: engine.loaded_source_digests.clone(),
        leases: engine.cold_leases.clone(),
        lease_generations: engine.cold_lease_generations.clone(),
        pcm_history: engine.cold_pcm_history.clone(),
        assets: engine.project_assets.clone(),
        engine_asset_owner: engine.project_asset_engine_owner.clone(),
        _source_pin: source_pin,
        ownership: engine.input_runtime_ownership.clone(),
        cancelled: engine.cold_cancelled.clone(),
        events: engine.loader_tx.clone(),
    };
    let panic_cleanup = work.publication_cleanup();
    let guard = LoadingGuard {
        id,
        request_id,
        slots: engine.cold_loading.clone(),
        loading: engine.loading_sample_ids.clone(),
    };
    engine
        .cold_jobs
        .submit(reservation, move || {
            let _guard = guard;
            let events = work.events.clone();
            run_worker_catching_panic_captured(panic_cleanup, &events, id, request_id, || {
                work.run(producer)
            });
        })
        .map_err(PyRuntimeError::new_err)?;
    // A worker's first current-request check takes this mutex. It cannot observe
    // the reservation before the request/loading commit has finished.
    resident_guards[id] = resident_guard;
    incoming_slots[id] = incoming;
    engine.cold_loading[id].store(request_id, Ordering::Release);
    loading.insert(id);
    drop(loading);
    drop(resident_guards);
    drop(incoming_slots);
    drop(requests);
    Ok(admitted)
}

#[cfg(test)]
pub(super) fn run_worker_catching_panic(
    adoption: Arc<AtomicU8>,
    events: &std::sync::mpsc::Sender<LoaderEvent>,
    id: usize,
    request_id: u64,
    run: impl FnOnce(),
) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).is_err() {
        let _ = adoption.compare_exchange(0, 3, Ordering::AcqRel, Ordering::Acquire);
        let _ = events.send(LoaderEvent::Error {
            id,
            request_id,
            error: "cold worker failed".into(),
        });
    }
}

fn run_worker_catching_panic_captured(
    cleanup: ColdPublicationCleanup,
    events: &Sender<LoaderEvent>,
    id: usize,
    request_id: u64,
    run: impl FnOnce(),
) {
    if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        let detail = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("unknown panic");
        let settled = cleanup.cancel_unclaimed();
        if settled {
            cleanup.rollback_rejected_artifacts();
        }
        let status = if settled {
            "native rejection settled"
        } else {
            "native permission unresolved or adopted; artifacts retained"
        };
        let diagnostic = format!("cold worker failed: {detail}; {status}");
        let event = if settled {
            LoaderEvent::Error {
                id,
                request_id,
                error: diagnostic,
            }
        } else {
            LoaderEvent::Progress {
                id,
                request_id,
                percent: 0.0,
                stage: diagnostic,
            }
        };
        let _ = events.send(event);
    }
}

/// The production unload transaction, also usable by headless virtual consumers.
pub(super) fn unload_for_producer(
    engine: &AudioEngine,
    id: usize,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
) -> PyResult<()> {
    if id >= super::constants::NUM_SAMPLES {
        return Err(PyValueError::new_err("id out of range"));
    }
    let mut requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    engine
        .input_runtime_ownership
        .ensure_no_copy_reservation(id)?;
    let captured = engine
        .input_runtime_ownership
        .slot_snapshot(id)
        .ok_or_else(|| PyRuntimeError::new_err("content placement is pending"))?;
    let write = engine
        .input_runtime_ownership
        .prepare_slot_write(captured, 0)
        .map_err(PyRuntimeError::new_err)?;
    let advance = PadRequestAdvance::prepare(
        &mut requests[id],
        engine
            .prepared_source_epoch(id)
            .map_err(PyRuntimeError::new_err)?,
    )
    .map_err(PyRuntimeError::new_err)?;
    let mut cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    let mut digests = engine
        .loaded_source_digests
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source digest lock poisoned"))?;
    let mut leases = engine
        .cold_leases
        .lock()
        .map_err(|_| PyRuntimeError::new_err("cold lease lock poisoned"))?;
    let mut loading = engine
        .loading_sample_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("loading lock poisoned"))?;
    let mut tasks = engine
        .active_tasks
        .lock()
        .map_err(|_| PyRuntimeError::new_err("task lock poisoned"))?;
    let mut producer = producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err(
            "Failed to send UnloadSample - buffer may be full",
        ));
    }
    let authority = engine.input_runtime_ownership.next_authority(id)?;
    let logical_advance = engine
        .input_runtime_ownership
        .prepare_source_request_advance(id)
        .map_err(PyRuntimeError::new_err)?;
    let publication = engine
        .input_runtime_ownership
        .retire_source_write(&write, authority, || {
            logical_advance.commit();
            advance.commit();
            true
        })
        .map_err(PyRuntimeError::new_err)?;
    producer
        .push(ControlMessage::UnloadSample {
            id,
            publication: Some(publication),
        })
        .expect("reserved single-producer capacity");
    cache[id] = None;
    digests[id] = None;
    leases[id] = None;
    engine.cold_lease_generations[id].store(0, Ordering::Release);
    engine.cold_loading[id].store(0, Ordering::Release);
    loading.remove(&id);
    tasks.retain(|(task_id, _)| *task_id != captured.cell().index());
    drop(producer);
    drop(tasks);
    drop(loading);
    drop(leases);
    drop(digests);
    drop(cache);
    drop(requests);
    engine.offline_jobs.cancel(Some(id));
    Ok(())
}

fn same_source(left: Option<&SampleBuffer>, right: Option<&SampleBuffer>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.channels == right.channels && Arc::ptr_eq(&left.samples, &right.samples)
        }
        _ => false,
    }
}

impl ColdLoad {
    fn publication_cleanup(&self) -> ColdPublicationCleanup {
        ColdPublicationCleanup {
            adoption: self.adoption.clone(),
            publication: self.publication.clone(),
            cancellation_settled: self.cancellation_settled.clone(),
            requests: self.requests.clone(),
            ownership: self.ownership.clone(),
            lease: self.cleanup_lease.clone(),
            assets: self.assets.clone(),
        }
    }
    fn reuses_assignment(&self, sample: &SampleBuffer) -> Result<bool, String> {
        let mut history = self
            .pcm_history
            .lock()
            .map_err(|_| "PCM history lock poisoned")?;
        history[self.id].retain(|reader| reader.strong_count() > 0);
        Ok(same_source(self.initial_source.as_ref(), Some(sample))
            || history[self.id]
                .iter()
                .any(|reader| reader.ptr_eq(&Arc::downgrade(&sample.samples))))
    }
    fn is_cancelled(&self) -> bool {
        self.flags_cancelled()
            || !pad_request_matches(
                &self.requests,
                self.id,
                if self.proposal.armed() {
                    self.request_id
                } else {
                    self.base_request
                },
            )
    }

    fn flags_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
            || self.adoption.load(Ordering::Acquire) == 3
            || (!self.proposal.armed() && !self.ownership.slot_snapshot_current(self.placement))
            || !self.proposal.current()
            || self
                .resident_guard
                .as_ref()
                .is_some_and(|guard| guard.cancelled.load(Ordering::Acquire))
            || ((!self.proposal.armed() || self.guard_epoch)
                && self.epoch.load(Ordering::Acquire)
                    != if self.proposal.armed() {
                        self.expected_epoch
                    } else {
                        self.base_epoch
                    })
    }

    fn prepare(&self) -> Result<PreparedColdSample, String> {
        let cancelled = || self.is_cancelled();
        if let Some(prepared) = &self.prepared_material {
            if cancelled() {
                return Err("material subscriber became stale".into());
            }
            // A material preparation seed grants no implicit full-track residency.
            // The actual subscriber's saved loop/ALL demand is read under the
            // same sealed complete lease before source queue admission.
            let source = &prepared.sample;
            let (start, end, context) = if let Some(hint) = self.resident_hint {
                hint.selection_for_native(
                    self.output_rate,
                    source.frame_count(),
                    self.initial_pitch.native_geometry_required(),
                )?
            } else {
                (
                    0,
                    source.frame_count(),
                    crate::messages::ResidentContext::FullTrack,
                )
            };
            let mut sample = prepared
                .lease
                .read_window_resident(
                    source,
                    start,
                    end,
                    1,
                    context,
                    PCM_LIMIT_BYTES,
                    &cancelled,
                    Some(&self.resident),
                )
                .map_err(|error| error.to_string())?;
            if self.reuses_assignment(&sample)? {
                let overlap = sample
                    .samples
                    .len()
                    .checked_mul(8)
                    .ok_or("migration PCM extent overflow")?;
                if overlap > PCM_LIMIT_BYTES {
                    return Err("migration subscriber exceeds transient PCM byte limit".into());
                }
                sample = self.resident.copy_selected(&sample)?;
            }
            let lease = prepared
                .lease
                .subscribe_verified()
                .map_err(|error| error.to_string())?;
            self.resident.commit_selected_history(&sample)?;
            lease
                .bind_sample(&sample)
                .map_err(|error| error.to_string())?;
            return Ok((lease, sample, None, None, None, None, None));
        }

        if self.incoming.is_some() {
            return self.prepare_incoming();
        }

        let mut transaction =
            ColdTransaction::capture(&self.samples_root, &self.path, !self.restore, &cancelled)
                .map_err(|error| error.to_string())?;
        // Each request keeps its own cancellation/ACK guard. Only full-content
        // preparation is serialized for the same digest/device interpretation.
        let _preparation = transaction
            .preparation_gate(self.output_rate, self.output_channels, &cancelled)
            .map_err(|error| error.to_string())?;
        let mut progress = ProgressReporter::new(self.id, self.request_id, self.events.clone());
        let mut report = |update: SampleLoadProgress| {
            let stage = match update.subtask {
                SampleLoadSubtask::Decoding => LoadProgressStage::Decoding,
                SampleLoadSubtask::Resampling => LoadProgressStage::Resampling,
                SampleLoadSubtask::ChannelMapping => LoadProgressStage::ChannelMapping,
            };
            progress.emit(
                stage,
                update.percent,
                update.resampling_required,
                update.percent <= 0.0 || update.percent >= 1.0,
            );
        };
        let mut sample = super::material_migration::prepare_pcm_resident_for_native(
            &mut transaction,
            &self.path,
            self.output_rate,
            self.output_channels,
            PCM_LIMIT_BYTES,
            self.resident_hint,
            &cancelled,
            &mut report,
            Some(&self.resident),
            self.initial_pitch.native_geometry_required(),
        )?;
        cold_residency::attach(
            &mut sample,
            transaction.manifest().map_err(|error| error.to_string())?,
        )?;
        // Same-pad backing readers retain their prior distinct PCM allocation.
        // Share complete PCM between distinct pads, but never re-use that identity
        // for a new assignment while any previous generation is still pinned.
        {
            if self.reuses_assignment(&sample)? {
                let overlap = sample
                    .samples
                    .len()
                    .checked_mul(8)
                    .ok_or("assignment PCM extent overflow")?;
                if overlap > PCM_LIMIT_BYTES {
                    return Err("same-pad warm assignment exceeds transient PCM byte limit".into());
                }
                sample = self.resident.copy_selected(&sample)?;
                transaction.record_assignment_copy((overlap / 2) as u64);
            }
        }
        let resampling_required = transaction
            .playback_transform()
            .map_err(|error| error.to_string())?["source_rate_hz"]
            .as_u64()
            .ok_or("missing prepared source rate")?
            != u64::from(self.output_rate);
        #[cfg(test)]
        super::c3_observation::owned_pcm(sample.samples.len() * 4);
        let detected = if self.resident_hint.is_some() {
            None
        } else {
            super::initial_loop_start::detect_initial_loop_start(&sample, self.output_rate)
        };
        let (analysis, analysis_error) = if self.run_analysis {
            // Automatic analysis is still requested. Its existing complete-input
            // kernels have a separate genuine RAM budget; they cannot force a
            // selected loop to own complete source PCM or veto durable import.
            let analysis_result = (|| -> Result<crate::messages::SampleAnalysis, String> {
                let finite =
                    sample.resident_start() != 0 || sample.resident_end() != sample.frame_count();
                let held = if finite {
                    sample
                        .samples
                        .len()
                        .checked_mul(4)
                        .ok_or("analysis held PCM overflow")?
                } else {
                    0
                };
                super::analysis_pcm::default_analysis_peak(
                    sample.frame_count(),
                    sample.channels,
                    self.output_rate,
                    held,
                    super::constant_timing::MAX_PCM_BYTES,
                )
                .map_err(|error| {
                    format!("automatic analysis exceeds its PCM preparation budget: {error}")
                })?;
                if cancelled() {
                    return Err("cold load cancelled".into());
                }
                progress.emit(LoadProgressStage::Analyzing, 0.0, resampling_required, true);
                let complete = if finite {
                    transaction
                        .read_prepared_selection(
                            None,
                            super::constant_timing::MAX_PCM_BYTES.saturating_sub(held),
                            &cancelled,
                        )
                        .map_err(|error| {
                            format!("automatic analysis PCM preparation failed: {error}")
                        })?
                } else {
                    sample.clone()
                };
                analyze_sample(&complete, self.output_rate)
            })();
            // Cancel/stale source can never become a successful load carrying an
            // incidental analysis warning. It still rolls back the transaction.
            if cancelled() {
                return Err("cold load cancelled".into());
            }
            match analysis_result {
                Ok(result) => (Some(result), None),
                Err(error) => (None, Some(error)),
            }
        } else {
            (None, None)
        };
        progress.emit(
            LoadProgressStage::Publishing,
            0.0,
            resampling_required,
            true,
        );
        transaction
            .commit(&cancelled)
            .map_err(|error| error.to_string())?;
        // prepare_pcm_resident already read the exact selected extent/context;
        // rewriting its Arc view would invalidate the scoped owner witness.
        self.resident.commit_selected_history(&sample)?;
        transaction
            .bind_sample(&sample)
            .map_err(|error| error.to_string())?;
        Ok((
            transaction.into_lease(),
            sample,
            analysis,
            detected,
            analysis_error,
            None,
            None,
        ))
    }

    fn prepare_incoming(&self) -> Result<PreparedColdSample, String> {
        let cancelled = || self.is_cancelled();
        let mut transaction =
            ColdTransaction::capture(&self.samples_root, &self.path, !self.restore, &cancelled)
                .map_err(|error| error.to_string())?;
        let _preparation = transaction
            .preparation_gate(self.output_rate, self.output_channels, &cancelled)
            .map_err(|error| error.to_string())?;
        let mut progress = ProgressReporter::new(self.id, self.request_id, self.events.clone());
        let mut report = |update: SampleLoadProgress| {
            let stage = match update.subtask {
                SampleLoadSubtask::Decoding => LoadProgressStage::Decoding,
                SampleLoadSubtask::Resampling => LoadProgressStage::Resampling,
                SampleLoadSubtask::ChannelMapping => LoadProgressStage::ChannelMapping,
            };
            progress.emit(
                stage,
                update.percent,
                update.resampling_required,
                update.percent <= 0.0 || update.percent >= 1.0,
            );
        };
        super::material_migration::prepare_disk_pcm(
            &mut transaction,
            &self.path,
            self.output_rate,
            self.output_channels,
            PCM_LIMIT_BYTES,
            &cancelled,
            &mut report,
        )?;
        let source =
            cold_residency::identity(transaction.manifest().map_err(|error| error.to_string())?)?;
        let detected = transaction
            .detect_prepared_initial_loop_start(&cancelled)
            .map_err(|error| error.to_string())?;
        let resampling_required = transaction
            .playback_transform()
            .map_err(|error| error.to_string())?["source_rate_hz"]
            .as_u64()
            .ok_or("missing prepared source rate")?
            != u64::from(self.output_rate);
        // This separate bounded complete-input analysis is retired before any
        // selected live allocation. An analysis failure cannot invent an end.
        let analysis_result = (|| -> Result<crate::messages::SampleAnalysis, String> {
            super::analysis_pcm::default_analysis_peak(
                source.frame_count,
                source.channels,
                self.output_rate,
                0,
                super::constant_timing::MAX_PCM_BYTES,
            )
            .map_err(|error| {
                format!("automatic analysis exceeds its PCM preparation budget: {error}")
            })?;
            progress.emit(LoadProgressStage::Analyzing, 0.0, resampling_required, true);
            let complete = transaction
                .read_prepared_selection(None, super::constant_timing::MAX_PCM_BYTES, &cancelled)
                .map_err(|error| format!("automatic analysis PCM preparation failed: {error}"))?;
            analyze_sample(&complete, self.output_rate)
        })();
        if cancelled() {
            return Err("cold load cancelled".into());
        }
        let (analysis, analysis_error) = match analysis_result {
            Ok(result) => (Some(result), None),
            Err(error) => (None, Some(error)),
        };
        let incoming = self.incoming.as_ref().expect("incoming request");
        let mut selected = None;
        // Bounded retries use one already verified complete artifact. Rapid
        // continuing edits truthfully reject rather than spin or adopt stale PCM.
        for _ in 0..4 {
            let (intent, candidate_cancelled) = incoming.snapshot()?;
            let defaults = intent.defaults(
                self.output_rate,
                source.frame_count,
                detected,
                analysis.as_ref(),
            )?;
            let candidate_stale = || cancelled() || candidate_cancelled.load(Ordering::Acquire);
            match transaction.read_prepared_selection_resident(
                defaults.resident_hint(self.output_rate, source.frame_count),
                PCM_LIMIT_BYTES,
                &candidate_stale,
                Some(&self.resident),
            ) {
                Ok(sample) if !candidate_stale() => {
                    selected = Some((sample, defaults, candidate_cancelled));
                    break;
                }
                _ if cancelled() => return Err("cold load cancelled".into()),
                _ if candidate_cancelled.load(Ordering::Acquire) => continue,
                Err(error) => return Err(error.to_string()),
                Ok(_) => return Err("incoming selection became stale".into()),
            }
        }
        let (mut sample, defaults, candidate_cancelled) =
            selected.ok_or("incoming intent kept changing during selection")?;
        {
            if self.reuses_assignment(&sample)? {
                let overlap = sample
                    .samples
                    .len()
                    .checked_mul(8)
                    .ok_or("assignment PCM extent overflow")?;
                if overlap > PCM_LIMIT_BYTES {
                    return Err("same-pad warm assignment exceeds transient PCM byte limit".into());
                }
                sample = self.resident.copy_selected(&sample)?;
                transaction.record_assignment_copy((overlap / 2) as u64);
            }
        }
        self.resident.commit_selected_history(&sample)?;
        #[cfg(test)]
        super::c3_observation::owned_pcm(sample.samples.len() * 4);
        if candidate_cancelled.load(Ordering::Acquire) {
            return Err("incoming intent became stale".into());
        }
        progress.emit(
            LoadProgressStage::Publishing,
            0.0,
            resampling_required,
            true,
        );
        transaction
            .commit(&cancelled)
            .map_err(|error| error.to_string())?;
        transaction
            .bind_sample(&sample)
            .map_err(|error| error.to_string())?;
        Ok((
            transaction.into_lease(),
            sample,
            analysis,
            detected,
            analysis_error,
            Some(defaults),
            Some(candidate_cancelled),
        ))
    }

    fn run(self, producer: Arc<Mutex<Producer<ControlMessage>>>) {
        let _ = self.events.send(LoaderEvent::Started {
            id: self.id,
            request_id: self.request_id,
        });
        let result = self.publish(&producer);
        if let Err(error) = result {
            let cleanup = self.publication_cleanup();
            let rejected = cleanup.cancel_unclaimed();
            if rejected {
                cleanup.rollback_rejected_artifacts();
            }
            let metadata_superseded = self.adoption.load(Ordering::Acquire) == 2
                && self.metadata_superseded.load(Ordering::Acquire);
            let event = if rejected || metadata_superseded {
                LoaderEvent::Error {
                    id: self.id,
                    request_id: self.request_id,
                    error,
                }
            } else {
                LoaderEvent::Progress {
                    id: self.id,
                    request_id: self.request_id,
                    percent: 0.0,
                    stage: format!(
                        "{error}; native permission unresolved or adopted; artifacts retained"
                    ),
                }
            };
            let _ = self.events.send(event);
        }
    }

    fn superseded_metadata_error(&self) -> String {
        self.metadata_superseded.store(true, Ordering::Release);
        "adopted cold source superseded".into()
    }

    fn await_adoption(&self) -> Result<(), (String, bool)> {
        #[cfg(test)]
        if let Some(gate) = &self.adoption_wait_gate {
            gate.entered.store(true, Ordering::Release);
            let gate_deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !gate.released.load(Ordering::Acquire) {
                assert!(
                    std::time::Instant::now() < gate_deadline,
                    "adoption test gate timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            match self.adoption.load(Ordering::Acquire) {
                2 => return Ok(()),
                phase @ (0 | 3)
                    if phase == 3
                        || self.is_cancelled()
                        || std::time::Instant::now() >= deadline =>
                {
                    if cancel_unclaimed_material(
                        &self.requests,
                        &self.publication,
                        &self.adoption,
                        &self.cancellation_settled,
                        &self.ownership,
                    ) {
                        return Err(("cold native adoption rejected".into(), false));
                    }
                    if self.cancelled.load(Ordering::Acquire)
                        || std::time::Instant::now() >= deadline
                        || self.requests.is_poisoned()
                        || self.publication.is_poisoned()
                    {
                        return Err((
                            "cold native cancellation unresolved; artifacts retained".into(),
                            true,
                        ));
                    }
                }
                1 if self.cancelled.load(Ordering::Acquire)
                    || std::time::Instant::now() >= deadline =>
                {
                    // Claimed callback work may already have changed the bank. Keep its
                    // complete files durable; never invent Success or roll back that source.
                    return Err((
                        "cold callback acknowledgement incomplete; artifacts retained".into(),
                        true,
                    ));
                }
                _ => {}
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    fn rollback_pending(
        &self,
        prepared: &SampleBuffer,
        previous: PreviousSourceState,
        enqueue_epoch: u64,
        publication: &SourcePublication,
    ) -> Result<(), String> {
        let _requests = self.requests.lock().map_err(|_| "request lock poisoned")?;
        let mut intents = self
            .intents
            .lock()
            .map_err(|_| "timing intent lock poisoned")?;
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| "sample cache lock poisoned")?;
        let mut generations = self
            .generations
            .lock()
            .map_err(|_| "source generation lock poisoned")?;
        let mut digests = self
            .digests
            .lock()
            .map_err(|_| "source digest lock poisoned")?;
        let key = publication.key().ok_or("source publication key missing")?;
        let target = match self.ownership.observe_content_key(key) {
            PlacementObservation::Current(snapshot) => snapshot.slot(),
            PlacementObservation::Pending => return Err("content placement is pending".into()),
            _ => return Ok(()),
        };
        if generations[target] == (self.request_id, self.output_rate)
            && same_source(cache[target].as_ref(), Some(prepared))
        {
            let restore_intent = self.epoch.load(Ordering::Acquire) == enqueue_epoch;
            if self.ownership.rollback_source_write(
                publication,
                prepared,
                self.output_rate,
                self.placement,
                self.initial_source
                    .as_ref()
                    .map(|sample| (sample, previous.generation.1, previous.generation.0)),
                restore_intent.then_some(previous.intent),
            )? {
                cache[target] = self.initial_source.clone();
                generations[target] = previous.generation;
                digests[target] = previous.digest;
                if restore_intent {
                    intents[target] = previous.intent;
                }
            }
        }
        Ok(())
    }

    fn publish(&self, producer: &Arc<Mutex<Producer<ControlMessage>>>) -> Result<(), String> {
        let (
            lease,
            sample,
            analysis,
            detected_loop_start_s,
            analysis_error,
            incoming_defaults,
            incoming_cancelled,
        ) = self.prepare()?;
        let source_digest = lease.manifest.descriptor["decoder"]["original"]["sha256"]
            .as_str()
            .ok_or("prepared original digest missing")?
            .to_owned();
        lease
            .bind_sample(&sample)
            .map_err(|error| error.to_string())?;
        *self
            .cleanup_lease
            .lock()
            .map_err(|_| "cold cleanup lease lock poisoned")? = Some(lease.clone());
        // Register before enqueue. Ordinary pre-ACK rejection still leaves a
        // queued PCM owner; file cleanup must follow that actual reader lifetime.
        let mut publication = PublicationLease {
            lease: lease.clone(),
            adopted: false,
            assets: self.assets.clone(),
            _pcm: sample.samples.clone(),
            cleanup: self.publication_cleanup(),
        };
        // Reserve before enqueue, with rollback guarded even if capacity fails.
        // The pin acknowledges only when the matching Python delivery is adopted.
        let original_lease = self
            .assets
            .acquire_pin(&self.samples_root, &lease.original_path)
            .map_err(|error| error.to_string())?;
        let mut resident_registration = self.resident.take_registration()?;
        self.assets
            .retain_cold_reserved(
                lease.clone(),
                &sample,
                Arc::downgrade(&self.engine_asset_owner),
                &mut resident_registration,
            )
            .map_err(|error| error.to_string())?;
        let mut requests = self.requests.lock().map_err(|_| "request lock poisoned")?;
        let mut captured_publication = self
            .publication
            .lock()
            .map_err(|_| "cold publication lock poisoned")?;
        if self.flags_cancelled()
            || incoming_cancelled
                .as_ref()
                .is_some_and(|cancelled| cancelled.load(Ordering::Acquire))
            || requests[self.id] != self.base_request
            || self.epoch.load(Ordering::Acquire) != self.base_epoch
        {
            return Err("cold request became stale".into());
        }
        {
            let cache = self
                .cache
                .lock()
                .map_err(|_| "sample cache lock poisoned")?;
            if !same_source(cache[self.id].as_ref(), self.initial_source.as_ref()) {
                return Err("cold source changed".into());
            }
        }
        let mut intents = self
            .intents
            .lock()
            .map_err(|_| "timing intent lock poisoned")?;
        let mut generations = self
            .generations
            .lock()
            .map_err(|_| "source generation lock poisoned")?;
        let mut digests = self
            .digests
            .lock()
            .map_err(|_| "source digest lock poisoned")?;
        let previous = PreviousSourceState {
            generation: generations[self.id],
            digest: digests[self.id].clone(),
            intent: intents[self.id],
        };
        // Successful arming advances this exact epoch. Every post-enqueue
        // metadata/rollback comparison must use the admitted epoch, rather
        // than the predecessor snapshot retained for preparation validation.
        let enqueue_epoch = self.expected_epoch;
        let project_root =
            std::fs::canonicalize(self.samples_root.parent().ok_or("samples parent missing")?)
                .map_err(|error| error.to_string())?;
        let cached_path = lease
            .original_path
            .strip_prefix(&project_root)
            .map_err(|_| "project original escaped verified project root")?
            .to_string_lossy()
            .replace('\\', "/");
        let duration_s = sample.frame_count() as f64 / f64::from(self.output_rate);
        let loop_region = self
            .resident_hint
            .map(|hint| hint.region(self.output_rate, sample.frame_count()))
            .transpose()?
            .map(|region| (region.start, region.end));
        let advance = PadRequestAdvance::prepare(&mut requests[self.id], &self.epoch)?;
        let logical_advance = self.ownership.prepare_source_request_advance(self.id)?;
        if advance.next_request > self.request_id || advance.next_epoch != self.expected_epoch {
            return Err("cold proposal current authority changed before arming".into());
        }
        let arm_rejected = "cold source proposal arm rejected".to_owned();
        let source_publication = publish_loaded_sample_with_arm(
            producer,
            &self.cache,
            self.id,
            sample.clone(),
            LoadedSourcePublication {
                placement: self.placement,
                ownership: &self.ownership,
                generation: self.request_id,
                rate: self.output_rate,
                generation_slot: &mut generations[self.id],
                digest_slot: &mut digests[self.id],
                digest: source_digest,
                cold: true,
                cold_epoch: self.guard_epoch.then(|| self.epoch.clone()),
                cold_adoption: Some(self.adoption.clone()),
                replace_assignment: self.replace_assignment,
                loop_region,
                incoming_defaults,
                initial_pitch: self.initial_pitch,
                resident_cancelled: incoming_cancelled.or_else(|| {
                    self.resident_guard
                        .as_ref()
                        .map(|guard| guard.cancelled.clone())
                }),
                intent: if incoming_defaults.is_some() {
                    Some((&mut intents[self.id], TimingIntent::Legacy))
                } else {
                    self.restore_automatic
                        .then_some((&mut intents[self.id], TimingIntent::Automatic))
                },
            },
            || {
                if self.adoption.load(Ordering::Acquire) != 0 || !self.proposal.try_arm() {
                    return Err(arm_rejected);
                }
                logical_advance.commit();
                advance.commit_reserved_prechecked(self.request_id);
                Ok(())
            },
        )?;
        *captured_publication = Some(source_publication.clone());
        drop(digests);
        drop(generations);
        drop(intents);
        drop(captured_publication);
        drop(requests);
        if let Err((error, may_be_adopted)) = self.await_adoption() {
            if may_be_adopted {
                publication.adopted = true;
                return Err(error);
            }
            lease.rollback_unadopted_original();
            lease.rollback_unadopted_cache();
            self.rollback_pending(&sample, previous, enqueue_epoch, &source_publication)?;
            return Err(error);
        }
        publication.adopted = true;
        // ACK retains native readers even if a newer request wins before metadata delivery.
        let metadata_deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let requests = self.requests.lock().map_err(|_| "request lock poisoned")?;
            let target = match self
                .ownership
                .observe_source_publication(&source_publication)
            {
                PlacementObservation::Current(snapshot) => snapshot.slot(),
                PlacementObservation::Pending => {
                    drop(requests);
                    if self.cancelled.load(Ordering::Acquire)
                        || std::time::Instant::now() >= metadata_deadline
                    {
                        return Err(
                            "acknowledged cold source metadata remains pending; artifacts retained"
                                .into(),
                        );
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                    continue;
                }
                _ => return Err(self.superseded_metadata_error()),
            };
            let cache = self
                .cache
                .lock()
                .map_err(|_| "sample cache lock poisoned")?;
            let generations = self
                .generations
                .lock()
                .map_err(|_| "source generation lock poisoned")?;
            let stale_source = (self.guard_epoch
                && self.epoch.load(Ordering::Acquire) != enqueue_epoch)
                || generations[target] != (self.request_id, self.output_rate)
                || !same_source(cache[target].as_ref(), Some(&sample))
                || !self.ownership.source_publication_identity_current(
                    &source_publication,
                    &sample,
                    self.output_rate,
                );
            // A writer may acquire its marker after the first observation. A second
            // observation distinguishes that transient fence from a real stale source.
            match self
                .ownership
                .observe_source_publication(&source_publication)
            {
                PlacementObservation::Pending => {
                    drop(generations);
                    drop(cache);
                    drop(requests);
                    if self.cancelled.load(Ordering::Acquire)
                        || std::time::Instant::now() >= metadata_deadline
                    {
                        return Err(
                            "acknowledged cold source metadata remains pending; artifacts retained"
                                .into(),
                        );
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                    continue;
                }
                PlacementObservation::Current(snapshot) if snapshot.slot() == target => {}
                _ => return Err(self.superseded_metadata_error()),
            }
            if stale_source {
                return Err(self.superseded_metadata_error());
            }
            // Only callback ACK transfers sealed original/artifact ownership and project metadata.
            self.leases.lock().map_err(|_| "cold lease lock poisoned")?[target] = Some(lease);
            self.lease_generations[target].store(self.request_id, Ordering::Release);
            self.events
                .send(
                    super::NativeAnalysisOwner::from_publication(
                        &source_publication,
                        &sample,
                        self.output_rate,
                    )
                    .event(LoaderEvent::Success {
                        timing_epoch: Some(enqueue_epoch),
                        id: target,
                        request_id: self.request_id,
                        duration_s,
                        detected_loop_start_s,
                        cached_path,
                        original_lease: Some(original_lease),
                        analysis,
                        analysis_error,
                        incoming_defaults,
                    }),
                )
                .map_err(|_| "native adoption metadata receiver closed")?;
            return Ok(());
        }
    }
}

#[cfg(test)]
#[path = "cold_load_tests.rs"]
pub(in crate::audio_engine) mod tests;

#[cfg(test)]
#[path = "cold_cancellation_publication_tests.rs"]
mod cancellation_publication_tests;

#[cfg(test)]
mod source_pitch_initialization_tests {
    use super::*;
    use crate::audio_engine::pitch_intent::PitchIntent;

    #[test]
    fn restore_parser_is_strict_and_does_not_grant_import_pitch_initialization() {
        Python::initialize();
        Python::attach(|py| {
            for source in ["import", "auto"] {
                let base = py.eval(c"2", None, None).unwrap();
                assert!(parse_initial_pitch(source, Some(&base), None, None).is_err());
            }
            for expression in [c"True", c"False", c"2.0", c"'2'", c"24", c"-24"] {
                let base = py.eval(expression, None, None).unwrap();
                assert!(parse_initial_pitch("restore", Some(&base), None, None).is_err());
            }
            for expression in [c"True", c"18.0", c"19", c"-19"] {
                let extra = py.eval(expression, None, None).unwrap();
                assert!(parse_initial_pitch("restore", None, Some(&extra), None).is_err());
            }
            let base = py.eval(c"-5", None, None).unwrap();
            let extra = py.eval(c"-18", None, None).unwrap();
            let lock = py.eval(c"False", None, None).unwrap();
            assert_eq!(
                parse_initial_pitch("restore", Some(&base), Some(&extra), Some(&lock)).unwrap(),
                SourcePitchInitialization::RestoredTuple {
                    pitch: PitchIntent::new(-5, -18).unwrap(),
                    key_lock: Some(false)
                }
            );
            for expression in [c"0", c"1", c"'false'"] {
                let lock = py.eval(expression, None, None).unwrap();
                assert!(parse_initial_pitch("restore", None, None, Some(&lock)).is_err());
            }
            assert_eq!(
                parse_initial_pitch("import", None, None, None).unwrap(),
                SourcePitchInitialization::FreshNeutral
            );
        });
    }

    #[test]
    fn invalid_public_restore_options_leave_source_request_and_workers_untouched() {
        Python::initialize();
        Python::attach(|py| {
            let engine = AudioEngine::new().unwrap();
            engine.pad_request_ids.lock().unwrap()[0] = 17;
            engine.loaded_source_generations.lock().unwrap()[0] = (13, 48_000);
            let epoch = engine.prepared_source_epochs[0].load(Ordering::Acquire);
            for (source, base, extra, lock) in [
                ("restore", c"True", c"None", c"None"),
                ("restore", c"0", c"19", c"None"),
                ("restore", c"0", c"0", c"0"),
                ("import", c"0", c"0", c"False"),
            ] {
                let base = py.eval(base, None, None).unwrap();
                let extra = py.eval(extra, None, None).unwrap();
                let lock = py.eval(lock, None, None).unwrap();
                let error = engine
                    .load_sample_async_with_pitch(
                        0,
                        "not-opened.wav".into(),
                        Some(false),
                        false,
                        true,
                        None,
                        None,
                        false,
                        source,
                        Some(&base),
                        Some(&extra),
                        Some(&lock),
                    )
                    .unwrap_err();
                assert!(error.is_instance_of::<PyValueError>(py));
                assert_eq!(engine.pad_request_ids.lock().unwrap()[0], 17);
                assert_eq!(
                    engine.loaded_source_generations.lock().unwrap()[0],
                    (13, 48_000)
                );
                assert_eq!(
                    engine.prepared_source_epochs[0].load(Ordering::Acquire),
                    epoch
                );
                assert!(engine.active_tasks.lock().unwrap().is_empty());
                assert!(engine.loading_sample_ids.lock().unwrap().is_empty());
            }
        });
    }

    #[test]
    fn finite_native_requirement_never_changes_actual_saved_lock_or_geometry() {
        let hint = ResidentLoadHint {
            start_s: 100.0 / 48_000.0,
            end_s: 1100.0 / 48_000.0,
            key_lock: false,
        };
        let restored = SourcePitchInitialization::RestoredTuple {
            pitch: PitchIntent::new(6, 18).unwrap(),
            key_lock: Some(false),
        };
        assert_eq!(
            hint.selection_for_native(48_000, 4096, restored.native_geometry_required())
                .unwrap(),
            (
                100,
                1100,
                crate::messages::ResidentContext::KeyLockFiniteLoop
            )
        );
        assert!(!hint.key_lock);
        assert_eq!(
            restored.resolve(PitchIntent::default(), true),
            (PitchIntent::new(6, 18).unwrap(), false)
        );
        assert_eq!(
            SourcePitchInitialization::PreserveEffective
                .resolve(PitchIntent::new(-3, 5).unwrap(), false),
            (PitchIntent::new(-3, 5).unwrap(), false)
        );
    }

    #[test]
    fn source_bank_initializer_restores_effective_tuple_without_retuning_retained_voice() {
        crate::audio_engine::mixer::RtMixer::assert_source_pitch_initialization_preserves_retained_voice_for_test();
    }
}
