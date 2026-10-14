//! Productive bounded cold preparation and request-serialized publication.

use super::cold_jobs::PCM_LIMIT_BYTES;
use super::cold_residency::{self, ResidentLoadGuard, ResidentLoadHint};
use super::cold_store::{ColdTransaction, CommittedColdLease};
use super::input_runtime_binding::InputRuntimeOwnership;
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
}

impl Drop for PublicationLease {
    fn drop(&mut self) {
        if !self.adopted {
            self.assets.orphan_cold(&self.lease);
            self.lease.rollback_unadopted_original();
            self.lease.rollback_unadopted_cache();
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
    content_lifetime: u64,
}

struct ColdLoad {
    id: usize,
    request_id: u64,
    base_request: u64,
    base_epoch: u64,
    proposal: super::resident_proposal::ResidentProposal,
    resident: super::resident_proposal::ResidentReadContext,
    adoption: Arc<AtomicU8>,
    prepared_material: Option<Arc<super::material_migration::PreparedMigrationMaterial>>,
    path: PathBuf,
    restore: bool,
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
    #[cfg(test)]
    if let Some((producer, _)) = &engine.test_control_channels {
        // An explicitly installed headless command/feedback pair exercises this
        // real PyO3 route without constructing an App, CPAL stream or device.
        return admit_for_format_selected_intent(
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
        );
    }
    let handle = engine
        .stream_handle
        .as_ref()
        .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
    admit_for_format_selected_intent(
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

fn admit_for_format_selected_intent(
    engine: &AudioEngine,
    id: usize,
    path: String,
    selection: (bool, bool, bool, Option<ResidentLoadHint>),
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    output: (usize, u32, PathBuf),
    source_intent: &str,
) -> PyResult<u64> {
    admit_selected_material(
        engine,
        id,
        path,
        selection,
        producer,
        output,
        source_intent,
        None,
    )
    .map(|admitted| admitted.request_id)
}

pub(super) struct AdmittedMaterial {
    pub request_id: u64,
    pub adoption: Arc<AtomicU8>,
    pub old_source: Option<SampleBuffer>,
    pub old_lease: Option<CommittedColdLease>,
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
    let base_epoch = engine.prepared_source_epochs[id].load(Ordering::Acquire);
    let expected_epoch = {
        let advance =
            PadRequestAdvance::prepare(&mut requests[id], &engine.prepared_source_epochs[id])
                .map_err(PyRuntimeError::new_err)?;
        advance.next_epoch
    };
    let (proposal, request_id) = engine
        .resident_proposals
        .begin_source(id, base_request, base_epoch)
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
        prepared_material,
        path: source_path,
        restore,
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
        epoch: engine.prepared_source_epochs[id].clone(),
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
    let panic_adoption = work.adoption.clone();
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
            run_worker_catching_panic(panic_adoption, &events, id, request_id, || {
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
    let advance = PadRequestAdvance::prepare(&mut requests[id], &engine.prepared_source_epochs[id])
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
    engine.input_runtime_ownership.revoke(id, authority);
    advance.commit();
    engine.input_runtime_ownership.revoke_source(id);
    engine
        .input_runtime_ownership
        .publish_content_lifetime(id, 0);
    engine.input_runtime_ownership.clear_launch_admitted(id);
    producer
        .push(ControlMessage::UnloadSample { id })
        .expect("reserved single-producer capacity");
    cache[id] = None;
    digests[id] = None;
    leases[id] = None;
    engine.cold_lease_generations[id].store(0, Ordering::Release);
    engine.cold_loading[id].store(0, Ordering::Release);
    loading.remove(&id);
    tasks.retain(|(task_id, _)| *task_id != id);
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
            || !self.proposal.current()
            || self
                .resident_guard
                .as_ref()
                .is_some_and(|guard| guard.cancelled.load(Ordering::Acquire))
            || self.epoch.load(Ordering::Acquire)
                != if self.proposal.armed() {
                    self.expected_epoch
                } else {
                    self.base_epoch
                }
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
                hint.selection(self.output_rate, source.frame_count())?
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
        let mut sample = super::material_migration::prepare_pcm_resident(
            &mut transaction,
            &self.path,
            self.output_rate,
            self.output_channels,
            PCM_LIMIT_BYTES,
            self.resident_hint,
            &cancelled,
            &mut report,
            Some(&self.resident),
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
            let _ = self
                .adoption
                .compare_exchange(0, 3, Ordering::AcqRel, Ordering::Acquire);
            let _ = self.events.send(LoaderEvent::Error {
                id: self.id,
                request_id: self.request_id,
                error,
            });
        }
    }

    fn await_adoption(&self) -> Result<(), (String, bool)> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            match self.adoption.load(Ordering::Acquire) {
                2 => return Ok(()),
                3 => return Err(("cold native adoption rejected".into(), false)),
                0 if (self.is_cancelled() || std::time::Instant::now() >= deadline)
                    && self
                        .adoption
                        .compare_exchange(0, 3, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok() =>
                {
                    self.ownership.cancel_cold(self.id, self.request_id);
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
        published_lifetime: u64,
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
        if generations[self.id] == (self.request_id, self.output_rate)
            && same_source(cache[self.id].as_ref(), Some(prepared))
            && self
                .ownership
                .content_lifetime_current(self.id, published_lifetime)
        {
            cache[self.id] = self.initial_source.clone();
            generations[self.id] = previous.generation;
            digests[self.id] = previous.digest;
            self.ownership
                .publish_content_lifetime(self.id, previous.content_lifetime);
            if self.epoch.load(Ordering::Acquire) == enqueue_epoch {
                intents[self.id] = previous.intent;
                self.ownership.set_timing_intent(self.id, previous.intent);
            }
            if let Some(previous_sample) = self.initial_source.as_ref() {
                self.ownership.publish_source(
                    self.id,
                    previous_sample,
                    previous.generation.1,
                    previous.generation.0,
                );
            } else {
                self.ownership.revoke_source(self.id);
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
        // Register before enqueue. Ordinary pre-ACK rejection still leaves a
        // queued PCM owner; file cleanup must follow that actual reader lifetime.
        let mut publication = PublicationLease {
            lease: lease.clone(),
            adopted: false,
            assets: self.assets.clone(),
            _pcm: sample.samples.clone(),
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
            content_lifetime: self.ownership.content_lifetime(self.id),
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
        if advance.next_request > self.request_id || advance.next_epoch != self.expected_epoch {
            return Err("cold proposal current authority changed before arming".into());
        }
        publish_loaded_sample_with_arm(
            producer,
            &self.cache,
            self.id,
            sample.clone(),
            LoadedSourcePublication {
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
                self.proposal.arm()?;
                advance.commit_reserved(self.request_id)?;
                Ok(())
            },
        )?;
        let published_lifetime = self.ownership.content_lifetime(self.id);
        drop(digests);
        drop(generations);
        drop(intents);
        drop(requests);
        if let Err((error, may_be_adopted)) = self.await_adoption() {
            if may_be_adopted {
                lease.rollback_unadopted_original();
                let _requests = self.requests.lock().map_err(|_| "request lock poisoned")?;
                let cache = self
                    .cache
                    .lock()
                    .map_err(|_| "sample cache lock poisoned")?;
                let generations = self
                    .generations
                    .lock()
                    .map_err(|_| "source generation lock poisoned")?;
                if generations[self.id] == (self.request_id, self.output_rate)
                    && same_source(cache[self.id].as_ref(), Some(&sample))
                {
                    self.ownership.revoke_source(self.id);
                    self.leases.lock().map_err(|_| "cold lease lock poisoned")?[self.id] =
                        Some(lease);
                    self.lease_generations[self.id].store(0, Ordering::Release);
                }
                return Err(error);
            }
            lease.rollback_unadopted_original();
            lease.rollback_unadopted_cache();
            self.rollback_pending(&sample, previous, enqueue_epoch, published_lifetime)?;
            return Err(error);
        }
        // ACK retains native readers even if a newer request wins before metadata delivery.
        let requests = self.requests.lock().map_err(|_| "request lock poisoned")?;
        let cache = self
            .cache
            .lock()
            .map_err(|_| "sample cache lock poisoned")?;
        let generations = self
            .generations
            .lock()
            .map_err(|_| "source generation lock poisoned")?;
        if requests[self.id] != self.request_id
            || generations[self.id] != (self.request_id, self.output_rate)
            || !same_source(cache[self.id].as_ref(), Some(&sample))
            || !self
                .ownership
                .content_lifetime_current(self.id, published_lifetime)
        {
            lease.rollback_unadopted_original();
            lease.rollback_unadopted_cache();
            return Err("adopted cold source superseded".into());
        }
        // Only callback ACK transfers sealed original/artifact ownership and project metadata.
        self.leases.lock().map_err(|_| "cold lease lock poisoned")?[self.id] = Some(lease);
        self.lease_generations[self.id].store(self.request_id, Ordering::Release);
        self.events
            .send(LoaderEvent::Success {
                timing_epoch: Some(enqueue_epoch),
                id: self.id,
                request_id: self.request_id,
                duration_s,
                detected_loop_start_s,
                cached_path,
                original_lease: Some(original_lease),
                analysis,
                analysis_error,
                incoming_defaults,
            })
            .map_err(|_| "native adoption metadata receiver closed")?;
        publication.adopted = true;
        Ok(())
    }
}

#[cfg(test)]
#[path = "cold_load_tests.rs"]
mod tests;
