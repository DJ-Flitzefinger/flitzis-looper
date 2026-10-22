use crate::audio_engine::audio_stream::{AudioStreamHandle, create_audio_stream, start_stream};
use crate::audio_engine::constants::{
    NUM_SAMPLES, PAD_EQ_DB_MAX, PAD_EQ_DB_MIN, PAD_GAIN_DB_MAX, PAD_GAIN_DB_MIN, SPEED_MAX,
    SPEED_MIN, VOLUME_MAX, VOLUME_MIN,
};
use crate::audio_engine::input_mapping::InputRuntime;
use crate::audio_engine::stem_cache::{project_stem_cache_dir, source_version_hash};
use crate::audio_engine::timing::{InputClock, validated_input_timestamp};
use crate::audio_engine::transport::QuantizeGrid;
use flitzis_looper_analysis as analysis;

use crate::messages::{
    AudioMessage, BackgroundTaskKind, ControlMessage, ControlParameterMessage, LoaderEvent,
    STEM_COMPONENT_MASK, SampleAnalysis, SampleBuffer, StemMixMode, TriggerQuantization,
    task_to_str,
};
use numpy::{PyArray1, ToPyArray};
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyInt};
use rtrb::Producer;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{
    Arc, Mutex,
    mpsc::{Receiver, Sender},
};

pub(crate) mod accepted_timing_refresh;
pub use accepted_timing_refresh::AcceptedTimingRefreshTicket;
mod analysis_jobs;
mod analysis_pcm;
mod analysis_predictions;
mod audio_stream;
pub use analysis_jobs::OfflineAnalysisJob;
mod buffer_retirement;
#[cfg(test)]
mod c3_observation;
#[cfg(all(test, windows))]
mod c3_process_metrics;
mod channels;
mod cold_jobs;
mod cold_load;
mod cold_residency;
#[cfg(all(test, windows))]
mod cold_residency_tests;
mod cold_store;
pub(crate) mod committed_stem_residency;
mod complete_context;
#[cfg(all(test, windows))]
mod native_lifecycle_observation;
pub use committed_stem_residency::{CommittedStemSetTicket, ResidentStemTicket};
pub(crate) use complete_context::NativeAnalysisOwner;
pub(crate) mod constant_timing;
mod constants;
mod content_lifetime;
pub(crate) mod content_placement;
pub use constant_timing::CapturedConstantTiming;
pub use constant_timing::ConstantTimingTicket;
pub use constant_timing::SavedConstantTimingTicket;
pub(crate) mod captured_global_start;
mod dsp;
mod errors;
pub(crate) mod global_playback_batch;
mod late_start_dsp;
pub use captured_global_start::CapturedGlobalStart;
pub(crate) mod hold_action;
pub use global_playback_batch::GlobalPlaybackBatchTicket;
mod incoming_load;
mod initial_loop_start;
mod input_mapping;
pub(crate) mod input_runtime_binding;
#[cfg(all(test, windows))]
mod ordinary_loader_incoming_tests;
mod ordinary_transport;
mod transport_intent_control;
pub use input_runtime_binding::InputRuntimePadBinding;
pub use ordinary_transport::{OrdinaryTransportAdmission, OrdinaryTransportObservation};
pub(crate) mod global_rate_status;
mod key_lock_attack_measurement;
mod key_lock_attack_reference;
#[cfg(all(test, windows))]
mod key_lock_authored_onset_tests;
mod key_lock_feedback;
#[cfg(all(test, windows))]
mod key_lock_native_envelope_tests;
#[cfg(all(test, windows))]
mod key_lock_physical_onset_tests;
pub(crate) mod key_lock_preparation;
#[cfg(all(test, windows))]
mod key_lock_recorded_source_tests;
#[cfg(test)]
mod key_lock_source_preparation;
pub(crate) mod key_lock_start_policy;
#[cfg(test)]
mod key_lock_start_policy_tests;
#[cfg(all(test, windows))]
mod key_lock_tonal_onset_tests;
mod loader_events;
mod loop_acceptance;
pub(crate) mod mix_capture;
mod mix_capture_api;
mod source_beatmap;
pub use source_beatmap::SourceBeatmap;
mod source_beatmap_artifact;
pub use source_beatmap_artifact::SourceBeatmapArtifact;
pub(crate) use source_beatmap_artifact::SourceBeatmapRawArtifact;
mod source_beatmap_capture;
mod source_beatmap_raw;
mod source_beatmap_raw_data;
#[cfg(all(test, windows))]
mod source_map_test_support;
pub use source_beatmap_raw::SourceBeatmapRawCandidate;
mod mixer;
pub use mix_capture_api::MixCaptureTicket;
pub use mix_capture_api::inspect_mix_capture_spool;
mod native_history_permit;
mod native_owned_work;
mod native_source_coverage;
mod offline_stem_source;
mod ordinary_endpoint_reserve;
pub(crate) mod ordinary_start;
pub(crate) mod pad_copy;
pub(crate) mod pad_pair;
pub(crate) mod pad_start;
mod pad_start_api;
pub(crate) mod pitch_actions;
mod pitch_api;
pub(crate) mod pitch_intent;
pub(crate) mod pitch_publication;
pub(crate) mod pitch_runtime;
pub use pad_start::CapturedPadStart;
#[cfg(test)]
mod audio_quality_metrics;
#[cfg(test)]
mod ordinary_endpoint_reserve_tests;
#[cfg(test)]
mod ordinary_global_start_tests;
#[cfg(test)]
mod ordinary_midi_start_tests;
#[cfg(test)]
mod ordinary_start_tests;
#[cfg(all(test, windows))]
mod pitch_reserve_resource_tests;
#[cfg(test)]
mod pitch_runtime_tests;
pub use hold_action::HoldActionTicket;
pub use input_mapping::CapturedInputPadTrigger;
pub use loader_events::LoaderEventDelivery;
pub use offline_stem_source::OfflineStemSourceLease;
pub use pad_copy::bank::{
    BankMutationAcknowledgement, BankMutationTicket, BankParticipants, CapturedBankMutation,
    RecoveredBankAcceptance,
};
pub use pad_copy::recovery::RecoveredCopyAcceptance;
pub use pad_copy::{CapturedPadCopy, PadCopyTicket};
pub use pad_pair::recovery::RecoveredPairAcceptance;
pub use pad_pair::{CapturedPadPair, PadPairAcknowledgement, PadPairTicket};
pub use pitch_actions::CapturedPitchAction;
mod prepared_native_history;
pub(crate) mod prepared_source;
mod prepared_source_feed;
mod prepared_stems_work;
mod productive_source_history;
mod progress;
mod project_assets;
mod resident_accounting;
mod resident_host_memory;
mod resident_proposal;
pub(crate) mod resident_relocation;
pub(crate) mod resident_seek;
pub use prepared_source::PreparedSourceTicket;
mod material_migration;
mod material_migration_control;
mod material_migration_journal;
mod material_migration_recovery;
pub use material_migration_recovery::{
    MigrationArtifactLease, MigrationInventoryLease, MigrationProjectGuard, portable_relocate_data,
    resolve_migration_config_reference,
};
mod material_migration_stems;
mod material_paths;
pub use material_migration_control::{
    MaterialMigrationHold, MaterialMigrationJournalStore, MaterialMigrationPreparation,
    MaterialMigrationSourceTicket, MaterialMigrationStemPreparation,
};
pub use material_paths::{resolve_project_asset, resolve_project_asset_details};
mod processing_files;
use prepared_source::next_epoch;
pub use processing_files::SealedProcessingFiles;
pub use project_assets::ProjectAssetLease;
pub(crate) mod rubberband_backend;
mod sample_loader;
mod scalar_grid;
pub(crate) use scalar_grid::ScalarSourceGrid;
#[cfg(test)]
mod l3_source_reader_tests;
mod mapped_source_clock;
mod scheduler;
mod source_control_epoch;
mod source_feed_history;
mod source_grid;
mod source_map_admission;
mod source_map_api;
mod source_playback;
mod source_reader;
mod source_sync_policy;
mod stem_cache;
mod stem_pair;
mod stem_pair_preparation;
pub use stem_pair_preparation::{InstrumentalStemReader, PreparedStemPair};
mod stem_pair_descriptor;
#[cfg(test)]
mod stem_pair_fault_tests;
#[cfg(all(test, windows))]
mod stem_pair_tests;
pub(crate) mod stretch_processor;
mod timing;
mod transport;
mod voice_slot;
mod waveform;

/// Tuple: (is_raw_mode, xs, y_min, y_max)
///
/// - `is_raw_mode` (bool): If True, draw a simple line using `xs` and `y_min`.
/// - `xs`: Time values (seconds).
/// - `y_min`: Min values (or raw samples if in raw mode).
/// - `y_max`: Max values (or None if in raw mode).
type WaveformResult = PyResult<
    Option<(
        bool,
        Py<PyArray1<f64>>,
        Py<PyArray1<f32>>,
        Option<Py<PyArray1<f32>>>,
    )>,
>;

fn parse_trigger_quantization(mode: &str) -> Option<TriggerQuantization> {
    let normalized_owned = mode.trim().to_ascii_lowercase().replace(['-', '/'], "_");
    let normalized = normalized_owned
        .strip_prefix("grid_")
        .unwrap_or(normalized_owned.as_str());

    let step_64ths = match normalized {
        "1_64" => Some(1),
        "1_32" => Some(2),
        "1_16" => Some(4),
        "next_beat" | "beat" | "next_bar" | "bar" => Some(4),
        _ => None,
    };

    match normalized {
        "immediate" | "disabled" | "off" => Some(TriggerQuantization::Immediate),
        _ => step_64ths.map(|step_64ths| TriggerQuantization::Grid { step_64ths }),
    }
}

fn parse_stem_mix_mode(mode: &str) -> Option<StemMixMode> {
    match mode {
        "full_mix" | "full-mix" | "fullmix" => Some(StemMixMode::FullMix),
        "all_stems" | "all-stems" | "stems" => Some(StemMixMode::AllStems),
        _ => None,
    }
}

/// Reserve capacity before publishing a global timing edit's revocation fence.
fn push_global_timing_message<T>(
    revision: &AtomicU64,
    publishing: &AtomicBool,
    producer: &mut Producer<T>,
    message: T,
    name: &str,
) -> PyResult<()> {
    if producer.is_full() {
        return Err(PyRuntimeError::new_err(format!(
            "Failed to send {name} - buffer may be full"
        )));
    }
    let next = next_epoch(revision).map_err(PyRuntimeError::new_err)?;
    publishing.store(true, Ordering::Release);
    revision.store(next, Ordering::Release);
    let result = producer
        .push(message)
        .map_err(|_| PyRuntimeError::new_err("reserved single-producer capacity lost"));
    publishing.store(false, Ordering::Release);
    result
}

/// Keep the originally queued rate identity distinct from later global revocations.
fn push_global_rate_message(
    revision: &AtomicU64,
    publishing: &AtomicBool,
    status: &global_rate_status::GlobalRateStatusSlot,
    producer: &mut Producer<ControlParameterMessage>,
    speed: Option<f64>,
    period_seconds: Option<f64>,
    name: &str,
) -> PyResult<()> {
    if producer.is_full() {
        return Err(PyRuntimeError::new_err(format!(
            "Failed to send {name} - buffer may be full"
        )));
    }
    let next = next_epoch(revision).map_err(PyRuntimeError::new_err)?;
    publishing.store(true, Ordering::Release);
    revision.store(next, Ordering::Release);
    status.publish_requested(next, speed, period_seconds);
    let result = producer
        .push(ControlParameterMessage::SetRateIntent {
            revision: next,
            speed,
            period_seconds,
        })
        .map_err(|_| PyRuntimeError::new_err("reserved single-producer rate capacity lost"));
    publishing.store(false, Ordering::Release);
    result
}

fn push_control_message(
    producer: &mut Producer<ControlMessage>,
    message: ControlMessage,
    label: &str,
) -> PyResult<()> {
    producer.push(message).map_err(|_| {
        PyRuntimeError::new_err(format!("Failed to send {label} - buffer may be full"))
    })
}

fn push_parameter_message(
    producer: &mut Producer<ControlParameterMessage>,
    message: ControlParameterMessage,
    label: &str,
) -> PyResult<()> {
    producer.push(message).map_err(|_| {
        PyRuntimeError::new_err(format!("Failed to send {label} - buffer may be full"))
    })
}

struct PadTaskGuard {
    id: usize,
    task: BackgroundTaskKind,
    active_tasks: Arc<Mutex<HashSet<(usize, BackgroundTaskKind)>>>,
    task_token: Arc<()>,
    task_tokens: Arc<Mutex<HashMap<(usize, BackgroundTaskKind), Arc<()>>>>,
}

impl Drop for PadTaskGuard {
    fn drop(&mut self) {
        if let Ok(mut set) = self.active_tasks.lock()
            && let Ok(mut tokens) = self.task_tokens.lock()
            && tokens
                .get(&(self.id, self.task))
                .is_some_and(|token| Arc::ptr_eq(token, &self.task_token))
        {
            tokens.remove(&(self.id, self.task));
            set.remove(&(self.id, self.task));
        }
    }
}

fn has_active_task_for_id(tasks: &HashSet<(usize, BackgroundTaskKind)>, id: usize) -> bool {
    tasks.iter().any(|(task_id, _)| *task_id == id)
}

/// Checked ownership transition prepared and committed while holding the request mutex.
/// Both counters are validated before any publication or identity mutation.
struct PadRequestAdvance<'a> {
    current: &'a mut u64,
    epoch: &'a AtomicU64,
    next_request: u64,
    next_epoch: u64,
}

impl<'a> PadRequestAdvance<'a> {
    fn prepare(current: &'a mut u64, epoch: &'a AtomicU64) -> Result<Self, String> {
        let next_request = current.checked_add(1).ok_or("pad request id exhausted")?;
        let next_epoch = next_epoch(epoch)?;
        Ok(Self {
            current,
            epoch,
            next_request,
            next_epoch,
        })
    }

    fn commit(self) -> u64 {
        // Callback freshness must be invalidated before the new request/command is visible.
        self.epoch.store(self.next_epoch, Ordering::Release);
        *self.current = self.next_request;
        self.next_request
    }

    fn commit_reserved_prechecked(self, target: u64) {
        self.epoch.store(self.next_epoch, Ordering::Release);
        *self.current = target;
    }
}

fn next_pad_request_id(
    pad_request_ids: &Arc<Mutex<Vec<u64>>>,
    id: usize,
    epoch: &AtomicU64,
) -> Result<u64, String> {
    let mut guard = pad_request_ids
        .lock()
        .map_err(|_| "Failed to acquire pad request id lock".to_string())?;
    let Some(current) = guard.get_mut(id) else {
        return Err("id out of range".to_string());
    };
    Ok(PadRequestAdvance::prepare(current, epoch)?.commit())
}

fn current_pad_request_id(
    pad_request_ids: &Arc<Mutex<Vec<u64>>>,
    id: usize,
) -> Result<u64, String> {
    let guard = pad_request_ids
        .lock()
        .map_err(|_| "Failed to acquire pad request id lock".to_string())?;
    guard
        .get(id)
        .copied()
        .ok_or_else(|| "id out of range".to_string())
}

fn pad_request_matches(pad_request_ids: &Arc<Mutex<Vec<u64>>>, id: usize, request_id: u64) -> bool {
    current_pad_request_id(pad_request_ids, id).is_ok_and(|current| current == request_id)
}

/// Normal analysis admission owns the same actual source/request boundary as timing work.
fn admit_sample_analysis(
    engine: &AudioEngine,
    id: usize,
) -> PyResult<(complete_context::CompleteSourceReader, u64, u32)> {
    let mut requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let loading = engine
        .loading_sample_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("loading lock poisoned"))?;
    if loading.contains(&id) {
        return Err(PyValueError::new_err("sample is currently loading"));
    }
    let mut tasks = engine
        .active_tasks
        .lock()
        .map_err(|_| PyRuntimeError::new_err("task lock poisoned"))?;
    let task_cell = engine
        .active_task_cell(id)
        .map_err(PyRuntimeError::new_err)?;
    if has_active_task_for_id(&tasks, task_cell) {
        return Err(PyValueError::new_err("sample task already running"));
    }
    let cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    let sample = cache[id]
        .clone()
        .ok_or_else(|| PyValueError::new_err("sample is not loaded"))?;
    let mut reader = complete_context::CompleteSourceReader::capture(engine, id, sample)
        .map_err(PyValueError::new_err)?;
    let generations = engine
        .loaded_source_generations
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source generation lock poisoned"))?;
    let (generation, rate) = generations[id];
    if generation == 0 || rate == 0 {
        return Err(PyValueError::new_err("loaded source identity unavailable"));
    }
    admit_complete_analysis(&reader, rate).map_err(PyValueError::new_err)?;
    let source_guard = reader
        .guard
        .clone()
        .ok_or_else(|| PyRuntimeError::new_err("analysis Native source owner unavailable"))?;
    let prepared_epoch = engine
        .prepared_source_epoch(id)
        .map_err(PyRuntimeError::new_err)?
        .clone();
    let task_token = Arc::new(());
    let mut task_tokens = engine
        .analysis_task_tokens
        .lock()
        .map_err(|_| PyRuntimeError::new_err("analysis task owner lock poisoned"))?;
    let advance = PadRequestAdvance::prepare(
        &mut requests[id],
        engine
            .prepared_source_epoch(id)
            .map_err(PyRuntimeError::new_err)?,
    )
    .map_err(PyRuntimeError::new_err)?;
    let logical_advance = engine
        .input_runtime_ownership
        .prepare_source_request_advance(id)
        .map_err(PyRuntimeError::new_err)?;
    logical_advance.commit();
    let request_id = advance.commit();
    reader.analysis_owner = Some(NativeAnalysisOwner::new(source_guard, prepared_epoch));
    task_tokens.insert((task_cell, BackgroundTaskKind::Analysis), task_token);
    tasks.insert((task_cell, BackgroundTaskKind::Analysis));
    Ok((reader, request_id, rate))
}

fn admit_complete_analysis(
    reader: &complete_context::CompleteSourceReader,
    rate: u32,
) -> Result<(), String> {
    if rate == 0 {
        return Err("analysis source rate is unavailable".into());
    }
    let sample = &reader.reference;
    if sample
        .residency
        .as_ref()
        .is_some_and(|view| view.source.sample_rate_hz != rate)
    {
        return Err("complete analysis rate differs from immutable source".into());
    }
    // Same executed conversion/analyzer admission as cold-load analysis, plus
    // the separately retained finite reference during complete materialization.
    let finite = sample.resident_start() != 0 || sample.resident_end() != sample.frame_count();
    let held = if finite { reader.held_bytes()? } else { 0 };
    admit_analysis_geometry(sample.frame_count(), sample.channels, rate, held)?;
    Ok(())
}

fn admit_analysis_geometry(
    frames: usize,
    channels: usize,
    rate: u32,
    held: usize,
) -> Result<usize, String> {
    analysis_pcm::default_analysis_peak(
        frames,
        channels,
        rate,
        held,
        constant_timing::MAX_PCM_BYTES,
    )
    .map_err(|error| error.to_string())
}

fn resident_seek_available(sample: &SampleBuffer, position_s: f64, rate: u32) -> bool {
    if sample.resident_start() == 0 && sample.resident_end() == sample.frame_count() {
        return true;
    }
    // Match the callback's existing source-second rounding and complete-extent clamp.
    let frame = (position_s * f64::from(rate))
        .round()
        .clamp(0.0, sample.frame_count() as f64) as usize;
    frame >= sample.resident_start() && frame < sample.resident_end()
}

pub(crate) use cold_load::{SourceMixInitialization, SourcePitchInitialization};

struct LoadedSourcePublication<'a> {
    ownership: &'a Arc<input_runtime_binding::InputRuntimeOwnership>,
    placement: content_placement::SlotSnapshot,
    generation: u64,
    rate: u32,
    generation_slot: &'a mut (u64, u32),
    digest_slot: &'a mut Option<String>,
    digest: String,
    cold: bool,
    cold_epoch: Option<Arc<AtomicU64>>,
    cold_adoption: Option<Arc<std::sync::atomic::AtomicU8>>,
    replace_assignment: bool,
    loop_region: Option<(usize, usize)>,
    resident_cancelled: Option<Arc<AtomicBool>>,
    incoming_defaults: Option<crate::messages::IncomingLoadDefaults>,
    initial_pitch: cold_load::SourcePitchInitialization,
    initial_mix: Option<cold_load::SourceMixInitialization>,
    intent: Option<(
        &'a mut analysis::tempo_acceptance::TimingIntent,
        analysis::tempo_acceptance::TimingIntent,
    )>,
}

#[cfg(test)]
fn publish_loaded_sample(
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    sample_cache: &Arc<Mutex<Vec<Option<SampleBuffer>>>>,
    id: usize,
    sample: SampleBuffer,
    publication: LoadedSourcePublication<'_>,
) -> Result<(), String> {
    publish_loaded_sample_with_arm(producer, sample_cache, id, sample, publication, || Ok(()))
        .map(|_| ())
}

#[cfg(test)]
fn publish_loaded_sample_with_arm(
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    sample_cache: &Arc<Mutex<Vec<Option<SampleBuffer>>>>,
    id: usize,
    sample: SampleBuffer,
    publication: LoadedSourcePublication<'_>,
    arm: impl FnOnce() -> Result<(), String>,
) -> Result<input_runtime_binding::SourcePublication, String> {
    try_publish_loaded_sample_with_arm(producer, sample_cache, id, sample, publication, arm)
        .map_err(|error| error.to_string())
}

fn try_publish_loaded_sample_with_arm(
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    sample_cache: &Arc<Mutex<Vec<Option<SampleBuffer>>>>,
    id: usize,
    sample: SampleBuffer,
    publication: LoadedSourcePublication<'_>,
    arm: impl FnOnce() -> Result<(), String>,
) -> Result<input_runtime_binding::SourcePublication, input_runtime_binding::SourcePublicationError>
{
    let mut cache = sample_cache
        .lock()
        .map_err(|_| "sample cache lock poisoned")?;
    let slot = cache.get_mut(id).ok_or("id out of range")?;
    let mut producer_guard = producer
        .lock()
        .map_err(|_| "Failed to acquire producer lock".to_string())?;

    if producer_guard.is_full() {
        return Err("Failed to send LoadSample - buffer may be full".into());
    }
    publication
        .ownership
        .ensure_no_copy_reservation(id)
        .map_err(|error| error.to_string())?;
    let authority = next_epoch(
        publication
            .ownership
            .authority_for_cell(publication.placement.cell()),
    )?;
    let content_lifetime = content_lifetime::for_assignment(
        publication.placement.lifetime(),
        publication.replace_assignment
            || slot.is_none()
            || publication.digest_slot.as_ref() != Some(&publication.digest),
    )?;
    // Preparatory proposals cannot invalidate the old usable source before all
    // resource/queue/scalar admission succeeds. The caller owns its request gate.
    let write = publication
        .ownership
        .prepare_slot_write_fixed(publication.placement, content_lifetime)?;
    // Capacity is reserved under the only producer lock. Publish the source fence
    // before releasing the command so a fast callback never sees an old fence.
    let intent = publication.intent.as_ref().map(|(_, intent)| *intent);
    let mut arm_error = None;
    let published = publication
        .ownership
        .publish_source_write_fixed(
            &write,
            &sample,
            publication.rate,
            publication.generation,
            authority,
            intent,
            publication.cold,
            || match arm() {
                Ok(()) => true,
                Err(error) => {
                    arm_error = Some(error);
                    false
                }
            },
        )
        .map_err(|error| {
            arm_error.map_or(
                error,
                input_runtime_binding::SourcePublicationError::Rejected,
            )
        })?;
    if let Some((slot, intent)) = publication.intent {
        *slot = intent;
    }
    *publication.generation_slot = (publication.generation, publication.rate);
    *publication.digest_slot = Some(publication.digest);
    *slot = Some(sample.clone());

    let message = if publication.cold {
        let epoch = publication.cold_epoch;
        let captured_epoch = epoch
            .as_ref()
            .map_or(0, |epoch| epoch.load(Ordering::Acquire));
        ControlMessage::LoadColdSample {
            publication: published.clone(),
            id,
            sample,
            source_generation: publication.generation,
            content_lifetime,
            adoption: publication
                .cold_adoption
                .expect("bounded cold adoption token"),
            epoch,
            captured_epoch,
            replace_assignment: publication.replace_assignment,
            loop_region: publication.loop_region,
            resident_cancelled: publication.resident_cancelled,
            incoming_defaults: publication.incoming_defaults,
            initial_pitch: publication.initial_pitch,
            initial_mix: publication.initial_mix,
        }
    } else {
        ControlMessage::LoadSample {
            id,
            sample,
            publication: Some(published.clone()),
        }
    };
    producer_guard
        .push(message)
        .expect("reserved single-producer capacity");

    Ok(published)
}

/// Resample mono f32 audio to a target sample rate using rubato.
///
/// Returns the original buffer unchanged if `src_rate` already equals `target_rate`.
#[cfg(test)]
fn resample_mono_to_target(
    mono: Vec<f32>,
    src_rate: u32,
    target_rate: u32,
) -> Result<Vec<f32>, String> {
    analysis_pcm::resample_mono_cancellable(mono, src_rate, target_rate, usize::MAX, &|| false)
        .map_err(|error| error.to_string())
}
/// Run the BPM detection pipeline on mono audio.
///
/// Returns `(bpm, beat_grid)` on success.
fn run_bpm_pipeline(
    mono_f64: &[f64],
    sample_rate_hz: u32,
) -> Result<(f32, analysis::BeatGrid), String> {
    analysis::analyze_bpm(
        mono_f64,
        sample_rate_hz,
        &analysis::AnalysisConfig::default(),
    )
}

/// Run the key detection pipeline on the shared widened mono at 44100 Hz.
///
/// Returns the key string (e.g., "Am", "C", or "unknown" on failure).
fn run_key_detection(mono_44100: &[f64]) -> String {
    #[cfg(debug_assertions)]
    let _timer = std::time::Instant::now();

    match analysis::detect_key_f64(mono_44100, 44_100) {
        Ok(result) => {
            #[cfg(debug_assertions)]
            eprintln!(
                "[analysis] key detection: {} ({:.0}%) in {:?}",
                result.key_name,
                result.confidence * 100.0,
                _timer.elapsed()
            );
            result.key_name
        }
        Err(analysis::KeyError::InsufficientData) => "unknown".to_string(),
        Err(analysis::KeyError::SilentAudio) => "unknown".to_string(),
        Err(analysis::KeyError::ModelError(_msg)) => {
            #[cfg(debug_assertions)]
            eprintln!("[analysis] key detection model error: {_msg}");
            "unknown".to_string()
        }
        Err(analysis::KeyError::CqtError(_msg)) => {
            #[cfg(debug_assertions)]
            eprintln!("[analysis] key detection CQT error: {_msg}");
            "unknown".to_string()
        }
    }
}

/// Analyze a sample buffer for BPM, beat grid, and key.
///
/// Shared preprocessing (borrowed PCM -> mono -> resample to 44100) runs once, then
/// the BPM pipeline and key detection pipeline execute concurrently via
/// `std::thread::scope`. Total analysis time is bounded by the slower pipeline.
fn analyze_sample(sample: &SampleBuffer, sample_rate_hz: u32) -> Result<SampleAnalysis, String> {
    if sample.samples.is_empty() || sample_rate_hz == 0 {
        return Err("analysis failed: empty sample or zero sample rate".to_string());
    }

    let mono_f64 = analysis_pcm::prepare_default_mono_f64(sample, sample_rate_hz)
        .map_err(|error| format!("analysis failed: {error}"))?;

    // Measure total analysis time in debug builds.
    #[cfg(debug_assertions)]
    let start = std::time::Instant::now();

    // Launch BPM and key detection pipelines concurrently.
    let (bpm_result, key_result) = {
        let mut bpm_result = None;
        let mut key_result = None;

        std::thread::scope(|s| {
            // BPM pipeline thread.
            s.spawn(|| {
                bpm_result = Some(run_bpm_pipeline(&mono_f64, 44_100));
            });

            // Key detection thread.
            s.spawn(|| {
                key_result = Some(run_key_detection(&mono_f64));
            });
        });

        // Both threads have joined at this point.
        (bpm_result.unwrap(), key_result.unwrap())
    };

    #[cfg(debug_assertions)]
    eprintln!("[analysis] total analysis time: {:?}", start.elapsed());

    // Assemble result. Key detection failures already return "unknown".
    let (bpm, beat_grid) = bpm_result?;

    Ok(SampleAnalysis {
        bpm,
        key: key_result,
        beat_grid,
    })
}

/// Reject malformed Python values before they cross the bounded command boundary.
fn parse_input_timestamp(received_at_ns: Option<&Bound<'_, PyAny>>) -> PyResult<Option<u64>> {
    let Some(value) = received_at_ns else {
        return Ok(None);
    };
    if value.is_instance_of::<PyBool>() || !value.is_instance_of::<PyInt>() {
        return Err(PyTypeError::new_err(
            "received_at_ns must be an integer or None",
        ));
    }
    value
        .extract::<u64>()
        .map(Some)
        .map_err(|_| PyValueError::new_err("received_at_ns must be in 0..=18446744073709551615"))
}

fn parse_constant_timing_pcm_limit(value: &Bound<'_, PyAny>) -> PyResult<usize> {
    if value.is_instance_of::<PyBool>() || !value.is_instance_of::<PyInt>() {
        return Err(PyTypeError::new_err("pcm_limit_bytes must be an integer"));
    }
    let limit_bytes = value.extract::<usize>().map_err(|_| {
        PyValueError::new_err("constant timing PCM limit must be in 1..=1073741824 bytes")
    })?;
    constant_timing::validated_pcm_limit_bytes(limit_bytes).map_err(PyValueError::new_err)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamQuiescence {
    Open,
    Verified,
    Uncertain,
}

/// Receipt for the last installed stream, sharing its actual callback failure bit.
/// Absence of a stream handle alone never establishes verified quiescence.
struct StreamLifecycle {
    failed: Arc<AtomicBool>,
    quiescence: StreamQuiescence,
}

/// AudioEngine provides audio output and non-realtime control through CPAL.
#[pyclass]
pub struct AudioEngine {
    stream_handle: Option<AudioStreamHandle>,
    stream_lifecycle: Option<StreamLifecycle>,
    mix_capture_owner: Mutex<Option<mix_capture_api::EngineCapture>>,
    #[cfg(test)]
    test_control_channels: Option<(
        Arc<Mutex<Producer<ControlMessage>>>,
        Arc<Mutex<rtrb::Consumer<AudioMessage>>>,
    )>,
    #[cfg(test)]
    test_output_format: Option<(u32, usize)>,
    #[cfg(test)]
    test_parameter_producer: Option<Arc<Mutex<Producer<ControlParameterMessage>>>>,
    is_playing: bool,
    loader_tx: Sender<LoaderEvent>,
    loader_rx: Mutex<Receiver<LoaderEvent>>,
    pending_loader_event: Mutex<Option<LoaderEvent>>,
    sample_cache: Arc<Mutex<Vec<Option<SampleBuffer>>>>,
    loading_sample_ids: Arc<Mutex<HashSet<usize>>>,
    active_tasks: Arc<Mutex<HashSet<(usize, BackgroundTaskKind)>>>,
    analysis_task_tokens: Arc<Mutex<HashMap<(usize, BackgroundTaskKind), Arc<()>>>>,
    pad_request_ids: Arc<Mutex<Vec<u64>>>,
    loaded_source_generations: Arc<Mutex<Vec<(u64, u32)>>>,
    loaded_source_digests: Arc<Mutex<Vec<Option<String>>>>,
    prepared_source_epochs: Vec<Arc<AtomicU64>>,
    pending_prepared_stems: Mutex<Vec<Option<Box<prepared_stems_work::PendingPreparedStems>>>>,
    timing_intents: Arc<Mutex<Vec<analysis::tempo_acceptance::TimingIntent>>>,
    current_timing_acknowledgements: Arc<constant_timing::CurrentTimingAcknowledgements>,
    loop_acceptance: Arc<loop_acceptance::SharedLoopAcceptance>,
    current_constant_timing: Mutex<Vec<Vec<constant_timing::CurrentConstantTimingRecord>>>,
    input_runtime_ownership: Arc<input_runtime_binding::InputRuntimeOwnership>,
    pitch_work_capacity: pitch_publication::PitchWorkCapacity,
    global_timing_revision: Arc<AtomicU64>,
    constant_timing_busy: Arc<AtomicBool>,
    offline_jobs: analysis_jobs::OfflineJobs,
    cold_jobs: cold_jobs::ColdJobs,
    waveform_requests: waveform::WaveformRequests,
    cold_cancelled: Arc<AtomicBool>,
    cold_loading: Arc<Vec<AtomicU64>>,
    cold_leases: Arc<Mutex<Vec<Option<cold_store::CommittedColdLease>>>>,
    resident_stem_cache: Arc<Mutex<Vec<resident_relocation::ResidentStemCache>>>,
    resident_restore_guards: Mutex<Vec<Option<Arc<cold_residency::ResidentLoadGuard>>>>,
    incoming_loads: Mutex<Vec<Option<Arc<incoming_load::IncomingLoad>>>>,
    pad_copies: Mutex<Vec<Option<Arc<pad_copy::PadCopyState>>>>,
    bank_mutations: Mutex<Vec<Arc<pad_copy::bank::BankMutationState>>>,
    cold_lease_generations: Arc<Vec<AtomicU64>>,
    cold_pcm_history: Arc<project_assets::PcmHistory>,
    project_assets: Arc<project_assets::ProjectAssets>,
    project_asset_engine_owner: Arc<()>,
    resident_account: Arc<resident_accounting::ResidentAccount>,
    resident_proposals: Arc<resident_proposal::ResidentProposals>,
    input_runtime: Option<InputRuntime>,
    input_clock: InputClock,
}

impl AudioEngine {
    fn stream_quiescence_error() -> PyErr {
        PyRuntimeError::new_err(
            "audio callback quiescence is unverified; original ownership retained",
        )
    }

    /// Shared control transaction; the productive caller closes its actual stream.
    /// Tests supply the actual hardware-free callback owner and the same join barrier.
    fn close_audio_stream_with(
        &mut self,
        close: impl FnOnce(&mut Self) -> std::io::Result<()>,
    ) -> PyResult<()> {
        match self.stream_lifecycle.as_ref().map(|state| state.quiescence) {
            Some(StreamQuiescence::Uncertain) => return Err(Self::stream_quiescence_error()),
            Some(StreamQuiescence::Open) => {
                // Set before transferring the owner: neither a failed join nor an unwind
                // may turn a missing handle into permission for orphan takeover/restart.
                self.stream_lifecycle.as_mut().unwrap().quiescence = StreamQuiescence::Uncertain;
                self.is_playing = false;
                close(self).map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
                self.stream_lifecycle.as_mut().unwrap().quiescence = StreamQuiescence::Verified;
            }
            Some(StreamQuiescence::Verified) | None => {}
        }
        self.poll_mix_capture_shutdown_checked()
    }

    fn close_audio_stream_checked(&mut self) -> PyResult<()> {
        self.close_audio_stream_with(|engine| {
            let stream = engine.stream_handle.take().ok_or_else(|| {
                std::io::Error::other(
                    "original audio stream owner is unavailable for verified close",
                )
            })?;
            stream.close_checked()
        })
    }

    fn discard_uninstalled_stream(&mut self, stream: AudioStreamHandle, error: PyErr) -> PyErr {
        let failure = stream.stream_failed.clone();
        match stream.close_checked() {
            Ok(()) => error,
            Err(close_error) => {
                // A failed explicit run must not erase the previous stream's sticky
                // error. Even a first startup failure must fence a later restart if
                // its newly built callback image could not be verified closed.
                self.note_unverified_stream_close(failure);
                PyRuntimeError::new_err(format!("{error}; {close_error}"))
            }
        }
    }

    fn note_unverified_stream_close(&mut self, failure: Arc<AtomicBool>) {
        self.stream_lifecycle
            .get_or_insert(StreamLifecycle {
                failed: failure,
                quiescence: StreamQuiescence::Uncertain,
            })
            .quiescence = StreamQuiescence::Uncertain;
    }

    fn note_stream_startup_error(&mut self, error: &(dyn std::error::Error + 'static)) {
        if let Some(error) = error.downcast_ref::<audio_stream::UncertainStreamStartup>() {
            self.note_unverified_stream_close(error.failure_signal());
        }
    }

    fn quiesce_failed_stream_with(
        &mut self,
        close: impl FnOnce(&mut Self) -> std::io::Result<()>,
    ) -> PyResult<bool> {
        if self
            .stream_lifecycle
            .as_ref()
            .is_some_and(|state| state.quiescence == StreamQuiescence::Uncertain)
        {
            return Err(Self::stream_quiescence_error());
        }
        if !self.audio_stream_failed() {
            return Ok(false);
        }
        // Establish the actual stream failure before any ordinary cancellation.
        // The capture's first reason and already closed successful writer survive.
        self.poll_mix_capture_stream_failure()?;
        if let Some(runtime) = self.input_runtime.as_ref() {
            runtime.set_enabled(false);
        }
        self.close_audio_stream_with(close)?;
        Ok(true)
    }

    /// Existing Rust callers share the optional original-participant Python kernel.
    pub fn capture_bank_mutation(
        &self,
        source_bank: Option<usize>,
        target_bank: usize,
        intents: Vec<Option<String>>,
    ) -> PyResult<CapturedBankMutation> {
        pad_copy::bank::capture(self, source_bank, target_bank, intents)
    }

    /// Existing Rust callers share the optional-binding capture kernel with Python.
    pub fn _capture_pad_pair(
        &self,
        source_id: usize,
        target_id: usize,
    ) -> PyResult<CapturedPadPair> {
        pad_pair::control::capture(self, source_id, target_id)
    }

    /// Native background tasks occupy stable cells, independently of physical request ids.
    fn active_task_cell(&self, id: usize) -> Result<usize, String> {
        self.input_runtime_ownership
            .slot_snapshot(id)
            .map(|snapshot| snapshot.cell().index())
            .ok_or_else(|| "background task placement is pending".into())
    }
    pub(super) fn prepared_source_epoch(&self, id: usize) -> Result<&Arc<AtomicU64>, String> {
        let snapshot = self
            .input_runtime_ownership
            .slot_snapshot(id)
            .ok_or("content placement is pending or unavailable")?;
        self.prepared_source_epochs
            .get(snapshot.cell().index())
            .ok_or_else(|| "content epoch cell is unavailable".into())
    }

    pub fn play_sample(
        &mut self,
        id: usize,
        volume: f32,
        received_at_ns: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        self.play_sample_with_source(id, volume, received_at_ns, None)
    }

    pub fn play_sample_exclusive(
        &mut self,
        id: usize,
        volume: f32,
        received_at_ns: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        self.play_sample_exclusive_with_source(id, volume, received_at_ns, None)
    }

    pub fn seek_sample(&mut self, id: usize, position_s: f64) -> PyResult<()> {
        self.seek_sample_with_source(id, position_s, None)
    }

    pub fn prepare_resident_control(
        &self,
        id: usize,
        start_s: Option<f64>,
        end_s: Option<f64>,
        position_s: Option<f64>,
        key_lock: Option<bool>,
    ) -> PyResult<resident_relocation::ResidentWindowTicket> {
        self.prepare_resident_control_with_source(id, start_s, end_s, position_s, key_lock, None)
    }

    pub fn set_pad_timing_intent(&self, sample_id: usize, intent: &str) -> PyResult<()> {
        self.set_pad_timing_intent_with_source(sample_id, intent, None)
    }

    pub fn set_pad_key_lock(&mut self, id: usize, enabled: bool) -> PyResult<u64> {
        self.set_pad_key_lock_with_source(id, enabled, None)
    }

    pub fn set_pad_bpm(&mut self, id: usize, bpm: Option<f64>) -> PyResult<()> {
        self.set_pad_bpm_with_source(id, bpm, None)
    }

    pub fn set_pad_timing_metadata(&mut self, id: usize, phase_anchor_s: f64) -> PyResult<()> {
        self.set_pad_timing_metadata_with_source(id, phase_anchor_s, None)
    }

    pub fn set_pad_loop_region(
        &mut self,
        id: usize,
        start_s: f64,
        end_s: Option<f64>,
    ) -> PyResult<()> {
        self.set_pad_loop_region_with_source(id, start_s, end_s, None)
    }

    /// The public API and headless controller bridges share this admission path.
    fn publish_pad_loop_region_with_source(
        &self,
        id: usize,
        start_s: f64,
        end_s: Option<f64>,
        expected_source_generation: Option<u64>,
        queue: Option<(u32, &Arc<Mutex<Producer<ControlMessage>>>)>,
    ) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        if !start_s.is_finite() || start_s < 0.0 {
            return Err(PyValueError::new_err("start_s out of range"));
        }
        if end_s.is_some_and(|end| !end.is_finite() || end < 0.0) {
            return Err(PyValueError::new_err("end_s out of range"));
        }
        resident_relocation::reconcile(self)?;
        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        input_runtime_binding::validate_expected_source_generation_under_request_lock(
            self,
            id,
            expected_source_generation,
        )?;
        let output_sample_rate = match queue {
            Some((rate, _)) => rate,
            None => self.output_sample_rate()?,
        };
        if let Some(sample) = self
            .sample_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?[id]
            .as_ref()
        {
            let start = (start_s * f64::from(output_sample_rate)).round() as usize;
            let end = end_s.map(|end| (end * f64::from(output_sample_rate)).round() as usize);
            if let Some(region) =
                source_reader::effective_loop_region(start, end, sample.frame_count())
                && (region.start < sample.resident_start() || region.end > sample.resident_end())
            {
                return Err(PyValueError::new_err(
                    "loop requires a matching prepared resident window",
                ));
            }
        }
        let producer = match queue {
            Some((_, producer)) => producer,
            None => self.control_producer()?,
        };
        let mut producer = producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;
        if producer.is_full() {
            return Err(PyRuntimeError::new_err(
                "Failed to send SetPadLoopRegion - buffer may be full",
            ));
        }
        let input_authority = self.input_runtime_ownership.next_authority(id)?;
        self.note_resident_loop_intent(id, start_s, end_s)?;
        self.input_runtime_ownership.revoke(id, input_authority);
        producer
            .push(ControlMessage::SetPadLoopRegion {
                id,
                start_s,
                end_s,
                expected_source_generation,
            })
            .map_err(|_| PyRuntimeError::new_err("reserved single-producer capacity lost"))
    }

    fn parameter_producer(&self) -> PyResult<&Arc<Mutex<Producer<ControlParameterMessage>>>> {
        if let Some(handle) = &self.stream_handle {
            return Ok(&handle.parameter_producer);
        }
        #[cfg(test)]
        if let Some(producer) = &self.test_parameter_producer {
            return Ok(producer);
        }
        Err(PyRuntimeError::new_err("Audio engine not initialized"))
    }

    fn control_producer(&self) -> PyResult<&Arc<Mutex<Producer<ControlMessage>>>> {
        if let Some(handle) = &self.stream_handle {
            return Ok(&handle.producer);
        }
        #[cfg(test)]
        if let Some((producer, _)) = &self.test_control_channels {
            return Ok(producer);
        }
        Err(PyRuntimeError::new_err("Audio engine not initialized"))
    }

    fn audio_message_consumer(&self) -> PyResult<&Arc<Mutex<rtrb::Consumer<AudioMessage>>>> {
        if let Some(handle) = &self.stream_handle {
            return Ok(&handle.consumer);
        }
        #[cfg(test)]
        if let Some((_, consumer)) = &self.test_control_channels {
            return Ok(consumer);
        }
        Err(PyRuntimeError::new_err("Audio engine not initialized"))
    }

    /// Prepare, lease and enqueue a current stem set through the actual control producer.
    /// Stream startup supplies the producer; hardware-free probes use the same kernel.
    #[cfg(test)]
    pub(super) fn publish_prepared_stems_with_producer(
        &self,
        py: Python<'_>,
        id: usize,
        source_version: String,
        cache_dir: String,
        source_ticket: &PreparedSourceTicket,
        producer: &Arc<Mutex<Producer<ControlMessage>>>,
    ) -> PyResult<()> {
        if prepared_stems_work::has_pending(self, source_ticket)? {
            return prepared_stems_work::reconcile(self);
        }
        let work = prepared_stems_work::PreparedStemsWork::capture(
            self,
            id,
            source_version,
            cache_dir,
            source_ticket,
        )?;
        let prepared = py
            .detach(move || work.prepare())
            .map_err(PyValueError::new_err)?;
        prepared.finish(self, source_ticket, producer)
    }

    fn note_resident_loop_intent(&self, id: usize, start: f64, end: Option<f64>) -> PyResult<()> {
        if let Some(guard) = self
            .resident_restore_guards
            .lock()
            .map_err(|_| PyRuntimeError::new_err("resident intent lock poisoned"))?[id]
            .as_ref()
            && self.cold_loading[id].load(Ordering::Acquire) == guard.request
        {
            guard.loop_intent(start, end);
        }
        Ok(())
    }

    fn admit_resident_key_lock(&self, id: usize, enabled: bool) -> PyResult<()> {
        if enabled
            && self
                .sample_cache
                .lock()
                .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?[id]
                .as_ref()
                .is_some_and(|sample| !mixer::RtMixer::initial_key_lock_geometry_available(sample))
        {
            return Err(PyValueError::new_err(
                "Key Lock requires an admitted finite ON loop or complete-track resident context",
            ));
        }
        Ok(())
    }

    fn note_resident_key_lock_intent(&self, id: usize, enabled: bool) -> PyResult<()> {
        if let Some(guard) = self
            .resident_restore_guards
            .lock()
            .map_err(|_| PyRuntimeError::new_err("resident intent lock poisoned"))?[id]
            .as_ref()
            && self.cold_loading[id].load(Ordering::Acquire) == guard.request
        {
            guard.key_lock_intent(enabled);
        }
        Ok(())
    }

    /// Materialize a bounded full-source reader for non-realtime evidence work.
    /// It shares immutable source authority without changing the effective window.
    #[cfg(test)]
    pub(super) fn complete_sample(
        &self,
        id: usize,
        sample: &SampleBuffer,
        maximum: usize,
    ) -> PyResult<SampleBuffer> {
        native_owned_work::CompleteSampleWork::capture(self, id, sample)
            .and_then(|work| work.materialize(maximum))
            .map_err(PyValueError::new_err)
    }
}

impl AudioEngine {
    /// Preserve the existing Rust test/control call shape; Python uses the strict
    /// restore initializer wrapper registered under the same public method name.
    #[allow(clippy::too_many_arguments)]
    pub fn load_sample_async(
        &self,
        id: usize,
        path: String,
        run_analysis: Option<bool>,
        restore_automatic: bool,
        replace_assignment: bool,
        resident_loop_start_s: Option<f64>,
        resident_loop_end_s: Option<f64>,
        resident_key_lock: bool,
        source_intent: &str,
    ) -> PyResult<u64> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        cold_load::admit(
            self,
            id,
            path,
            run_analysis.unwrap_or(true),
            restore_automatic,
            replace_assignment,
            cold_residency::ResidentLoadHint::parse(
                resident_loop_start_s,
                resident_loop_end_s,
                resident_key_lock,
            )
            .map_err(PyValueError::new_err)?,
            source_intent,
        )
    }
}

#[pymethods]
impl AudioEngine {
    /// Prepare one internal float spool. Encoded recording publication is separate.
    #[pyo3(signature = (spool_path, block_frames=4096, block_count=32, max_pool_pcm_bytes=8388608))]
    pub fn prepare_mix_capture(
        &self,
        spool_path: &str,
        block_frames: usize,
        block_count: usize,
        max_pool_pcm_bytes: usize,
    ) -> PyResult<MixCaptureTicket> {
        self.prepare_mix_capture_internal(spool_path, block_frames, block_count, max_pool_pcm_bytes)
    }

    /// Queue intent only; actual start and frame belong to the callback ACK.
    #[pyo3(signature = (ticket, target_frame=None))]
    pub fn start_mix_capture(
        &self,
        ticket: &MixCaptureTicket,
        target_frame: Option<u64>,
    ) -> PyResult<bool> {
        match target_frame {
            Some(frame) => self.start_mix_capture_internal(ticket, frame),
            None => self.start_mix_capture_target_internal(
                ticket,
                crate::messages::MixCaptureTarget::NextAdmissible,
            ),
        }
    }

    /// Queue an exclusive endpoint using the existing scheduler's reserved stop.
    #[pyo3(signature = (ticket, target_frame=None))]
    pub fn stop_mix_capture(
        &self,
        ticket: &MixCaptureTicket,
        target_frame: Option<u64>,
    ) -> PyResult<bool> {
        match target_frame {
            Some(frame) => self.stop_mix_capture_internal(ticket, frame),
            None => self.stop_mix_capture_target_internal(
                ticket,
                crate::messages::MixCaptureTarget::NextAdmissible,
            ),
        }
    }

    pub fn cancel_mix_capture(&self, ticket: &MixCaptureTicket) -> PyResult<bool> {
        self.cancel_mix_capture_internal(ticket)
    }

    /// Poll actual worker/stream state without waiting for filesystem completion.
    pub fn mix_capture_status(
        &self,
        py: Python<'_>,
        ticket: &MixCaptureTicket,
    ) -> PyResult<Py<PyDict>> {
        self.mix_capture_status_internal(py, ticket)
    }

    /// Accepted global start contract; queue admission never changes this value.
    pub fn key_lock_start_behavior(&self) -> &'static str {
        self.input_runtime_ownership
            .key_lock_start_behavior()
            .as_str()
    }

    /// Callback-owned fully stopped observation, including paused and pending playback.
    pub fn can_change_key_lock_start_behavior(&self) -> bool {
        self.input_runtime_ownership
            .can_change_key_lock_start_behavior()
    }

    /// Synchronously accept while stopped or reject without remembering a later choice.
    pub fn set_key_lock_start_behavior(&self, behavior: &str) -> PyResult<bool> {
        let behavior =
            key_lock_start_policy::KeyLockStartBehavior::parse(behavior).ok_or_else(|| {
                PyValueError::new_err(
                    "Key Lock start behavior must be first_sound_at_grid or attack_at_grid",
                )
            })?;
        Ok(self
            .input_runtime_ownership
            .set_key_lock_start_behavior(behavior))
    }

    /// Device-free capability proof for scoped admission and residency interests.
    #[classattr]
    const RESIDENT_PCM_ADMISSION_VERSION: u32 = 2;
    #[classattr]
    const GLOBAL_TIMING_POLICY_API_VERSION: u32 = 1;
    #[classattr]
    const ORDINARY_TRANSPORT_API_VERSION: u32 = 1;
    /// Seal an already committed immutable disk selection independently of component residency.
    pub fn register_committed_stem_pair(
        &self,
        prepared: &PreparedStemPair,
        source_ticket: &PreparedSourceTicket,
    ) -> PyResult<CommittedStemSetTicket> {
        self.register_committed_stem_pair_with_producer(
            prepared,
            source_ticket,
            self.control_producer()?,
        )
    }

    /// Seal a future complete set without revoking the actual predecessor selection.
    pub fn register_pending_committed_stem_pair(
        &self,
        prepared: &PreparedStemPair,
        source_ticket: &PreparedSourceTicket,
    ) -> PyResult<CommittedStemSetTicket> {
        self.register_pending_committed_stem_pair_with_producer(
            prepared,
            source_ticket,
            self.control_producer()?,
        )
    }

    #[pyo3(signature = (selection_ticket, *, mode="all_stems", enabled_mask=15, interest="explicit"))]
    pub fn request_committed_stem_residency(
        &self,
        selection_ticket: &CommittedStemSetTicket,
        mode: &str,
        enabled_mask: u8,
        interest: &str,
    ) -> PyResult<ResidentStemTicket> {
        self.request_committed_stem_residency_with_interest_and_producer(
            selection_ticket,
            mode,
            enabled_mask,
            interest,
            self.control_producer()?,
        )
    }

    /// Configure automatic host admission or an additional manual limit, without eviction.
    pub fn set_resident_pcm_budget_mib(&self, value: &Bound<'_, PyAny>) -> PyResult<()> {
        if value.is_instance_of::<PyBool>() || !value.is_instance_of::<PyInt>() {
            return Err(PyValueError::new_err(
                "resident_pcm_budget_mib must be 0 (automatic) or an integer in 128..16384",
            ));
        }
        let mib = value.extract::<usize>().map_err(|_| {
            PyValueError::new_err(
                "resident_pcm_budget_mib must be 0 (automatic) or an integer in 128..16384",
            )
        })?;
        self.project_assets
            .set_resident_budget_mib(&self.resident_account, mib)
            .map_err(|error| PyValueError::new_err(error.to_string()))
    }

    /// Exact scoped PCM ownership and pending promises, separately from scratch/RSS.
    pub fn get_resident_pcm_budget_status(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let status = self
            .project_assets
            .resident_budget_status(&self.resident_account)
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        let dict = PyDict::new(py);
        dict.set_item("budget_mib", status.budget_mib)?;
        dict.set_item("automatic", status.automatic)?;
        dict.set_item("budget_bytes", status.budget_bytes)?;
        dict.set_item("held_unique_pcm_bytes", status.held_unique_pcm_bytes)?;
        dict.set_item("pending_resident_bytes", status.pending_resident_bytes)?;
        dict.set_item("admitted_total_bytes", status.admitted_total_bytes)?;
        dict.set_item("unique_backing_count", status.unique_backing_count)?;
        dict.set_item(
            "pending_reservation_count",
            status.pending_reservation_count,
        )?;
        dict.set_item("available_bytes", status.available_bytes)?;
        dict.set_item("over_budget", status.over_budget)?;
        let pending = resident_host_memory::pending_bytes()
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        dict.set_item("host_pending_allocation_bytes", pending)?;
        match resident_host_memory::host_headroom() {
            Ok(image) => {
                dict.set_item("host_available_physical_bytes", image.available_physical)?;
                dict.set_item("host_available_commit_bytes", image.available_commit())?;
                dict.set_item(
                    "host_allocation_allowance_bytes",
                    image.allowance().saturating_sub(pending),
                )?;
                dict.set_item("host_memory_error", py.None())?;
            }
            Err(error) => {
                dict.set_item("host_available_physical_bytes", py.None())?;
                dict.set_item("host_available_commit_bytes", py.None())?;
                dict.set_item("host_allocation_allowance_bytes", py.None())?;
                dict.set_item("host_memory_error", error)?;
            }
        }
        Ok(dict.unbind())
    }

    /// Create a new AudioEngine instance with default audio device.
    #[new]
    pub fn new() -> PyResult<Self> {
        let (loader_tx, loader_rx) = std::sync::mpsc::channel();
        let project_assets = project_assets::ProjectAssets::shared();
        let cold_pcm_history = Arc::new(Mutex::new((0..NUM_SAMPLES).map(|_| Vec::new()).collect()));
        project_assets
            .watch_history(Arc::downgrade(&cold_pcm_history))
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;

        let input_runtime_ownership =
            Arc::new(input_runtime_binding::InputRuntimeOwnership::tracked());
        let global_timing_revision = input_runtime_ownership.global_timing_revision.clone();

        Ok(AudioEngine {
            stream_handle: None,
            stream_lifecycle: None,
            mix_capture_owner: Mutex::new(None),
            #[cfg(test)]
            test_control_channels: None,
            #[cfg(test)]
            test_output_format: None,
            #[cfg(test)]
            test_parameter_producer: None,
            is_playing: false,
            loader_tx,
            loader_rx: Mutex::new(loader_rx),
            pending_loader_event: Mutex::new(None),
            sample_cache: Arc::new(Mutex::new(vec![None; NUM_SAMPLES])),
            loading_sample_ids: Arc::new(Mutex::new(HashSet::new())),
            active_tasks: Arc::new(Mutex::new(HashSet::new())),
            analysis_task_tokens: Arc::new(Mutex::new(HashMap::new())),
            pad_request_ids: Arc::new(Mutex::new(vec![0; NUM_SAMPLES])),
            loaded_source_generations: Arc::new(Mutex::new(vec![(0, 0); NUM_SAMPLES])),
            loaded_source_digests: Arc::new(Mutex::new(vec![None; NUM_SAMPLES])),
            prepared_source_epochs: (0..NUM_SAMPLES)
                .map(|_| Arc::new(AtomicU64::new(1)))
                .collect(),
            pending_prepared_stems: Mutex::new((0..NUM_SAMPLES).map(|_| None).collect()),
            timing_intents: Arc::new(Mutex::new(vec![
                analysis::tempo_acceptance::TimingIntent::Legacy;
                NUM_SAMPLES
            ])),
            current_timing_acknowledgements: Arc::default(),
            loop_acceptance: Arc::default(),
            current_constant_timing: Mutex::new((0..NUM_SAMPLES).map(|_| Vec::new()).collect()),
            input_runtime_ownership,
            pitch_work_capacity: pitch_publication::PitchWorkCapacity::default(),
            global_timing_revision,
            constant_timing_busy: Arc::new(AtomicBool::new(false)),
            offline_jobs: analysis_jobs::OfflineJobs::default(),
            cold_jobs: cold_jobs::ColdJobs::new()
                .map_err(|error| PyRuntimeError::new_err(error.to_string()))?,
            waveform_requests: waveform::WaveformRequests::default(),
            cold_cancelled: Arc::new(AtomicBool::new(false)),
            cold_loading: Arc::new((0..NUM_SAMPLES).map(|_| AtomicU64::new(0)).collect()),
            cold_leases: Arc::new(Mutex::new((0..NUM_SAMPLES).map(|_| None).collect())),
            resident_stem_cache: Arc::new(Mutex::new(
                (0..NUM_SAMPLES)
                    .map(|_| resident_relocation::ResidentStemCache::default())
                    .collect(),
            )),
            resident_restore_guards: Mutex::new((0..NUM_SAMPLES).map(|_| None).collect()),
            incoming_loads: Mutex::new((0..NUM_SAMPLES).map(|_| None).collect()),
            pad_copies: Mutex::new((0..NUM_SAMPLES).map(|_| None).collect()),
            bank_mutations: Mutex::new(Vec::new()),
            cold_lease_generations: Arc::new((0..NUM_SAMPLES).map(|_| AtomicU64::new(0)).collect()),
            cold_pcm_history,
            project_assets,
            project_asset_engine_owner: Arc::new(()),
            resident_account: resident_accounting::ResidentAccount::new(),
            resident_proposals: Arc::default(),
            input_runtime: None,
            input_clock: InputClock::new(),
        })
    }

    /// Initialize and run the audio engine.
    pub fn run(&mut self) -> PyResult<()> {
        if self.stream_handle.is_some() {
            return Err(PyRuntimeError::new_err("AudioEngine already running"));
        }
        if self
            .stream_lifecycle
            .as_ref()
            .is_some_and(|state| state.quiescence != StreamQuiescence::Verified)
        {
            return Err(Self::stream_quiescence_error());
        }
        // A new stream must not reset source/timing truth over the old capture's
        // still-live pool or writer. Its original ticket/control stay unchanged.
        self.ensure_mix_capture_settled_before_run()?;
        if self.cold_cancelled.swap(false, Ordering::AcqRel) {
            self.cold_jobs = cold_jobs::ColdJobs::new()
                .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        }

        self.current_timing_acknowledgements.clear_all();
        self.loop_acceptance.reset();
        self.input_runtime_ownership.revoke_all_sources();
        for records in self
            .current_constant_timing
            .lock()
            .map_err(|_| PyRuntimeError::new_err("current timing lock poisoned"))?
            .iter_mut()
        {
            records.clear();
        }
        match create_audio_stream(
            self.input_clock,
            self.current_timing_acknowledgements.clone(),
            self.input_runtime_ownership.clone(),
            self.prepared_source_epochs.clone(),
            self.loop_acceptance.clone(),
        ) {
            Ok(handle) => {
                if let Err(error) = start_stream(&handle.stream) {
                    let error =
                        PyRuntimeError::new_err(format!("Failed to start audio stream: {error}"));
                    return Err(self.discard_uninstalled_stream(handle, error));
                }
                let runtime = InputRuntime::new_with_ownership(
                    handle.producer.clone(),
                    self.input_clock,
                    self.input_runtime_ownership.clone(),
                );
                if let Err(error) = runtime
                    .attach_pitch_clock(handle.output_clock.clone())
                    .and_then(|()| {
                        runtime.set_render_format(handle.output_sample_rate, handle.output_channels)
                    })
                {
                    drop(runtime);
                    return Err(
                        self.discard_uninstalled_stream(handle, PyRuntimeError::new_err(error))
                    );
                }
                self.input_runtime = Some(runtime);
                self.stream_lifecycle = Some(StreamLifecycle {
                    failed: handle.stream_failed.clone(),
                    quiescence: StreamQuiescence::Open,
                });
                self.stream_handle = Some(handle);
                self.is_playing = true;
                Ok(())
            }
            Err(error) => {
                self.note_stream_startup_error(error.as_ref());
                Err(PyRuntimeError::new_err(format!(
                    "Failed to create audio stream: {error}"
                )))
            }
        }
    }

    /// Observe the actual last installed stream's sticky error, even after close.
    /// This is independent of the original capture's first failure reason.
    pub fn audio_stream_failed(&self) -> bool {
        self.stream_lifecycle
            .as_ref()
            .is_some_and(|state| state.failed.load(Ordering::Acquire))
    }

    /// Close an actually failed stream and verify callback-image retirement.
    /// True is idempotent after that barrier, not a writer/finalization ACK.
    /// Healthy/unstarted engines return false without changing playback.
    pub fn quiesce_failed_audio_stream(&mut self) -> PyResult<bool> {
        self.quiesce_failed_stream_with(|engine| {
            let stream = engine.stream_handle.take().ok_or_else(|| {
                std::io::Error::other(
                    "original failed stream owner is unavailable for verified close",
                )
            })?;
            stream.close_checked()
        })
    }

    pub fn output_sample_rate(&self) -> PyResult<u32> {
        if let Some(handle) = &self.stream_handle {
            return Ok(handle.output_sample_rate);
        }
        #[cfg(test)]
        if self.test_control_channels.is_some() {
            return Ok(48_000);
        }
        Err(PyRuntimeError::new_err("Audio engine not initialized"))
    }

    /// Request one fixed callback observation. No playback action is performed.
    pub fn request_loop_acceptance_snapshot(&self, sample_id: usize) -> PyResult<u64> {
        if self.stream_handle.is_none() {
            return Err(PyRuntimeError::new_err("Audio engine not initialized"));
        }
        self.loop_acceptance.request(sample_id)
    }

    /// One nonblocking read; None means pending, superseded or a busy writer.
    pub fn loop_acceptance_snapshot(
        &self,
        py: Python<'_>,
        sample_id: usize,
        request_id: u64,
    ) -> PyResult<Option<Py<PyAny>>> {
        if self.stream_handle.is_none() {
            return Err(PyRuntimeError::new_err("Audio engine not initialized"));
        }
        loop_acceptance::metadata(self, py, sample_id, request_id)
    }

    /// Actual selected endpoint and stream configuration retained outside the callback.
    pub fn output_device_descriptor(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        let Some(handle) = self.stream_handle.as_ref() else {
            return Ok(None);
        };
        let dict = PyDict::new(py);
        dict.set_item("host", &handle.device_descriptor.host)?;
        dict.set_item("endpoint_name", &handle.device_descriptor.endpoint_name)?;
        dict.set_item(
            "endpoint_description_error",
            &handle.device_descriptor.endpoint_description_error,
        )?;
        dict.set_item("sample_rate_hz", handle.output_sample_rate)?;
        dict.set_item("channels", handle.output_channels)?;
        dict.set_item("stream_sample_format", "f32")?;
        dict.set_item(
            "default_device_sample_format",
            &handle.device_descriptor.default_sample_format,
        )?;
        dict.set_item("requested_buffer_frames", 512)?;
        dict.set_item(
            "buffer_policy",
            "fixed-request; observed callback_frames are separate",
        )?;
        Ok(Some(dict.into_any().unbind()))
    }

    /// Capture observable input time in the engine epoch shared with native MIDI.
    pub fn capture_input_timestamp_ns(&self) -> u64 {
        self.input_clock.capture_ns()
    }

    /// Return a coherent device-clock estimate without waiting for the callback.
    /// `valid` and `fresh` must both be true before using it for timestamp mapping.
    pub fn output_clock_snapshot(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        let Some(snapshot) = handle.output_clock.read() else {
            return Ok(None);
        };
        let dict = PyDict::new(py);
        dict.set_item("valid", snapshot.valid)?;
        dict.set_item("fresh", snapshot.is_fresh(self.input_clock.capture_ns()))?;
        dict.set_item("observed_at_ns", snapshot.observed_at_ns)?;
        dict.set_item("audible_at_ns", snapshot.audible_at_ns)?;
        dict.set_item("output_frame", snapshot.output_frame)?;
        dict.set_item("sample_rate_hz", snapshot.sample_rate_hz)?;
        dict.set_item("master_period_seconds", snapshot.master_period_seconds)?;
        dict.set_item(
            "master_bpm",
            snapshot.master_period_seconds.map(|period| 60.0 / period),
        )?;
        dict.set_item("downbeat_frame", snapshot.downbeat_frame)?;
        Ok(Some(dict.into_any().unbind()))
    }

    /// Diagnostic nearest-grid target. This does not schedule or start playback.
    #[pyo3(signature = (received_at_ns, step_64ths=4))]
    pub fn input_clock_target_frame(
        &self,
        received_at_ns: Option<&Bound<'_, PyAny>>,
        step_64ths: u16,
    ) -> PyResult<Option<u64>> {
        let received_at_ns = parse_input_timestamp(received_at_ns)?;
        let grid = QuantizeGrid::from_step_64ths(step_64ths)
            .ok_or_else(|| PyValueError::new_err("step_64ths must be in 1..=64"))?;
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        Ok(handle.output_clock.read().and_then(|snapshot| {
            snapshot.input_target_frame(received_at_ns, self.input_clock.capture_ns(), grid)
        }))
    }

    pub fn loaded_sample_shape(&self, id: usize) -> PyResult<(u32, usize, usize)> {
        resident_relocation::reconcile(self)?;
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err(format!(
                "id out of range (expected 0..{}, got {id})",
                NUM_SAMPLES - 1
            )));
        }

        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let sample = {
            let cache = self
                .sample_cache
                .lock()
                .map_err(|_| PyRuntimeError::new_err("Failed to acquire sample cache lock"))?;
            cache
                .get(id)
                .and_then(|slot| slot.clone())
                .ok_or_else(|| PyValueError::new_err("sample is not loaded"))?
        };

        let frames = sample.frame_count();
        Ok((handle.output_sample_rate, sample.channels, frames))
    }

    /// Bounded control snapshot of the actual loaded original, without file I/O.
    /// Pending replacement/adoption cannot advertise the previous source digest.
    /// This metadata does not replace a source ticket's publication authority.
    pub fn loaded_source_digest(&self, id: usize) -> PyResult<Option<String>> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        let requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        let loading = self
            .loading_sample_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("loading lock poisoned"))?;
        if loading.contains(&id) || self.cold_loading[id].load(Ordering::Acquire) != 0 {
            return Ok(None);
        }
        let cache = self
            .sample_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
        let Some(sample) = cache[id].as_ref() else {
            return Ok(None);
        };
        let generations = self
            .loaded_source_generations
            .lock()
            .map_err(|_| PyRuntimeError::new_err("source generation lock poisoned"))?;
        let (generation, rate) = generations[id];
        // Analysis can advance the request while retaining this loaded source.
        // Its actual source owner, rather than equal request numbers, is decisive.
        if requests[id] == 0
            || generation == 0
            || rate == 0
            || sample.channels == 0
            || sample.frame_count() == 0
            || self
                .input_runtime_ownership
                .source_generation(id, sample, rate)
                != Some(generation)
        {
            return Ok(None);
        }
        let digests = self
            .loaded_source_digests
            .lock()
            .map_err(|_| PyRuntimeError::new_err("source digest lock poisoned"))?;
        Ok(digests[id]
            .as_ref()
            .filter(|digest| {
                digest.len() == 64
                    && digest
                        .bytes()
                        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            })
            .cloned())
    }

    /// Retain the actual immutable loaded original for an offline stem worker.
    /// This does not capture or grant a live timing/window publication permit.
    pub fn capture_offline_stem_source(
        &self,
        id: usize,
        source_version: String,
    ) -> PyResult<OfflineStemSourceLease> {
        offline_stem_source::capture(self, id, source_version)
    }

    /// Check own loaded source admission without file work or timing/window epochs.
    pub fn offline_stem_source_is_current(
        &self,
        id: usize,
        source: &OfflineStemSourceLease,
    ) -> PyResult<bool> {
        offline_stem_source::is_current(self, id, source)
    }

    /// Atomically bind retained offline custody to a fresh ordinary source permit.
    pub fn capture_prepared_stem_source(
        &self,
        id: usize,
        source: &OfflineStemSourceLease,
    ) -> PyResult<PreparedSourceTicket> {
        offline_stem_source::capture_prepared(self, id, source)
    }

    /// Shut down the audio engine.
    pub fn shut_down(&mut self) -> PyResult<()> {
        self.close_mix_capture_admission();
        if let Some(runtime) = self.input_runtime.as_ref() {
            runtime.set_enabled(false);
        }
        input_runtime_binding::cancel_launches_before_shutdown(
            &self.input_runtime_ownership,
            self.stream_handle.as_ref().map(|handle| &handle.producer),
        )
        .map_err(PyRuntimeError::new_err)?;
        self.cold_jobs.close_admission();
        self.cold_cancelled.store(true, Ordering::Release);
        self.waveform_requests.cancel_all();
        prepared_stems_work::cancel_all(self)?;
        resident_relocation::cancel_all(self)?;
        self.cold_jobs.shutdown();
        self.offline_jobs.cancel(None);
        self.input_runtime = None;
        self.close_audio_stream_checked()?;
        // Pending metadata without a durable Python assignment becomes orphaned
        // at shutdown. Actual PCM readers and assignment/job tokens still protect
        // its bytes; the stopped engine object need not be destroyed to reconcile.
        self.project_asset_engine_owner = Arc::new(());
        self.loop_acceptance.reset();
        self.current_timing_acknowledgements.clear_all();
        for records in self
            .current_constant_timing
            .lock()
            .map_err(|_| PyRuntimeError::new_err("current timing lock poisoned"))?
            .iter_mut()
        {
            records.clear();
        }
        self.is_playing = false;
        Ok(())
    }

    pub fn set_input_mapping_enabled(&self, enabled: bool) -> PyResult<()> {
        let runtime = self
            .input_runtime
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        runtime.set_enabled(enabled);
        Ok(())
    }

    pub fn set_input_learn_active(&self, active: bool) -> PyResult<()> {
        let runtime = self
            .input_runtime
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        runtime.set_learn_capture_active(active);
        Ok(())
    }

    pub fn set_input_mapping_snapshot(&self, mappings: Vec<(String, String)>) -> PyResult<()> {
        let runtime = self
            .input_runtime
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        runtime.replace_mappings(mappings);
        Ok(())
    }

    #[pyo3(signature = (multi_loop, loaded, loop_starts, loop_ends, bindings=None, pitch_rows=None, source_map_rows=None, *, timing_policy_json=None))]
    pub fn set_input_runtime_state(
        &self,
        py: Python<'_>,
        multi_loop: bool,
        loaded: Vec<bool>,
        loop_starts: Vec<f64>,
        loop_ends: Vec<Option<f64>>,
        bindings: Option<Vec<Option<Py<InputRuntimePadBinding>>>>,
        pitch_rows: Option<Vec<Py<PyDict>>>,
        source_map_rows: Option<Vec<Py<PyDict>>>,
        timing_policy_json: Option<String>,
    ) -> PyResult<()> {
        let runtime = self
            .input_runtime
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        let bindings = bindings.unwrap_or_else(|| (0..NUM_SAMPLES).map(|_| None).collect());
        let borrowed: Vec<_> = bindings
            .iter()
            .map(|binding| binding.as_ref().map(|binding| binding.borrow(py)))
            .collect();
        let bindings = borrowed.iter().map(|binding| binding.as_deref()).collect();
        runtime
            .set_runtime_state_with_policy(
                multi_loop,
                loaded,
                loop_starts,
                loop_ends,
                bindings,
                pitch_rows
                    .map(|rows| pitch_api::parse_rows(py, rows))
                    .transpose()?,
                source_map_rows
                    .map(|rows| source_map_api::parse_rows(py, rows))
                    .transpose()?,
                source_sync_policy::GlobalTimingPolicy::parse(timing_policy_json.as_deref())
                    .map_err(PyValueError::new_err)?,
            )
            .map_err(PyValueError::new_err)
    }

    /// GUI selection enters the same Native ordering as mapped device selection.
    pub fn select_input_pad(&self, sample_id: usize) -> PyResult<u64> {
        let runtime = self
            .input_runtime
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        match input_runtime_binding::capture(self, sample_id)? {
            Some(binding) => runtime.set_selected_binding(&binding),
            None => runtime.set_selected_pad(sample_id),
        }
        .map_err(|error| PyValueError::new_err(error.to_string()))
    }

    pub fn input_selected_projection(&self) -> PyResult<(u64, Option<usize>)> {
        self.input_runtime
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?
            .selected_projection()
            .map_err(|error| PyValueError::new_err(error.to_string()))
    }

    #[pyo3(signature = (sample_id, action_key, received_at_ns=None))]
    pub fn capture_pad_pitch_action(
        &self,
        sample_id: usize,
        action_key: &str,
        received_at_ns: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<CapturedPitchAction> {
        let runtime = self
            .input_runtime
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        let binding = input_runtime_binding::capture(self, sample_id)?
            .ok_or_else(|| PyRuntimeError::new_err("pitch source is unavailable"))?;
        let now = self.input_clock.capture_ns();
        let timestamp =
            validated_input_timestamp(parse_input_timestamp(received_at_ns)?, now).unwrap_or(now);
        runtime
            .capture_gui_pitch(&binding, action_key, timestamp)
            .map_err(|error| PyValueError::new_err(error.to_string()))
    }

    #[pyo3(signature = (id, start_s, end_s=None, volume=1.0, *, exclusive=false, received_at_ns=None, expected_source_generation=None))]
    #[allow(clippy::too_many_arguments)]
    pub fn capture_pad_start(
        &self,
        id: usize,
        start_s: f64,
        end_s: Option<f64>,
        volume: f32,
        exclusive: bool,
        received_at_ns: Option<&Bound<'_, PyAny>>,
        expected_source_generation: Option<u64>,
    ) -> PyResult<CapturedPadStart> {
        self._capture_pad_start(
            id,
            start_s,
            end_s,
            volume,
            exclusive,
            received_at_ns,
            expected_source_generation,
        )
    }

    #[pyo3(name = "prepare_captured_pad_start", signature = (captured, window=None))]
    pub fn python_prepare_captured_pad_start(
        &self,
        captured: &CapturedPadStart,
        window: Option<&resident_relocation::ResidentWindowTicket>,
    ) -> PyResult<resident_relocation::ResidentWindowTicket> {
        self._prepare_captured_pad_start(captured, window)
    }

    #[pyo3(signature = (captured, window=None))]
    pub fn publish_pad_start(
        &self,
        captured: &CapturedPadStart,
        window: Option<&resident_relocation::ResidentWindowTicket>,
    ) -> PyResult<bool> {
        self._publish_pad_start(captured, window)
    }

    #[pyo3(signature = (action_key, received_at_ns=None))]
    pub fn capture_selected_pitch_action(
        &self,
        action_key: &str,
        received_at_ns: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<CapturedPitchAction> {
        let runtime = self
            .input_runtime
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        let now = self.input_clock.capture_ns();
        let timestamp =
            validated_input_timestamp(parse_input_timestamp(received_at_ns)?, now).unwrap_or(now);
        runtime
            .capture_selected_pitch(action_key, timestamp)
            .map_err(|error| PyValueError::new_err(error.to_string()))
    }

    /// Capture actual loaded source and current acknowledged timing for MIDI admission.
    pub fn current_input_runtime_pad_binding(
        &self,
        sample_id: usize,
    ) -> PyResult<Option<InputRuntimePadBinding>> {
        if sample_id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("sample id out of range"));
        }
        input_runtime_binding::capture(self, sample_id)
    }

    /// Retry MIDI through the same current guarded transaction, retaining input time.
    #[pyo3(signature = (sample_id, received_at_ns=None))]
    pub fn trigger_input_runtime_pad(
        &self,
        sample_id: usize,
        received_at_ns: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<bool> {
        if sample_id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("sample id out of range"));
        }
        let now = self.input_clock.capture_ns();
        let timestamp = validated_input_timestamp(parse_input_timestamp(received_at_ns)?, now);
        let runtime = self
            .input_runtime
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        Ok(runtime.trigger_pad(sample_id, timestamp.unwrap_or(now)))
    }

    pub fn start_midi_input(&self) -> PyResult<usize> {
        let runtime = self
            .input_runtime
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        runtime.start_midi_input().map_err(PyRuntimeError::new_err)
    }

    pub fn stop_midi_input(&self) -> PyResult<()> {
        let runtime = self
            .input_runtime
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        runtime.stop_midi_input();
        Ok(())
    }

    pub fn inject_midi_input_for_test(&self, message: Vec<u8>) -> PyResult<bool> {
        let runtime = self
            .input_runtime
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        Ok(runtime.inject_midi_message(&message))
    }

    /// Admit copy-first complete cold preparation in the fixed worker lane.
    #[allow(clippy::too_many_arguments)] // Fixed public source/load options preserve the existing API.
    #[pyo3(name = "load_sample_async", signature = (id, path, run_analysis=None, restore_automatic=false, replace_assignment=false, resident_loop_start_s=None, resident_loop_end_s=None, resident_key_lock=false, source_intent="auto", initial_base_shift=None, initial_extra_shift=None, initial_key_lock=None, initial_gain_db=None, initial_eq_db=None))]
    pub fn load_sample_async_with_pitch(
        &self,
        id: usize,
        path: String,
        run_analysis: Option<bool>,
        restore_automatic: bool,
        replace_assignment: bool,
        resident_loop_start_s: Option<f64>,
        resident_loop_end_s: Option<f64>,
        resident_key_lock: bool,
        source_intent: &str,
        initial_base_shift: Option<&Bound<'_, PyAny>>,
        initial_extra_shift: Option<&Bound<'_, PyAny>>,
        initial_key_lock: Option<&Bound<'_, PyAny>>,
        initial_gain_db: Option<&Bound<'_, PyAny>>,
        initial_eq_db: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<u64> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        let initial_pitch = cold_load::parse_initial_pitch(
            source_intent,
            initial_base_shift,
            initial_extra_shift,
            initial_key_lock,
        )?;
        let initial_mix =
            cold_load::parse_initial_mix(source_intent, initial_gain_db, initial_eq_db)?;
        cold_load::admit_with_initial_pitch(
            self,
            id,
            path,
            run_analysis.unwrap_or(true),
            restore_automatic,
            replace_assignment,
            cold_residency::ResidentLoadHint::parse(
                resident_loop_start_s,
                resident_loop_end_s,
                resident_key_lock,
            )
            .map_err(PyValueError::new_err)?,
            source_intent,
            initial_pitch,
            initial_mix,
        )
    }

    /// Ordinary incoming assignment uses its own analysed defaults before live selection.
    pub fn load_incoming_sample_async(
        &self,
        id: usize,
        path: String,
        key_lock: bool,
    ) -> PyResult<u64> {
        cold_load::admit_incoming(self, id, path, key_lock)
    }

    /// Update only an explicitly addressed incoming request, never its predecessor.
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (id, request_id, revision, key_lock, loop_start_s=None, loop_end_s=None, loop_auto=true, loop_bars=8.0, timing_override=false, period_seconds=None, grid_origin_s=None))]
    pub fn update_incoming_load_intent(
        &self,
        id: usize,
        request_id: u64,
        revision: u64,
        key_lock: bool,
        loop_start_s: Option<f64>,
        loop_end_s: Option<f64>,
        loop_auto: bool,
        loop_bars: f64,
        timing_override: bool,
        period_seconds: Option<f64>,
        grid_origin_s: Option<f64>,
    ) -> PyResult<bool> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        let requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        if requests[id] != request_id && self.cold_loading[id].load(Ordering::Acquire) != request_id
        {
            return Err(PyValueError::new_err("incoming request is stale"));
        }
        let incoming = self
            .incoming_loads
            .lock()
            .map_err(|_| PyRuntimeError::new_err("incoming intent lock poisoned"))?;
        let incoming = incoming[id]
            .as_ref()
            .filter(|incoming| incoming.request == request_id)
            .ok_or_else(|| PyValueError::new_err("incoming request is unavailable"))?;
        let intent = incoming_load::IncomingIntent {
            revision,
            key_lock,
            loop_start_s,
            loop_end_s,
            loop_auto,
            loop_bars,
            timing_override,
            period_seconds,
            grid_origin_s,
        };
        intent
            .validate_for_rate(self.output_sample_rate()?)
            .map_err(PyValueError::new_err)?;
        incoming.update(intent).map_err(PyValueError::new_err)
    }

    pub fn hold_material_migration(&self, ids: Vec<usize>) -> PyResult<MaterialMigrationHold> {
        material_migration_control::hold(self, ids)
    }

    pub fn migration_artifact_ledger_supported(&self) -> bool {
        true
    }

    #[pyo3(signature = (sample_id, material_creation_json, config_reference, instance_id, inventory, journal_json=None))]
    pub fn restore_material_creation(
        slf: &Bound<'_, Self>,
        py: Python<'_>,
        sample_id: usize,
        material_creation_json: String,
        config_reference: String,
        instance_id: String,
        inventory: &MigrationInventoryLease,
        journal_json: Option<String>,
    ) -> PyResult<()> {
        let work = {
            let engine = slf.try_borrow()?;
            material_migration_recovery::MaterialCreationRestoreWork::capture(
                &engine,
                sample_id,
                material_creation_json,
                config_reference,
                instance_id,
                inventory,
                journal_json,
            )
            .map_err(|error| PyValueError::new_err(error.to_string()))?
        };
        let prepared = py
            .detach(move || work.prepare())
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
        let engine = slf.try_borrow()?;
        prepared
            .finish(&engine)
            .map_err(|error| PyValueError::new_err(error.to_string()))
    }

    pub fn prepare_material_migration(
        &self,
        source: String,
    ) -> PyResult<MaterialMigrationPreparation> {
        material_migration_control::prepare(self, source)
    }

    pub fn prepare_material_migration_stems(
        &self,
        preparation: &MaterialMigrationPreparation,
        old_cache: String,
        old_source_version: String,
        new_source_version: String,
        generation_id: String,
    ) -> PyResult<MaterialMigrationStemPreparation> {
        material_migration_control::prepare_stems(
            self,
            preparation,
            old_cache,
            old_source_version,
            new_source_version,
            generation_id,
        )
    }

    pub fn adopt_material_migration(
        &self,
        sample_id: usize,
        preparation: &MaterialMigrationPreparation,
    ) -> PyResult<MaterialMigrationSourceTicket> {
        material_migration_control::adopt(self, sample_id, preparation)
    }

    /// Current resident allocation and full-source extent; pending relocation
    /// tickets expose their own ACK rather than relabelling this effective view.
    pub fn loaded_residency(&self, py: Python<'_>, id: usize) -> PyResult<Py<PyDict>> {
        resident_relocation::reconcile(self)?;
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        if self.cold_loading[id].load(Ordering::Acquire) != 0 {
            return Err(PyValueError::new_err(
                "source/window is pending native adoption",
            ));
        }
        let sample = self
            .sample_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?[id]
            .clone()
            .ok_or_else(|| PyValueError::new_err("sample is not loaded"))?;
        let dict = PyDict::new(py);
        dict.set_item("full_frame_count", sample.frame_count())?;
        dict.set_item("resident_start_frame", sample.resident_start())?;
        dict.set_item("resident_end_frame", sample.resident_end())?;
        dict.set_item(
            "resident_frame_count",
            sample.samples.len() / sample.channels,
        )?;
        dict.set_item(
            "resident_pcm_bytes",
            sample.samples.len() * size_of::<f32>(),
        )?;
        dict.set_item("window_revision", sample.window_revision())?;
        dict.set_item("source_identity", sample.source_address())?;
        dict.set_item("cache_backed", sample.residency.is_some())?;
        dict.set_item(
            "source_zero_frame",
            sample
                .residency
                .as_ref()
                .map_or(0, |view| view.source.source_zero_frame),
        )?;
        dict.set_item(
            "context",
            match sample.residency.as_ref().map(|view| view.context) {
                Some(crate::messages::ResidentContext::FiniteLoop) => "finite-loop-exact-taps-v1",
                Some(crate::messages::ResidentContext::KeyLockFiniteLoop) => {
                    "finite-keylock-source-coverage-v1"
                }
                Some(crate::messages::ResidentContext::KeyLockFullTrack) => {
                    "full-track-key-lock-continuation-unproved-v1"
                }
                _ => "full-track-v1",
            },
        )?;
        Ok(dict.unbind())
    }

    /// Prepare an acknowledged replacement view of the identical complete source.
    pub fn relocate_resident_window(
        &self,
        id: usize,
        start_s: f64,
        end_s: f64,
    ) -> PyResult<resident_relocation::ResidentWindowTicket> {
        resident_relocation::relocate(self, id, start_s, end_s)
    }

    /// Prepare one source-bound control transaction; native ACK publishes its
    /// storage and complete loop/seek/Key Lock effect together.
    #[pyo3(name = "prepare_resident_control", signature = (id, start_s=None, end_s=None, position_s=None, key_lock=None, *, expected_source_generation=None))]
    pub fn prepare_resident_control_with_source(
        &self,
        id: usize,
        start_s: Option<f64>,
        end_s: Option<f64>,
        position_s: Option<f64>,
        key_lock: Option<bool>,
        expected_source_generation: Option<u64>,
    ) -> PyResult<resident_relocation::ResidentWindowTicket> {
        if start_s.is_none() && end_s.is_some() {
            return Err(PyValueError::new_err("end_s requires start_s"));
        }
        resident_relocation::prepare_control(
            self,
            id,
            resident_relocation::WindowRequest {
                loop_region: start_s.map(|start| (start, end_s)),
                seek_position_s: position_s,
                key_lock,
                expected_source_generation,
                ..resident_relocation::WindowRequest::default()
            },
        )
    }

    /// Launch the acknowledged source/window with its original input timestamp.
    /// The intent and source guards remain current through native scheduling.
    #[pyo3(signature = (ticket, exclusive=false, received_at_ns=None))]
    pub fn play_resident_control(
        &self,
        ticket: &resident_relocation::ResidentWindowTicket,
        exclusive: bool,
        received_at_ns: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<bool> {
        let now = self.input_clock.capture_ns();
        let received_at_ns =
            validated_input_timestamp(parse_input_timestamp(received_at_ns)?, now).unwrap_or(now);
        resident_relocation::launch_with_producer(
            self,
            ticket,
            exclusive,
            received_at_ns,
            self.control_producer()?,
        )
    }

    /// Actual complete cold manifest; contains no accepted-timing claim.
    pub fn cold_source_manifest(&self, id: usize) -> PyResult<Option<String>> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        let generation = self
            .loaded_source_generations
            .lock()
            .map_err(|_| PyRuntimeError::new_err("source generation lock poisoned"))?[id]
            .0;
        if self.cold_loading[id].load(Ordering::Acquire) != 0
            || self.cold_lease_generations[id].load(Ordering::Acquire) != generation
            || generation == 0
        {
            return Ok(None);
        }
        let leases = self
            .cold_leases
            .lock()
            .map_err(|_| PyRuntimeError::new_err("cold lease lock poisoned"))?;
        leases[id].as_ref().map(|lease| serde_json::to_string(&serde_json::json!({"identity":lease.manifest.identity,"decoder_identity":lease.manifest.decoder_identity,"descriptor":lease.manifest.descriptor,"cache_path":lease.cache_path,"original_path":lease.original_path,"material_id":lease.material_id,"material_creation":lease.material_creation_receipt(),"integrity":{
            "warm":lease.integrity.warm,
            "source_copied_bytes":lease.integrity.source_copied_bytes,
            "incoming_verify_bytes":lease.integrity.incoming_verify_bytes,
            "snapshot_verify_bytes":lease.integrity.snapshot_verify_bytes,
            "original_copied_bytes":lease.integrity.original_copied_bytes,
            "original_verify_bytes":lease.integrity.original_verify_bytes,
            "decoder_verify_bytes":lease.integrity.decoder_verify_bytes,
            "playback_verify_bytes":lease.integrity.playback_verify_bytes,
            "manifest_verify_bytes":lease.integrity.manifest_verify_bytes,
            "playback_read_bytes":lease.integrity.playback_read_bytes,
            "assignment_copy_bytes":lease.integrity.assignment_copy_bytes,
            "stream_scratch_reserved_bytes":lease.integrity.stream_scratch_reserved_bytes,
            "warm_validation_wall_nanos":lease.integrity.wall_nanos,
            "warm_validation_cpu_nanos":lease.integrity.cpu_nanos,
        }})).map_err(|error| PyRuntimeError::new_err(error.to_string()))).transpose()
    }

    /// Pin an owned project original or exact stem generation for assignment/job use.
    pub fn acquire_project_asset_lease(&self, path: String) -> PyResult<ProjectAssetLease> {
        self.project_assets
            .acquire(&self.project_assets_root()?, std::path::Path::new(&path))
            .map_err(|error| PyValueError::new_err(error.to_string()))
    }

    #[pyo3(signature = (path, recursive=false))]
    pub fn retire_project_asset(&self, path: String, recursive: bool) -> PyResult<()> {
        self.project_assets
            .retire(
                &self.project_assets_root()?,
                std::path::Path::new(&path),
                recursive,
            )
            .map_err(|error| PyValueError::new_err(error.to_string()))
    }

    /// Pending targets, retained native records, successful deletions, terminal errors.
    pub fn project_asset_cleanup_status(&self) -> PyResult<(usize, usize, u64, Vec<String>)> {
        let (pending, readers, deleted, mut errors) = self
            .project_assets
            .status()
            .map_err(PyRuntimeError::new_err)?;
        let (cache_pending, cache_deleted, cache_errors) = cold_store::cleanup_status()
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        errors.extend(cache_errors);
        Ok((
            pending.saturating_add(cache_pending),
            readers,
            deleted.saturating_add(cache_deleted),
            errors,
        ))
    }

    /// Reserved cold-store cleanup slots and their finite admission capacity.
    pub fn project_asset_cleanup_admission_status(&self) -> (usize, usize) {
        cold_store::cleanup_admission_status()
    }

    /// Pin loaded PCM for the optional diagnostic adapter (not the default analyzer).
    pub fn begin_offline_analysis(&self, id: usize) -> PyResult<OfflineAnalysisJob> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        self.stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        if self
            .loading_sample_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("loading lock poisoned"))?
            .contains(&id)
        {
            return Err(PyValueError::new_err("sample is currently loading"));
        }
        if has_active_task_for_id(
            &*self
                .active_tasks
                .lock()
                .map_err(|_| PyRuntimeError::new_err("task lock poisoned"))?,
            self.active_task_cell(id).map_err(PyRuntimeError::new_err)?,
        ) {
            return Err(PyValueError::new_err("sample task already running"));
        }
        let sample = self
            .sample_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?
            .get(id)
            .and_then(Clone::clone)
            .ok_or_else(|| PyValueError::new_err("sample is not loaded"))?;
        let reader = complete_context::CompleteSourceReader::capture(self, id, sample)
            .map_err(PyRuntimeError::new_err)?;
        let (generation, loaded_rate) = self
            .loaded_source_generations
            .lock()
            .map_err(|_| PyRuntimeError::new_err("source generation lock poisoned"))?[id];
        self.offline_jobs
            .begin_complete(
                id,
                reader,
                loaded_rate,
                generation,
                analysis_jobs::OfflineRequestOwner {
                    request_ids: self.pad_request_ids.clone(),
                    prepared_epoch: self
                        .prepared_source_epoch(id)
                        .map_err(PyRuntimeError::new_err)?
                        .clone(),
                },
                self.loader_tx.clone(),
            )
            .map_err(PyRuntimeError::new_err)
    }

    /// Validate map data against an original loaded source without PCM export.
    /// The existing offline reservation owns request freshness and actual retirement.
    pub fn begin_source_beatmap(&self, sample_id: usize) -> PyResult<OfflineAnalysisJob> {
        self.begin_source_beatmap_admitted(sample_id, None)
    }

    /// Acquire only unverified complete-original legacy QM observations.
    #[pyo3(signature=(sample_id,*,expected_source_generation))]
    pub fn begin_source_beatmap_raw_async(
        &self,
        sample_id: usize,
        expected_source_generation: u64,
    ) -> PyResult<OfflineAnalysisJob> {
        self.begin_source_beatmap_raw_async_with(sample_id, expected_source_generation, || {})
    }

    /// Analyze complete original fullmix with the explicitly installed Beat This worker.
    #[pyo3(signature=(sample_id,installation_root,*,expected_source_generation))]
    pub fn begin_source_beatmap_beat_this_raw_async(
        &self,
        sample_id: usize,
        installation_root: String,
        expected_source_generation: u64,
    ) -> PyResult<OfflineAnalysisJob> {
        self.begin_source_beatmap_beat_this_raw_async_with(
            sample_id,
            expected_source_generation,
            installation_root,
            || {},
        )
    }

    /// Build an accepted map from explicit labels on a genuine original capture.
    #[pyo3(signature=(sample_id,candidate,review_json,*,expected_source_generation))]
    pub fn begin_source_beatmap_review_async(
        &self,
        sample_id: usize,
        candidate: &SourceBeatmapRawCandidate,
        review_json: String,
        expected_source_generation: u64,
    ) -> PyResult<OfflineAnalysisJob> {
        self.begin_source_beatmap_review_async_with(
            sample_id,
            candidate,
            review_json,
            expected_source_generation,
            || {},
        )
    }

    /// Queue source-bound map verification on the existing bounded cold worker lane.
    pub fn begin_source_beatmap_async(
        &self,
        sample_id: usize,
        record_json: String,
    ) -> PyResult<OfflineAnalysisJob> {
        self.begin_source_beatmap_async_with(sample_id, record_json, || {})
    }

    /// Export immutable map data without changing project or playback selection.
    pub fn begin_source_beatmap_store_async(
        &self,
        sample_id: usize,
        record_json: String,
    ) -> PyResult<OfflineAnalysisJob> {
        self.begin_source_beatmap_store_async_with(sample_id, record_json, || {})
    }

    /// Restore a compact historical receipt under newly captured source custody.
    #[pyo3(signature=(sample_id,origin_seconds,period_seconds,*,expected_source_generation=None))]
    pub fn begin_source_beatmap_create_async(
        &self,
        sample_id: usize,
        origin_seconds: f64,
        period_seconds: f64,
        expected_source_generation: Option<u64>,
    ) -> PyResult<OfflineAnalysisJob> {
        self.begin_source_beatmap_create_async_with(
            sample_id,
            origin_seconds,
            period_seconds,
            expected_source_generation,
            || {},
        )
    }

    /// Correct one exact receipt under the original source-bound worker job.
    pub fn begin_source_beatmap_edit_async(
        &self,
        sample_id: usize,
        receipt_json: String,
        operation_json: String,
    ) -> PyResult<OfflineAnalysisJob> {
        self.begin_source_beatmap_edit_async_with(sample_id, receipt_json, operation_json, || {})
    }

    /// Restore a compact historical receipt under newly captured source custody.
    pub fn begin_source_beatmap_restore_async(
        &self,
        sample_id: usize,
        receipt_json: String,
    ) -> PyResult<OfflineAnalysisJob> {
        self.begin_source_beatmap_restore_async_with(sample_id, receipt_json, || {})
    }

    /// Declare automatic/manual/TAP/legacy timing authority under the native owner.
    #[pyo3(name = "set_pad_timing_intent", signature = (sample_id, intent, *, expected_source_generation=None))]
    pub fn set_pad_timing_intent_with_source(
        &self,
        sample_id: usize,
        intent: &str,
        expected_source_generation: Option<u64>,
    ) -> PyResult<()> {
        if sample_id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        let intent = constant_timing::parse_intent(intent)?;
        constant_timing::set_intent_with_source(
            self,
            self.control_producer()?,
            sample_id,
            intent,
            expected_source_generation,
        )
    }

    /// Read declared native authority without claiming callback adoption or changing it.
    pub fn pad_timing_intent(&self, sample_id: usize) -> PyResult<&'static str> {
        if sample_id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        let intents = self
            .timing_intents
            .lock()
            .map_err(|_| PyRuntimeError::new_err("timing intent lock poisoned"))?;
        Ok(match intents[sample_id] {
            analysis::tempo_acceptance::TimingIntent::Automatic => "automatic",
            analysis::tempo_acceptance::TimingIntent::Manual => "manual",
            analysis::tempo_acceptance::TimingIntent::Tap => "tap",
            analysis::tempo_acceptance::TimingIntent::Legacy => "legacy",
        })
    }

    /// Current acknowledged accepted timing for the actual native loaded source.
    /// Historical tickets and pending publications do not establish availability.
    pub fn current_constant_timing(
        &self,
        py: Python<'_>,
        sample_id: usize,
    ) -> PyResult<Option<Py<PyAny>>> {
        if sample_id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        if self.control_producer().is_err() {
            return Ok(None);
        }
        constant_timing::current_metadata(self, py, sample_id)
    }

    /// Export only complete evidence for current acknowledged, content-verified timing.
    pub fn export_current_constant_timing(
        slf: &Bound<'_, Self>,
        py: Python<'_>,
        sample_id: usize,
        source_path: String,
    ) -> PyResult<Option<String>> {
        let work = {
            let engine = slf.try_borrow()?;
            if engine.control_producer().is_err() {
                return Ok(None);
            }
            constant_timing::SavedTimingExportWork::capture(&engine, sample_id, source_path)
                .map_err(PyValueError::new_err)?
        };
        let Some(work) = work else {
            return Ok(None);
        };
        let prepared = py
            .detach(move || work.prepare())
            .map_err(PyValueError::new_err)?;
        prepared
            .finish(&*slf.try_borrow()?)
            .map_err(PyValueError::new_err)
    }

    /// Capture fresh source/request/authority before background restore starts.
    pub fn capture_saved_constant_timing(
        &self,
        sample_id: usize,
        record_json: &str,
        source_path: String,
    ) -> PyResult<SavedConstantTimingTicket> {
        self.control_producer()?;
        constant_timing::capture_saved(self, sample_id, record_json, source_path)
            .map_err(PyValueError::new_err)
    }

    /// Verify actual source/mono/analyzer evidence and enqueue fresh guarded adoption.
    pub fn restore_constant_timing(
        slf: &Bound<'_, Self>,
        py: Python<'_>,
        ticket: &SavedConstantTimingTicket,
    ) -> PyResult<ConstantTimingTicket> {
        let (work, producer) = {
            let engine = slf.try_borrow()?;
            let producer = engine.control_producer()?.clone();
            let work = constant_timing::SavedTimingRestoreWork::capture(&engine, ticket)
                .map_err(PyValueError::new_err)?;
            (work, producer)
        };
        let prepared = py
            .detach(move || work.prepare())
            .map_err(PyValueError::new_err)?;
        prepared.finish(&*slf.try_borrow()?, &producer)
    }

    /// Read the actual shared Native marker when no derived ticket exists yet.
    /// This wait observation never captures a new content or timing authority.
    pub fn _content_placement_pending(&self) -> bool {
        self.input_runtime_ownership.placement_pending()
    }

    /// Preserve an observation of marker contention across control reads, even
    /// after a rejected claim has restored the original complete layout.
    pub fn _content_placement_read_token(&self) -> Option<u64> {
        self.input_runtime_ownership.placement_read_token()
    }

    /// Explicitly run complete native QM evidence on the actual current loaded source.
    pub fn prepare_constant_timing(
        slf: &Bound<'_, Self>,
        py: Python<'_>,
        sample_id: usize,
        timing_error_halfwidth_seconds: f64,
        timing_error_provenance: String,
    ) -> PyResult<ConstantTimingTicket> {
        let work = {
            let engine = slf.try_borrow()?;
            engine.control_producer()?;
            constant_timing::ConstantTimingPreparationWork::capture(
                &engine,
                sample_id,
                analysis::tempo_evidence::TimingBound {
                    halfwidth_seconds: timing_error_halfwidth_seconds,
                    provenance: timing_error_provenance,
                },
            )
            .map_err(PyRuntimeError::new_err)?
        };
        let prepared = py
            .detach(move || work.prepare())
            .map_err(PyRuntimeError::new_err)?;
        prepared
            .finish(&*slf.try_borrow()?)
            .map_err(PyRuntimeError::new_err)
    }

    /// Capture actual source/request/authority synchronously before off-thread analysis.
    #[pyo3(signature = (binding, timing_error_halfwidth_seconds, timing_error_provenance, *, pcm_limit_bytes = constant_timing::MAX_PCM_BYTES))]
    pub fn capture_current_constant_timing(
        &self,
        binding: &InputRuntimePadBinding,
        timing_error_halfwidth_seconds: f64,
        timing_error_provenance: String,
        #[pyo3(from_py_with = parse_constant_timing_pcm_limit)] pcm_limit_bytes: usize,
    ) -> PyResult<CapturedConstantTiming> {
        self.control_producer()?;
        constant_timing::capture_preparation_with_limit(
            self,
            binding.id,
            analysis::tempo_evidence::TimingBound {
                halfwidth_seconds: timing_error_halfwidth_seconds,
                provenance: timing_error_provenance,
            },
            Some(binding),
            pcm_limit_bytes,
        )
        .map_err(PyValueError::new_err)
    }

    /// Prepare only the captured immutable source; newer requests reject this work.
    pub fn prepare_captured_constant_timing(
        slf: &Bound<'_, Self>,
        py: Python<'_>,
        captured: &CapturedConstantTiming,
    ) -> PyResult<ConstantTimingTicket> {
        let work = {
            let engine = slf.try_borrow()?;
            engine.control_producer()?;
            constant_timing::ConstantTimingPreparationWork::capture_existing(&engine, captured)
                .map_err(PyRuntimeError::new_err)?
        };
        let prepared = py
            .detach(move || work.prepare())
            .map_err(PyRuntimeError::new_err)?;
        prepared
            .finish(&*slf.try_borrow()?)
            .map_err(PyRuntimeError::new_err)
    }

    /// Admit one guarded loop and optional master/bootstrap effect from current acceptance.
    #[pyo3(signature = (binding, start_s, end_s, master_period_seconds=None))]
    pub fn refresh_current_constant_timing(
        &self,
        binding: &InputRuntimePadBinding,
        start_s: f64,
        end_s: Option<f64>,
        master_period_seconds: Option<f64>,
    ) -> PyResult<AcceptedTimingRefreshTicket> {
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        accepted_timing_refresh::enqueue(
            self,
            &handle.producer,
            binding,
            start_s,
            end_s,
            master_period_seconds,
        )
    }

    /// Construct accepted timing from explicit independent assertions and publish natively.
    // Keep explicit evidence assertions distinct at the public control boundary.
    #[allow(clippy::too_many_arguments)]
    pub fn publish_constant_timing(
        slf: &Bound<'_, Self>,
        py: Python<'_>,
        ticket: &ConstantTimingTicket,
        hypotheses_json: String,
        origin_seconds: f64,
        origin_provenance: String,
        acceptance_policy_version: String,
        acceptance_provenance: String,
    ) -> PyResult<()> {
        let producer = slf.try_borrow()?.control_producer()?.clone();
        let timing = py.detach(|| {
            constant_timing::prepare_publication(
                ticket,
                &hypotheses_json,
                analysis::tempo_acceptance::IndependentTimingOrigin {
                    seconds: origin_seconds,
                    provenance: origin_provenance,
                },
                analysis::tempo_acceptance::TimingAcceptanceDecision {
                    policy_version: acceptance_policy_version,
                    provenance: acceptance_provenance,
                },
            )
        })?;
        constant_timing::publish_record(&*slf.try_borrow()?, &producer, ticket, timing, false)
    }

    /// Analyze a previously loaded sample on a background thread.
    pub fn analyze_sample_async(&self, id: usize) -> PyResult<u64> {
        if self.offline_jobs.has_pad(id) {
            return Err(PyValueError::new_err(
                "offline analysis is running or retiring",
            ));
        }
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err(format!(
                "id out of range (expected 0..{}, got {id})",
                NUM_SAMPLES - 1
            )));
        }

        self.stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        complete_context::start_analysis(self, id)
    }

    /// Legacy placeholder generation is disabled; productive jobs use the Python backend.
    pub fn generate_stems_async(
        &self,
        id: usize,
        source_version: String,
        cache_dir: String,
    ) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        if source_version.trim().is_empty() {
            return Err(PyValueError::new_err("source_version must not be empty"));
        }
        project_stem_cache_dir(&cache_dir).map_err(PyValueError::new_err)?;
        self.stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        Err(PyRuntimeError::new_err(
            "Legacy native stem generation is disabled; use the ticket-bound offline separator",
        ))
    }

    /// Capture real loaded-source ownership before starting a preparation job.
    /// The original content token must match the digest recorded by the loader.
    pub fn capture_prepared_source(
        &self,
        id: usize,
        source_version: String,
    ) -> PyResult<PreparedSourceTicket> {
        prepared_source::capture_prepared_source(self, id, source_version)
    }

    /// Background complete-pair preparation; only four components are optionally resident.
    #[pyo3(signature=(id,source_version,cache_dir,source_ticket,components,descriptor_reference=None,artifact_source_version=None))]
    pub fn prepare_stem_pair(
        slf: &Bound<'_, Self>,
        py: Python<'_>,
        id: usize,
        source_version: String,
        cache_dir: String,
        source_ticket: &PreparedSourceTicket,
        components: bool,
        descriptor_reference: Option<String>,
        artifact_source_version: Option<String>,
    ) -> PyResult<PreparedStemPair> {
        let work = {
            let engine = slf.try_borrow()?;
            let root = engine.project_assets_root()?;
            engine
                .capture_stem_pair_work(&root, id, &source_version, source_ticket)
                .map_err(PyValueError::new_err)?
        };
        // Python UI receivers may borrow this engine while immutable worker I/O runs.
        py.detach(move || {
            work.prepare(
                &cache_dir,
                components,
                descriptor_reference.as_deref(),
                artifact_source_version.as_deref(),
            )
        })
        .map_err(PyValueError::new_err)
    }

    /// Publish already prepared buffers with this subscriber's own current ticket.
    pub fn publish_stem_pair(
        &self,
        prepared: &PreparedStemPair,
        source_ticket: &PreparedSourceTicket,
    ) -> PyResult<()> {
        self.publish_stem_pair_api(prepared, source_ticket)
    }

    /// Explicit source-bound Instrumental access for offline workers.
    pub fn open_instrumental_stem_reader(
        slf: &Bound<'_, Self>,
        py: Python<'_>,
        id: usize,
        source_version: String,
        descriptor_reference: String,
    ) -> PyResult<InstrumentalStemReader> {
        let work = {
            let engine = slf.try_borrow()?;
            let ticket = engine.capture_prepared_source(id, source_version.clone())?;
            let root = engine.project_assets_root()?;
            engine
                .capture_stem_pair_work(&root, id, &source_version, &ticket)
                .map_err(PyValueError::new_err)?
        };
        let prepared = py
            .detach(move || work.prepare("", false, Some(&descriptor_reference), None))
            .map_err(PyValueError::new_err)?;
        prepared.instrumental_reader(py)
    }

    /// Recheck the admission snapshot before and after off-thread artifact preparation.
    pub fn publish_prepared_stems(
        slf: &Bound<'_, Self>,
        py: Python<'_>,
        id: usize,
        source_version: String,
        cache_dir: String,
        source_ticket: &PreparedSourceTicket,
    ) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err(format!(
                "id out of range (expected 0..{}, got {id})",
                NUM_SAMPLES - 1
            )));
        }
        if source_version.trim().is_empty() {
            return Err(PyValueError::new_err("source_version must not be empty"));
        }
        project_stem_cache_dir(&cache_dir).map_err(PyValueError::new_err)?;
        let (work, producer) = {
            let engine = slf.try_borrow()?;
            if prepared_stems_work::has_pending(&engine, source_ticket)? {
                return prepared_stems_work::reconcile(&engine);
            }
            let producer = engine.control_producer()?.clone();
            let work = prepared_stems_work::PreparedStemsWork::capture(
                &engine,
                id,
                source_version,
                cache_dir,
                source_ticket,
            )?;
            (work, producer)
        };
        let prepared = py
            .detach(move || work.prepare())
            .map_err(PyValueError::new_err)?;
        prepared.finish(&*slf.try_borrow()?, source_ticket, &producer)
    }

    /// Capture musical intent and the exact original target without publishing its epochs.
    #[pyo3(signature = (binding, target_id, intent_json, *, target_binding=None))]
    pub fn capture_pad_copy(
        &self,
        binding: &InputRuntimePadBinding,
        target_id: usize,
        intent_json: &str,
        target_binding: Option<&InputRuntimePadBinding>,
    ) -> PyResult<CapturedPadCopy> {
        pad_copy::capture(self, binding, target_id, intent_json, target_binding)
    }

    /// Prepare on the existing two-worker lane without retaining an AudioEngine Python borrow.
    pub fn prepare_pad_copy(
        slf: Py<Self>,
        py: Python<'_>,
        captured: &CapturedPadCopy,
    ) -> PyResult<PadCopyTicket> {
        let state = {
            let engine = slf.try_borrow(py)?;
            pad_copy::prepare(&engine, captured)?
        };
        py.detach(move || pad_copy::prepare_detached(state))
            .map_err(PyRuntimeError::new_err)
    }

    pub fn publish_pad_copy(&self, ticket: &PadCopyTicket) -> PyResult<()> {
        pad_copy::publish(self, ticket)
    }

    pub fn reconcile_pad_copy(&self, ticket: &PadCopyTicket) -> PyResult<bool> {
        pad_copy::reconcile(self, ticket)
    }

    pub fn finish_pad_copy(&self, ticket: &PadCopyTicket) -> PyResult<()> {
        pad_copy::finish(self, ticket)
    }

    pub fn cancel_pad_copy(&self, captured: &CapturedPadCopy) -> PyResult<bool> {
        pad_copy::cancel(self, captured)
    }

    #[pyo3(signature = (source_bank, target_bank))]
    pub fn capture_bank_participants(
        &self,
        source_bank: Option<usize>,
        target_bank: usize,
    ) -> PyResult<BankParticipants> {
        pad_copy::bank::capture_participants(self, source_bank, target_bank)
    }

    /// Native-only bank admission; durable/controller exposure is a separate settlement gate.
    #[pyo3(name = "capture_bank_mutation", signature = (source_bank, target_bank, intents, *, participants=None))]
    pub fn capture_bank_mutation_with_participants(
        &self,
        source_bank: Option<usize>,
        target_bank: usize,
        intents: Vec<Option<String>>,
        participants: Option<&BankParticipants>,
    ) -> PyResult<CapturedBankMutation> {
        pad_copy::bank::capture_with_participants(
            self,
            source_bank,
            target_bank,
            intents,
            participants,
        )
    }
    pub fn prepare_bank_mutation(
        slf: Py<Self>,
        py: Python<'_>,
        captured: &CapturedBankMutation,
    ) -> PyResult<BankMutationTicket> {
        let state = {
            let engine = slf.try_borrow(py)?;
            pad_copy::bank::prepare(&engine, captured)?
        };
        py.detach(move || pad_copy::bank::prepare_detached(state))
            .map_err(PyRuntimeError::new_err)
    }
    pub fn publish_bank_mutation(&self, ticket: &BankMutationTicket) -> PyResult<()> {
        pad_copy::bank::publish(self, ticket)
    }
    pub fn validate_bank_mutation(
        &self,
        ticket: &BankMutationTicket,
        ack: &BankMutationAcknowledgement,
    ) -> PyResult<bool> {
        pad_copy::bank::validate(self, ticket, ack)
    }
    pub fn reconcile_bank_mutation(
        &self,
        ticket: &BankMutationTicket,
        ack: &BankMutationAcknowledgement,
    ) -> PyResult<bool> {
        pad_copy::bank::reconcile(self, ticket, ack)
    }
    pub fn finish_bank_mutation(
        &self,
        ticket: &BankMutationTicket,
        ack: &BankMutationAcknowledgement,
    ) -> PyResult<()> {
        pad_copy::bank::finish(self, ticket, ack)
    }
    pub fn cancel_bank_mutation(&self, captured: &CapturedBankMutation) -> PyResult<bool> {
        pad_copy::bank::cancel(self, captured)
    }

    // Internal integration lane. UI admission stays fenced until the whole Pair/Hold path closes.
    #[pyo3(name = "_capture_pad_pair", signature = (source_id, target_id, *, source_binding=None))]
    pub fn _capture_pad_pair_with_source_binding(
        &self,
        source_id: usize,
        target_id: usize,
        source_binding: Option<&InputRuntimePadBinding>,
    ) -> PyResult<CapturedPadPair> {
        pad_pair::control::capture_with_source_binding(self, source_id, target_id, source_binding)
    }

    pub fn _prepare_pad_pair(&self, captured: &CapturedPadPair) -> PyResult<PadPairTicket> {
        let channels = self
            .stream_handle
            .as_ref()
            .map_or(2, |handle| handle.output_channels);
        pad_pair::control::prepare(self, captured, self.output_sample_rate()?, channels)
    }

    pub fn _publish_pad_pair(&self, ticket: &PadPairTicket) -> PyResult<()> {
        pad_pair::control::publish(self, ticket)
    }

    pub fn _reconcile_pad_pair(&self, ticket: &PadPairTicket) -> PyResult<bool> {
        pad_pair::control::reconcile(self, ticket)
    }

    pub fn _validate_pad_pair_acknowledgement(
        &self,
        ticket: &PadPairTicket,
        ack: &PadPairAcknowledgement,
    ) -> PyResult<bool> {
        pad_pair::control::validate_acknowledgement(self, ticket, ack)
    }

    pub fn _finish_pad_pair(
        &self,
        ticket: &PadPairTicket,
        ack: &PadPairAcknowledgement,
    ) -> PyResult<()> {
        pad_pair::control::finish(self, ticket, ack)
    }

    pub fn _cancel_pad_pair(&self, captured: &CapturedPadPair) -> PyResult<bool> {
        pad_pair::control::cancel(self, captured)
    }

    pub fn capture_recovered_pad_copy(
        &self,
        target: &InputRuntimePadBinding,
    ) -> PyResult<RecoveredCopyAcceptance> {
        pad_copy::recovery::capture_recovered(self, target)
    }

    pub fn capture_recovered_bank(
        &self,
        participants: &BankParticipants,
        intents: Vec<Option<String>>,
    ) -> PyResult<RecoveredBankAcceptance> {
        pad_copy::bank::recovery::capture(self, participants, intents)
    }

    pub fn validate_recovered_bank(&self, acceptance: &RecoveredBankAcceptance) -> PyResult<bool> {
        pad_copy::bank::recovery::validate(self, acceptance)
    }

    pub fn retry_recovered_bank(&self, acceptance: &RecoveredBankAcceptance) -> PyResult<bool> {
        pad_copy::bank::recovery::retry(self, acceptance)
    }

    pub fn release_recovered_bank(&self, acceptance: &RecoveredBankAcceptance) -> PyResult<bool> {
        pad_copy::bank::recovery::release(self, acceptance)
    }

    pub fn validate_recovered_pad_copy(
        &self,
        acceptance: &RecoveredCopyAcceptance,
    ) -> PyResult<bool> {
        pad_copy::recovery::validate_recovered(self, acceptance)
    }

    pub fn release_recovered_pad_copy(
        &self,
        acceptance: &RecoveredCopyAcceptance,
    ) -> PyResult<bool> {
        pad_copy::recovery::release_recovered(self, acceptance)
    }

    pub fn capture_recovered_pad_pair(
        &self,
        source_id: usize,
        target_id: usize,
    ) -> PyResult<RecoveredPairAcceptance> {
        pad_pair::recovery::capture_recovered(self, source_id, target_id)
    }

    pub fn validate_recovered_pad_pair(
        &self,
        acceptance: &RecoveredPairAcceptance,
    ) -> PyResult<bool> {
        pad_pair::recovery::validate_recovered(self, acceptance)
    }

    pub fn release_recovered_pad_pair(
        &self,
        acceptance: &RecoveredPairAcceptance,
    ) -> PyResult<bool> {
        pad_pair::recovery::release_recovered(self, acceptance)
    }

    /// Resolve diagnostic feedback only to its original living content and transport intent.
    pub fn _resolve_pad_feedback(&self, message: &AudioMessage) -> PyResult<Option<usize>> {
        let AudioMessage::ContentFeedback {
            owner,
            cell,
            lifetime,
            transport_revision,
            ..
        } = message
        else {
            return Ok(None);
        };
        let ownership = &self.input_runtime_ownership;
        if *owner != ownership.owner_identity().diagnostic() {
            return Ok(None);
        }
        let Some(cell) = content_placement::ContentCell::from_index(*cell) else {
            return Ok(None);
        };
        let Some(key) =
            content_placement::ContentKey::new(ownership.owner_identity(), cell, *lifetime)
        else {
            return Ok(None);
        };
        match ownership.observe_control_key(key) {
            content_placement::PlacementObservation::Current(snapshot) => Ok((ownership
                .transport_intent_for_cell(cell)
                .load(Ordering::Acquire)
                == *transport_revision)
                .then_some(snapshot.slot())),
            content_placement::PlacementObservation::Pending => Err(PyRuntimeError::new_err(
                "content feedback placement pending",
            )),
            _ => Ok(None),
        }
    }

    pub fn _resolve_ordinary_transport_feedback(
        &self,
        message: &AudioMessage,
    ) -> PyResult<Option<OrdinaryTransportObservation>> {
        ordinary_transport::resolve(&self.input_runtime_ownership, message)
    }

    /// Read current Native truth through the same content/source/placement fences.
    pub fn _current_ordinary_transport_observation(
        &self,
        id: usize,
    ) -> PyResult<Option<OrdinaryTransportObservation>> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("Invalid sample ID"));
        }
        ordinary_transport::current_for_slot(&self.input_runtime_ownership, id)
    }

    #[pyo3(signature = (sample_id, physical_press_id, received_at_ns=None))]
    pub fn pause_waveform_hold(
        &self,
        sample_id: usize,
        physical_press_id: &Bound<'_, PyAny>,
        received_at_ns: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<HoldActionTicket> {
        if physical_press_id.is_instance_of::<PyBool>()
            || !physical_press_id.is_instance_of::<PyInt>()
        {
            return Err(PyTypeError::new_err(
                "physical press ID must be a nonzero u64 integer",
            ));
        }
        let press = physical_press_id.extract::<u64>().map_err(|_| {
            PyValueError::new_err("physical press ID must be a nonzero u64 integer")
        })?;
        if press == 0 {
            return Err(PyValueError::new_err(
                "physical press ID must be a nonzero u64 integer",
            ));
        }
        let now = self.input_clock.capture_ns();
        let timestamp =
            validated_input_timestamp(parse_input_timestamp(received_at_ns)?, now).unwrap_or(now);
        hold_action::control::pause_for_producer(
            self,
            self.control_producer()?,
            sample_id,
            press,
            timestamp,
        )
    }

    pub fn release_waveform_hold(&self, ticket: &HoldActionTicket) -> PyResult<bool> {
        hold_action::control::release_for_producer(self, self.control_producer()?, ticket)
    }

    /// Select whether a pad renders from the loaded full mix or all prepared stems.
    #[pyo3(signature = (id, mode, source_version = None))]
    pub fn set_stem_mix_mode(
        &mut self,
        id: usize,
        mode: &str,
        source_version: Option<String>,
    ) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }

        let mode = parse_stem_mix_mode(mode)
            .ok_or_else(|| PyValueError::new_err("stem mix mode must be full_mix or all_stems"))?;

        let source_version_hash = match mode {
            StemMixMode::FullMix => 0,
            StemMixMode::AllStems => {
                let source_version = source_version
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| {
                        PyValueError::new_err("source_version must not be empty for all_stems mode")
                    })?;
                source_version_hash(source_version)
            }
        };

        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        self.input_runtime_ownership
            .ensure_no_copy_reservation(id)?;
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::SetStemMixMode {
                id,
                mode,
                source_version_hash,
            })
            .map_err(|_| {
                PyRuntimeError::new_err("Failed to send SetStemMixMode - buffer may be full")
            })
    }

    /// Release paired component residency after full-mix transition, preserving disk selection.
    pub fn set_stem_pair_full_mix(&mut self, sample_id: usize) -> PyResult<()> {
        if sample_id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio stream is not initialized"))?;
        resident_relocation::set_stem_pair_full_mix_with_producer(self, sample_id, &handle.producer)
    }

    /// Select which prepared component stems are enabled for all-stems playback.
    pub fn set_stem_enabled_mask(
        &mut self,
        id: usize,
        enabled_stem_mask: u8,
        source_version: String,
    ) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }

        if enabled_stem_mask & !STEM_COMPONENT_MASK != 0 {
            return Err(PyValueError::new_err(
                "stem enabled mask contains unsupported stems",
            ));
        }

        if source_version.trim().is_empty() {
            return Err(PyValueError::new_err("source_version must not be empty"));
        }

        let source_version_hash = source_version_hash(&source_version);
        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        self.input_runtime_ownership
            .ensure_no_copy_reservation(id)?;
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::SetStemEnabledMask {
                id,
                enabled_stem_mask,
                source_version_hash,
            })
            .map_err(|_| {
                PyRuntimeError::new_err("Failed to send SetStemEnabledMask - buffer may be full")
            })
    }

    /// Poll for pending background loader events.
    ///
    /// Returns `None` when no events are available.
    pub fn poll_loader_events(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        prepared_stems_work::reconcile(self)?;
        loader_events::poll(self, py, || {})
    }
    /// Resolve only an original Native event owner retained by the Controller.
    pub fn _resolve_loader_event(
        &self,
        delivery: &LoaderEventDelivery,
    ) -> PyResult<Option<(usize, bool)>> {
        loader_events::resolve_delivery(self, delivery)
    }
    /// Poll for pending normalized input mapping events.
    ///
    /// Returns `None` when no input events are available.
    pub fn poll_input_events(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        let runtime = self
            .input_runtime
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let Some(event) = runtime.poll_event() else {
            return Ok(None);
        };

        let dict = PyDict::new(py);
        dict.set_item("source", "midi")?;
        dict.set_item("binding_key", event.binding_key)?;
        dict.set_item("value", event.value)?;
        dict.set_item("received_at_ns", event.received_at_ns)?;
        dict.set_item("dispatched", event.dispatched)?;
        dict.set_item("direct", event.direct)?;
        if let Some(trigger) = event.captured_trigger {
            dict.set_item("captured_trigger", trigger)?;
        }
        if let Some(pitch) = event.captured_pitch {
            dict.set_item("captured_pitch", pitch)?;
        }
        if let Some(reason) = event.pitch_rejection {
            dict.set_item("pitch_rejection", reason.as_str())?;
        }
        if let Some(revision) = event.selection_revision {
            dict.set_item("selection_revision", revision)?;
        }
        if let Some(action_key) = event.action_key {
            dict.set_item("action_key", action_key)?;
        }

        Ok(Some(dict.into_any().unbind()))
    }

    pub fn trigger_captured_input_pad(&self, trigger: &CapturedInputPadTrigger) -> PyResult<bool> {
        self.input_runtime
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?
            .trigger_captured_pad(trigger)
    }

    /// Publish the original immutable GUI/MIDI owner once. No Python payload enters RT.
    pub fn publish_pitch_action(&self, action: &CapturedPitchAction) -> PyResult<bool> {
        use pitch_actions::{PitchActionReject, PitchActionWork};
        if !action.belongs_to(&self.input_runtime_ownership) {
            return Err(PyValueError::new_err(
                "pitch action belongs to another audio engine",
            ));
        }
        if action.is_published() {
            if action.state() == "rejected" {
                return Err(PyValueError::new_err(
                    action.reason().unwrap_or("pitch action rejected"),
                ));
            }
            return Ok(true);
        }
        action
            .control_guard(None)
            .map_err(|reason| PyValueError::new_err(reason.as_str()))?;
        let format = self
            .stream_handle
            .as_ref()
            .map(|handle| (handle.output_sample_rate, handle.output_channels));
        #[cfg(test)]
        let format = format
            .or(self.test_output_format)
            .or_else(|| self.test_control_channels.as_ref().map(|_| (48_000, 1)));
        let (sample_rate, channels) =
            format.ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        let mut producer = self
            .control_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;
        // The producer lock serializes staging with duplicate publication checks.
        if action.is_published() {
            return if action.state() == "rejected" {
                Err(PyValueError::new_err(
                    action.reason().unwrap_or("pitch action rejected"),
                ))
            } else {
                Ok(true)
            };
        }
        if producer.slots() == 0 {
            if let Some(runtime) = &self.input_runtime {
                let _ = runtime.cancel_pitch_capture(action, PitchActionReject::Capacity);
            }
            return Err(PyRuntimeError::new_err("pitch action queue is full"));
        }
        let Some(custody) = self.pitch_work_capacity.claim() else {
            if let Some(runtime) = &self.input_runtime {
                let _ = runtime.cancel_pitch_capture(action, PitchActionReject::Capacity);
            }
            return Err(PyRuntimeError::new_err(
                "pitch action work capacity is full",
            ));
        };
        let work = match PitchActionWork::new(action.clone(), sample_rate, channels, custody) {
            Ok(work) => work,
            Err(error) => {
                if let Some(runtime) = &self.input_runtime {
                    let _ =
                        runtime.cancel_pitch_capture(action, PitchActionReject::PreparationFailed);
                }
                return Err(PyRuntimeError::new_err(error.to_string()));
            }
        };
        action
            .mark_published()
            .map_err(|reason| PyValueError::new_err(reason.as_str()))?;
        let publication = Arc::new(pitch_publication::PitchActionPublication::new(work));
        if producer
            .push(ControlMessage::PitchAction(publication))
            .is_err()
        {
            if let Some(runtime) = &self.input_runtime {
                let _ = runtime.cancel_pitch_capture(action, PitchActionReject::Capacity);
            }
            return Err(PyRuntimeError::new_err("pitch action queue is full"));
        }
        Ok(true)
    }

    /// Cancel only this action. Later requested choices and completed attacks remain owned.
    pub fn cancel_pitch_action(&self, action: &CapturedPitchAction) -> PyResult<bool> {
        self.input_runtime
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?
            .cancel_pitch_capture(action, pitch_actions::PitchActionReject::StaleContent)
            .map_err(|reason| PyValueError::new_err(reason.as_str()))
    }

    pub fn prepare_captured_input_trigger(
        &self,
        trigger: &CapturedInputPadTrigger,
    ) -> PyResult<resident_relocation::ResidentWindowTicket> {
        self.input_runtime
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?
            .prepare_captured_trigger(self, trigger)
    }

    pub fn trigger_prepared_input_pad(
        &self,
        trigger: &CapturedInputPadTrigger,
        window: &resident_relocation::ResidentWindowTicket,
    ) -> PyResult<bool> {
        self.input_runtime
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?
            .trigger_prepared_pad(self, trigger, window)
    }

    /// Trigger playback of a previously loaded sample.
    #[pyo3(name = "play_sample", signature = (id, volume, *, received_at_ns=None, expected_source_generation=None))]
    pub fn play_sample_with_source(
        &mut self,
        id: usize,
        volume: f32,
        received_at_ns: Option<&Bound<'_, PyAny>>,
        expected_source_generation: Option<u64>,
    ) -> PyResult<()> {
        let captured = self._capture_pad_start(
            id,
            0.0,
            None,
            volume,
            false,
            received_at_ns,
            expected_source_generation,
        )?;
        self.publish_pad_start_work(&captured, None, true)
            .map(|_| ())
    }
    /// Stop all active voices and play a sample as one audio-thread command.
    #[pyo3(name = "play_sample_exclusive", signature = (id, volume, *, received_at_ns=None, expected_source_generation=None))]
    pub fn play_sample_exclusive_with_source(
        &mut self,
        id: usize,
        volume: f32,
        received_at_ns: Option<&Bound<'_, PyAny>>,
        expected_source_generation: Option<u64>,
    ) -> PyResult<()> {
        let captured = self._capture_pad_start(
            id,
            0.0,
            None,
            volume,
            true,
            received_at_ns,
            expected_source_generation,
        )?;
        self.publish_pad_start_work(&captured, None, true)
            .map(|_| ())
    }
    /// Stop playback of all active voices.
    pub fn stop_all(&mut self) -> PyResult<()> {
        let mut producer_guard = self
            .control_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        input_runtime_binding::enqueue_stop_with_producer(
            &self.input_runtime_ownership,
            &mut producer_guard,
            None,
        )
        .then_some(())
        .ok_or_else(|| PyRuntimeError::new_err("Failed to send Stop - buffer may be full"))
    }

    /// Atomically revoke earlier starts and report whether this pad needs ordered STOP.
    /// Acknowledged source/window state and admitted ownership are preserved.
    pub fn cancel_pad_launches(&self, sample_id: usize) -> PyResult<bool> {
        input_runtime_binding::cancel_launches_with_producer(
            &self.input_runtime_ownership,
            self.stream_handle.as_ref().map(|handle| &handle.producer),
            Some(sample_id),
        )
        .map(|targets| !targets.is_empty())
    }

    /// Atomically revoke earlier starts and return all admitted targets needing STOP.
    pub fn cancel_all_launches(&self) -> PyResult<Vec<usize>> {
        input_runtime_binding::cancel_launches_with_producer(
            &self.input_runtime_ownership,
            self.stream_handle.as_ref().map(|handle| &handle.producer),
            None,
        )
    }

    /// Source-bound IDs admitted before UI/active feedback; at most NUM_SAMPLES scalars.
    pub fn admitted_launch_ids(&self) -> Vec<usize> {
        self.input_runtime_ownership.admitted_launch_ids()
    }

    /// Admit one controller-owned global launch against actual native current bindings.
    #[pyo3(signature = (entries, *, received_at_ns=None))]
    pub fn capture_global_start(
        &self,
        py: Python<'_>,
        entries: Vec<(
            Py<InputRuntimePadBinding>,
            f64,
            Option<f64>,
            Option<Py<resident_relocation::ResidentWindowTicket>>,
        )>,
        received_at_ns: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<CapturedGlobalStart> {
        let now = self.input_clock.capture_ns();
        let timestamp =
            validated_input_timestamp(parse_input_timestamp(received_at_ns)?, now).unwrap_or(now);
        let bindings: Vec<_> = entries
            .iter()
            .map(|(binding, _, _, _)| binding.borrow(py))
            .collect();
        let windows: Vec<_> = entries
            .iter()
            .map(|(_, _, _, window)| window.as_ref().map(|window| window.borrow(py)))
            .collect();
        let entries = bindings
            .iter()
            .zip(entries.iter())
            .zip(windows.iter())
            .map(|((binding, (_, start, end, _)), window)| {
                (&**binding, *start, *end, window.as_deref())
            })
            .collect();
        captured_global_start::capture(self, entries, timestamp)
    }

    /// Publish only the original retained group; None means genuine readiness is pending.
    #[pyo3(name = "publish_captured_global_start", signature = (captured, *, capture=None))]
    pub fn publish_captured_global_start_py(
        &self,
        captured: &CapturedGlobalStart,
        capture: Option<&MixCaptureTicket>,
    ) -> PyResult<Option<GlobalPlaybackBatchTicket>> {
        captured_global_start::publish(self, captured, capture)
    }

    #[pyo3(signature = (entries, *, received_at_ns=None))]
    pub fn start_global_playback_batch(
        &self,
        py: Python<'_>,
        entries: Vec<(Py<InputRuntimePadBinding>, f64, Option<f64>)>,
        received_at_ns: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<GlobalPlaybackBatchTicket> {
        let received_at_ns = validated_input_timestamp(
            parse_input_timestamp(received_at_ns)?,
            self.input_clock.capture_ns(),
        );
        let producer = self.control_producer()?;
        let bindings: Vec<_> = entries
            .iter()
            .map(|(binding, _, _)| binding.borrow(py))
            .collect();
        let entries = bindings
            .iter()
            .zip(entries.iter())
            .map(|(binding, (_, start, end))| (&**binding, *start, *end))
            .collect();
        global_playback_batch::enqueue(self, producer, entries, true, received_at_ns)
    }

    /// Stop only after every actual voice is covered by matching current native authority.
    #[pyo3(signature = (bindings, *, received_at_ns=None))]
    pub fn stop_global_playback_batch(
        &self,
        py: Python<'_>,
        bindings: Vec<Py<InputRuntimePadBinding>>,
        received_at_ns: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<GlobalPlaybackBatchTicket> {
        let received_at_ns = validated_input_timestamp(
            parse_input_timestamp(received_at_ns)?,
            self.input_clock.capture_ns(),
        );
        let producer = self.control_producer()?;
        let bindings: Vec<_> = bindings.iter().map(|binding| binding.borrow(py)).collect();
        let entries = bindings
            .iter()
            .map(|binding| (&**binding, 0.0, None))
            .collect();
        global_playback_batch::enqueue(self, producer, entries, false, received_at_ns)
    }

    /// Set the global volume multiplier.
    pub fn set_volume(&mut self, volume: f32) -> PyResult<()> {
        if !volume.is_finite() || !(VOLUME_MIN..=VOLUME_MAX).contains(&volume) {
            return Err(PyValueError::new_err("volume out of range"));
        }

        let mut producer_guard = self
            .parameter_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        push_parameter_message(
            &mut producer_guard,
            ControlParameterMessage::SetVolume(volume),
            "SetVolume",
        )
    }

    /// Set the global speed multiplier.
    pub fn set_speed(&mut self, speed: f64) -> PyResult<()> {
        if !speed.is_finite() || !(SPEED_MIN..=SPEED_MAX).contains(&speed) {
            return Err(PyValueError::new_err("speed out of range"));
        }

        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        let mut producer_guard = self
            .parameter_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        push_global_rate_message(
            &self.global_timing_revision,
            &self.input_runtime_ownership.global_timing_publishing,
            &self.input_runtime_ownership.global_rate_status,
            &mut producer_guard,
            Some(speed),
            None,
            "SetSpeed",
        )
    }

    pub fn set_bpm_lock(&mut self, enabled: bool) -> PyResult<()> {
        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        let mut producer_guard = self
            .control_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        push_global_timing_message(
            &self.global_timing_revision,
            &self.input_runtime_ownership.global_timing_publishing,
            &mut producer_guard,
            ControlMessage::SetBpmLock(enabled),
            "SetBpmLock",
        )
    }

    pub fn set_key_lock(&mut self, enabled: bool) -> PyResult<()> {
        resident_relocation::reconcile(self)?;
        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        for id in 0..NUM_SAMPLES {
            self.input_runtime_ownership
                .ensure_no_copy_reservation(id)?;
        }
        for id in 0..NUM_SAMPLES {
            self.admit_resident_key_lock(id, enabled)?;
        }
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        if producer_guard.is_full() {
            return Err(PyRuntimeError::new_err(
                "Failed to send SetKeyLock - buffer may be full",
            ));
        }
        for id in 0..NUM_SAMPLES {
            self.note_resident_key_lock_intent(id, enabled)?;
        }
        push_control_message(
            &mut producer_guard,
            ControlMessage::SetKeyLock(enabled),
            "SetKeyLock",
        )
    }

    /// Enqueue source-bound mode intent; only matching callback feedback confirms it.
    /// Zero denotes saved intent admitted before a current loaded source exists.
    #[pyo3(name = "set_pad_key_lock", signature = (id, enabled, *, expected_source_generation=None))]
    pub fn set_pad_key_lock_with_source(
        &mut self,
        id: usize,
        enabled: bool,
        expected_source_generation: Option<u64>,
    ) -> PyResult<u64> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        resident_relocation::reconcile(self)?;
        if expected_source_generation.is_none() {
            self.admit_resident_key_lock(id, enabled)?;
        }
        key_lock_feedback::enqueue_with_source(self, id, enabled, expected_source_generation)
    }

    /// Observe actual current-source mode/processing feedback, independently of enqueue.
    pub fn pad_key_lock_status(&self, py: Python<'_>, id: usize) -> PyResult<Option<Py<PyDict>>> {
        key_lock_feedback::status(self, py, id)
    }

    /// Observe the actual current-source mapped voices, independently of requested SYNC intent.
    pub fn pad_source_sync_status(
        &self,
        py: Python<'_>,
        id: usize,
    ) -> PyResult<Option<Py<PyDict>>> {
        key_lock_feedback::source_sync_status(self, py, id)
    }

    /// Observe queued global rate intent separately from the callback-confirmed effect.
    pub fn global_rate_status(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        use global_rate_status::RateState;
        let Some(status) = self.input_runtime_ownership.global_rate_status.read() else {
            return Ok(None);
        };
        let queued = status.request_revision != status.effect.revision;
        let state = if queued {
            "queued"
        } else {
            match status.effect.state {
                RateState::Preparing => "preparing",
                RateState::Applied => "applied",
                RateState::Rejected => "rejected",
            }
        };
        let dict = PyDict::new(py);
        dict.set_item("request_revision", status.request_revision)?;
        dict.set_item("effect_revision", status.effect.revision)?;
        dict.set_item("state", state)?;
        dict.set_item("requested_speed", status.requested.speed)?;
        dict.set_item("requested_master_period", status.requested.period)?;
        dict.set_item("effective_speed", status.effect.effective.speed)?;
        dict.set_item("effective_master_period", status.effect.effective.period)?;
        dict.set_item(
            "execution_frame",
            (!queued).then_some(status.effect.execution_frame).flatten(),
        )?;
        dict.set_item(
            "error",
            (!queued).then(|| status.effect.error.message()).flatten(),
        )?;
        Ok(Some(dict.unbind()))
    }

    pub fn set_master_bpm(&mut self, bpm: f64) -> PyResult<()> {
        if !bpm.is_finite() || bpm <= 0.0 || !(60.0 / bpm).is_finite() {
            return Err(PyValueError::new_err("bpm out of range"));
        }

        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        let output_rate = self.output_sample_rate()?;

        if !((60.0 / bpm) * f64::from(output_rate) * 4.0).is_finite() {
            return Err(PyValueError::new_err("bpm output period out of range"));
        }

        let mut producer_guard = self
            .parameter_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        push_global_rate_message(
            &self.global_timing_revision,
            &self.input_runtime_ownership.global_timing_publishing,
            &self.input_runtime_ownership.global_rate_status,
            &mut producer_guard,
            None,
            Some(60.0 / bpm),
            "SetMasterBpm",
        )
    }

    /// Publish binary64 output seconds per quarter, preserving the current beat epoch.
    pub fn set_master_period(&mut self, period_seconds: f64) -> PyResult<()> {
        if !period_seconds.is_finite() || period_seconds <= 0.0 {
            return Err(PyValueError::new_err("period_seconds out of range"));
        }

        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        let output_rate = self.output_sample_rate()?;

        if !(period_seconds * f64::from(output_rate) * 4.0).is_finite() {
            return Err(PyValueError::new_err("master output period out of range"));
        }

        let mut producer = self
            .parameter_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        push_global_rate_message(
            &self.global_timing_revision,
            &self.input_runtime_ownership.global_timing_publishing,
            &self.input_runtime_ownership.global_rate_status,
            &mut producer,
            None,
            Some(period_seconds),
            "SetMasterPeriod",
        )
    }

    /// Admit coupled speed and output period as one validated parameter message.
    pub fn set_speed_and_master_period(&mut self, speed: f64, period_seconds: f64) -> PyResult<()> {
        if !speed.is_finite() || !(SPEED_MIN..=SPEED_MAX).contains(&speed) {
            return Err(PyValueError::new_err("speed out of range"));
        }
        if !period_seconds.is_finite() || period_seconds <= 0.0 {
            return Err(PyValueError::new_err("period_seconds out of range"));
        }
        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        let output_rate = self.output_sample_rate()?;
        if !(period_seconds * f64::from(output_rate) * 4.0).is_finite() {
            return Err(PyValueError::new_err("master output period out of range"));
        }
        let mut producer = self
            .parameter_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;
        push_global_rate_message(
            &self.global_timing_revision,
            &self.input_runtime_ownership.global_timing_publishing,
            &self.input_runtime_ownership.global_rate_status,
            &mut producer,
            Some(speed),
            Some(period_seconds),
            "SetSpeedAndMasterPeriod",
        )
    }

    #[pyo3(name = "set_pad_bpm", signature = (id, bpm=None, *, expected_source_generation=None))]
    pub fn set_pad_bpm_with_source(
        &mut self,
        id: usize,
        bpm: Option<f64>,
        expected_source_generation: Option<u64>,
    ) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }

        if bpm
            .is_some_and(|value| !value.is_finite() || value <= 0.0 || !(60.0 / value).is_finite())
        {
            return Err(PyValueError::new_err("bpm out of range"));
        }

        let output_rate = self.output_sample_rate()?;

        if bpm.is_some_and(|value| !((60.0 / value) * f64::from(output_rate)).is_finite()) {
            return Err(PyValueError::new_err("bpm source period out of range"));
        }

        constant_timing::publish_legacy_bpm_with_source(
            self,
            self.parameter_producer()?,
            id,
            bpm,
            expected_source_generation,
        )
    }

    #[pyo3(name = "set_pad_timing_metadata", signature = (id, phase_anchor_s, *, expected_source_generation=None))]
    pub fn set_pad_timing_metadata_with_source(
        &mut self,
        id: usize,
        phase_anchor_s: f64,
        expected_source_generation: Option<u64>,
    ) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }

        if !phase_anchor_s.is_finite() {
            return Err(PyValueError::new_err("phase_anchor_s out of range"));
        }

        constant_timing::publish_legacy_origin_with_source(
            self,
            self.control_producer()?,
            id,
            phase_anchor_s,
            expected_source_generation,
        )
    }

    pub fn anchor_transport_phase_from_pad(&mut self, id: usize) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }

        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        self.input_runtime_ownership
            .ensure_no_copy_reservation(id)?;
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        push_global_timing_message(
            &self.global_timing_revision,
            &self.input_runtime_ownership.global_timing_publishing,
            &mut producer_guard,
            ControlMessage::AnchorTransportPhaseFromPad { id },
            "AnchorTransportPhaseFromPad",
        )
    }

    /// Arm the selected BPM-lock reference once per audio-stream session.
    pub fn bootstrap_transport_from_pad(&mut self, id: usize) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        self.input_runtime_ownership
            .ensure_no_copy_reservation(id)?;
        let mut producer = self
            .control_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;
        push_global_timing_message(
            &self.global_timing_revision,
            &self.input_runtime_ownership.global_timing_publishing,
            &mut producer,
            ControlMessage::BootstrapTransportFromPad { id },
            "BootstrapTransportFromPad",
        )
    }

    pub fn set_pad_gain(&mut self, id: usize, gain_db: f32) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }

        if !gain_db.is_finite() || !(PAD_GAIN_DB_MIN..=PAD_GAIN_DB_MAX).contains(&gain_db) {
            return Err(PyValueError::new_err("gain out of range"));
        }

        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        self.input_runtime_ownership
            .ensure_no_copy_reservation(id)?;
        let expected_epoch = self.input_runtime_ownership.copy_parameter_epoch(id);
        let placement = self
            .input_runtime_ownership
            .slot_snapshot(id)
            .ok_or_else(|| PyRuntimeError::new_err("content placement is pending"))?;
        let mut producer_guard = self
            .parameter_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        if producer_guard.slots() == 0 {
            return Err(PyRuntimeError::new_err(
                "Failed to send SetPadGain - buffer may be full",
            ));
        }
        let mut result = Ok(());
        self.input_runtime_ownership
            .commit_parameter_intent(
                placement,
                expected_epoch,
                || true,
                || {
                    result = push_parameter_message(
                        &mut producer_guard,
                        ControlParameterMessage::SetPadGainGuarded {
                            placement,
                            id,
                            gain_db,
                            expected_epoch,
                        },
                        "SetPadGain",
                    );
                },
            )
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        result
    }

    pub fn set_pad_eq(
        &mut self,
        id: usize,
        low_db: f32,
        mid_db: f32,
        high_db: f32,
    ) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }

        let all = [low_db, mid_db, high_db];
        if all
            .iter()
            .any(|v| !v.is_finite() || !(PAD_EQ_DB_MIN..=PAD_EQ_DB_MAX).contains(v))
        {
            return Err(PyValueError::new_err("eq gain out of range"));
        }

        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        self.input_runtime_ownership
            .ensure_no_copy_reservation(id)?;
        let expected_epoch = self.input_runtime_ownership.copy_parameter_epoch(id);
        let placement = self
            .input_runtime_ownership
            .slot_snapshot(id)
            .ok_or_else(|| PyRuntimeError::new_err("content placement is pending"))?;
        let mut producer_guard = self
            .parameter_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        if producer_guard.slots() == 0 {
            return Err(PyRuntimeError::new_err(
                "Failed to send SetPadEq - buffer may be full",
            ));
        }
        let mut result = Ok(());
        self.input_runtime_ownership
            .commit_parameter_intent(
                placement,
                expected_epoch,
                || true,
                || {
                    result = push_parameter_message(
                        &mut producer_guard,
                        ControlParameterMessage::SetPadEqGuarded {
                            placement,
                            id,
                            low_db,
                            mid_db,
                            high_db,
                            expected_epoch,
                        },
                        "SetPadEq",
                    );
                },
            )
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        result
    }

    #[pyo3(name = "set_pad_loop_region", signature = (id, start_s, end_s=None, *, expected_source_generation=None))]
    pub fn set_pad_loop_region_with_source(
        &mut self,
        id: usize,
        start_s: f64,
        end_s: Option<f64>,
        expected_source_generation: Option<u64>,
    ) -> PyResult<()> {
        self.publish_pad_loop_region_with_source(
            id,
            start_s,
            end_s,
            expected_source_generation,
            None,
        )
    }

    /// Seek an active or paused sample voice to a source position in seconds.
    #[pyo3(name = "seek_sample", signature = (id, position_s, *, expected_source_generation=None))]
    pub fn seek_sample_with_source(
        &mut self,
        id: usize,
        position_s: f64,
        expected_source_generation: Option<u64>,
    ) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }

        if !position_s.is_finite() || position_s < 0.0 {
            return Err(PyValueError::new_err("position_s out of range"));
        }

        resident_relocation::reconcile(self)?;
        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        input_runtime_binding::validate_expected_source_generation_under_request_lock(
            self,
            id,
            expected_source_generation,
        )?;
        let output_sample_rate = self.output_sample_rate()?;
        if let Some(sample) = self
            .sample_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?[id]
            .as_ref()
            && !resident_seek_available(sample, position_s, output_sample_rate)
        {
            return Err(PyValueError::new_err(
                "seek requires a matching prepared resident window",
            ));
        }

        let mut producer_guard = self
            .control_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        transport_intent_control::publish_under_request_lock(
            self,
            &mut producer_guard,
            id,
            crate::messages::TransportContentEffect::Seek {
                position_s,
                expected_source_generation,
            },
        )
        .map(|_| ())
    }

    pub fn set_trigger_quantization(&mut self, mode: &str) -> PyResult<()> {
        let mode = parse_trigger_quantization(mode).ok_or_else(|| {
            PyValueError::new_err(
                "trigger quantization mode must be immediate or one of 1/16, 1/32, 1/64",
            )
        })?;

        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::SetTriggerQuantization(mode))
            .map_err(|_| {
                PyRuntimeError::new_err(
                    "Failed to send SetTriggerQuantization - buffer may be full",
                )
            })
    }

    /// Stop playback of a previously triggered sample.
    pub fn stop_sample(&mut self, id: usize) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err(format!(
                "id out of range (expected 0..{}, got {id})",
                NUM_SAMPLES - 1
            )));
        }

        let mut producer_guard = self
            .control_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        input_runtime_binding::enqueue_stop_with_producer(
            &self.input_runtime_ownership,
            &mut producer_guard,
            Some(id),
        )
        .then_some(())
        .ok_or_else(|| PyRuntimeError::new_err("Failed to send StopSample - buffer may be full"))
    }

    /// Pause playback of a sample without resetting its position.
    ///
    /// If the sample is playing, it becomes silent but retains its current
    /// playback position. If the sample is not playing, this has no effect.
    pub fn pause_sample(&mut self, id: usize) -> PyResult<OrdinaryTransportAdmission> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err(format!(
                "id out of range (expected 0..{}, got {id})",
                NUM_SAMPLES - 1
            )));
        }

        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        self.input_runtime_ownership
            .ensure_no_copy_reservation(id)?;
        let mut producer_guard = self
            .control_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        transport_intent_control::publish_under_request_lock(
            self,
            &mut producer_guard,
            id,
            crate::messages::TransportContentEffect::Pause,
        )
    }

    /// Resume playback of a paused sample from its saved position.
    ///
    /// If the sample was paused, playback continues. If the sample was not
    /// paused, this has no effect.
    pub fn resume_sample(&mut self, id: usize) -> PyResult<OrdinaryTransportAdmission> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err(format!(
                "id out of range (expected 0..{}, got {id})",
                NUM_SAMPLES - 1
            )));
        }

        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        self.input_runtime_ownership
            .ensure_no_copy_reservation(id)?;
        let mut producer_guard = self
            .control_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        transport_intent_control::publish_under_request_lock(
            self,
            &mut producer_guard,
            id,
            crate::messages::TransportContentEffect::Resume,
        )
    }

    /// Unload a sample slot.
    pub fn unload_sample(&mut self, id: usize) -> PyResult<()> {
        cold_load::unload_for_producer(self, id, self.control_producer()?)
    }

    /// Send a ping message to the audio thread.
    pub fn ping(&mut self) -> PyResult<()> {
        let mut producer_guard = self
            .control_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::Ping())
            .map_err(|_| PyRuntimeError::new_err("Failed to send Ping - buffer may be full"))
    }

    /// Receive a message from the audio thread.
    pub fn receive_msg(&mut self) -> PyResult<Option<AudioMessage>> {
        let mut consumer_guard = self
            .audio_message_consumer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire consumer lock"))?;

        match consumer_guard.pop() {
            Ok(msg) => Ok(Some(msg)),
            Err(_) => Ok(None),
        }
    }

    /// Get the waveform data for a loaded sample slot.
    ///
    /// # Parameters
    ///
    /// - `sample_id`: Pad number/sample slot
    /// - `width_px`: The bucket size (number of horizontal plot pixels)
    /// - `start_s`: Start x value of plot (in seconds)
    /// - `end_s`: End x value of plot (in seconds)
    ///
    /// # Returns
    ///
    /// The waveform render data for the specified region and resolution.
    pub fn get_waveform_render_data(
        &self,
        py: Python,
        sample_id: usize,
        width_px: usize,
        start_s: f64,
        end_s: f64,
    ) -> WaveformResult {
        if sample_id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        if self.cold_loading[sample_id].load(Ordering::Acquire) != 0 {
            return Ok(None);
        }
        // Acquire data
        let sample_arc = {
            let cache = self
                .sample_cache
                .lock()
                .map_err(|_| PyRuntimeError::new_err("Lock fail"))?;
            cache.get(sample_id).and_then(|slot| slot.clone())
        };

        let Some(sample) = sample_arc else {
            return Ok(None);
        };

        // Retrieve sample rate
        let sample_rate = if let Some(view) = sample.residency.as_ref() {
            f64::from(view.source.sample_rate_hz)
        } else if let Some(handle) = self.stream_handle.as_ref() {
            f64::from(handle.output_sample_rate)
        } else {
            44_100.0
        };

        let channels = sample.channels;
        if channels == 0 {
            return Ok(None);
        }
        let total_frames = sample.frame_count();
        let region = waveform::source_frame_range(start_s, end_s, sample_rate, total_frames);
        if region.is_empty() {
            return Ok(None);
        }
        if region.start < sample.resident_start()
            || region.end > sample.resident_end()
            || region.len() > 4096
        {
            let Some(data) = self
                .waveform_requests
                .request(self, sample_id, &sample, region, width_px)
                .map_err(PyRuntimeError::new_err)?
            else {
                return Ok(None);
            };
            return Ok(Some((
                data.is_raw,
                data.xs.to_pyarray(py).to_owned().into(),
                data.y_min.to_pyarray(py).to_owned().into(),
                data.y_max
                    .as_ref()
                    .map(|ys| ys.to_pyarray(py).to_owned().into()),
            )));
        }
        let Some(data) = waveform::render_region(
            &sample.samples[(region.start - sample.resident_start()) * channels
                ..(region.end - sample.resident_start()) * channels],
            channels,
            sample_rate,
            region.start,
            width_px,
        ) else {
            return Ok(None);
        };
        Ok(Some((
            data.is_raw,
            data.xs.to_pyarray(py).to_owned().into(),
            data.y_min.to_pyarray(py).to_owned().into(),
            data.y_max.map(|ys| ys.to_pyarray(py).to_owned().into()),
        )))
    }

    /// Read waveform preparation readiness without seeking or changing timing.
    pub fn waveform_readiness(&self, sample_id: usize) -> PyResult<(String, Option<String>)> {
        if sample_id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        if self.cold_loading[sample_id].load(Ordering::Acquire) != 0 {
            return Ok(("pending".into(), None));
        }
        self.waveform_requests
            .status(self, sample_id)
            .map_err(PyRuntimeError::new_err)
    }

    /// Explicit retry of a failed view; cancels only that editor request.
    pub fn retry_waveform(&self, sample_id: usize) -> PyResult<()> {
        self.waveform_requests
            .cancel(self, sample_id)
            .map_err(PyRuntimeError::new_err)
    }

    /// Assignment freshness for editor caches, independent of accepted timing.
    pub fn waveform_source_identity(
        &self,
        sample_id: usize,
    ) -> PyResult<Option<(u64, String, usize, u32)>> {
        if sample_id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        if self.cold_loading[sample_id].load(Ordering::Acquire) != 0 {
            return Ok(None);
        }
        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        if self.cold_loading[sample_id].load(Ordering::Acquire) != 0 {
            return Ok(None);
        }
        let cache = self
            .sample_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
        let generations = self
            .loaded_source_generations
            .lock()
            .map_err(|_| PyRuntimeError::new_err("source generation lock poisoned"))?;
        let Some(sample) = cache[sample_id].as_ref() else {
            return Ok(None);
        };
        let (generation, rate) = generations[sample_id];
        let digest = sample
            .residency
            .as_ref()
            .map(|view| {
                view.source
                    .transform_sha256
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Some((generation, digest, sample.frame_count(), rate)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtrb::RingBuffer;

    #[test]
    fn explicit_analysis_preflight_rejects_between_512_mib_and_one_gib_without_advancing() {
        let frames = 24_000_000;
        let old_peak =
            analysis_pcm::default_analysis_peak(frames, 2, 44_100, 128, cold_jobs::PCM_LIMIT_BYTES)
                .unwrap();
        assert!(old_peak > constant_timing::MAX_PCM_BYTES);
        let mut request = 7;
        let epoch = AtomicU64::new(9);
        {
            let _advance = PadRequestAdvance::prepare(&mut request, &epoch).unwrap();
            let error = admit_analysis_geometry(frames, 2, 44_100, 128).unwrap_err();
            assert!(error.contains("analysis PCM limit exceeded"));
        }
        assert_eq!(request, 7);
        assert_eq!(epoch.load(Ordering::Acquire), 9);
        assert!(admit_analysis_geometry(44_100, 2, 44_100, 128).is_ok());
    }

    #[test]
    fn latest_source_proposal_error_is_delivered_after_old_control_changes() {
        Python::initialize();
        let engine = AudioEngine::new().unwrap();
        engine.pad_request_ids.lock().unwrap()[0] = 7;
        engine.prepared_source_epochs[0].store(9, Ordering::Release);
        let (_, old_request) = engine.resident_proposals.begin_source(0, 7, 9).unwrap();
        let (_, current_proposal) = engine.resident_proposals.begin_source(0, 7, 9).unwrap();
        engine.pad_request_ids.lock().unwrap()[0] = 8;
        engine.prepared_source_epochs[0].store(10, Ordering::Release);
        for event in [
            LoaderEvent::Error {
                id: 0,
                request_id: old_request,
                error: "obsolete source proposal".into(),
            },
            LoaderEvent::Started {
                id: 0,
                request_id: current_proposal,
            },
            LoaderEvent::Error {
                id: 0,
                request_id: current_proposal,
                error: "current source proposal became stale".into(),
            },
        ] {
            engine.loader_tx.send(event).unwrap();
        }
        Python::attach(|py| {
            let event = engine.poll_loader_events(py).unwrap().unwrap();
            let dict = event.bind(py).cast::<PyDict>().unwrap();
            assert_eq!(
                dict.get_item("type")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "error"
            );
            assert_eq!(
                dict.get_item("request_id")
                    .unwrap()
                    .unwrap()
                    .extract::<u64>()
                    .unwrap(),
                current_proposal
            );
            assert_eq!(
                dict.get_item("msg")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "current source proposal became stale"
            );
            assert!(engine.poll_loader_events(py).unwrap().is_none());
        });
        assert_eq!(engine.pad_request_ids.lock().unwrap()[0], 8);
        assert_eq!(engine.prepared_source_epochs[0].load(Ordering::Acquire), 10);
    }

    #[test]
    fn resident_seek_admission_keeps_complete_end_clamp_and_matches_native_rounding() {
        use crate::messages::ResidentContext;
        let full = SampleBuffer {
            channels: 1,
            samples: Arc::from(vec![0.0; 100]),
            residency: None,
        }
        .with_complete_source(48_000);
        let finite = full.window(12, 23, 1, ResidentContext::FiniteLoop).unwrap();
        assert!(resident_seek_available(&full, 1.0, 48_000));
        assert!(resident_seek_available(&finite, 11.75 / 48_000.0, 48_000));
        assert!(!resident_seek_available(&finite, 22.75 / 48_000.0, 48_000));
        assert!(resident_seek_available(&finite, 22.25 / 48_000.0, 48_000));
        assert!(!resident_seek_available(&finite, 11.25 / 48_000.0, 48_000));
    }

    #[test]
    fn finite_waveform_uses_absolute_source_offsets_and_analysis_rejects_before_request_mutation() {
        Python::initialize();
        Python::attach(|py| {
            let engine = AudioEngine::new().unwrap();
            let sample = SampleBuffer {
                channels: 1,
                samples: Arc::from((0..100).map(|n| n as f32).collect::<Vec<_>>()),
                residency: None,
            }
            .with_complete_source(48_000)
            .window(12, 23, 1, crate::messages::ResidentContext::FiniteLoop)
            .unwrap();
            engine.sample_cache.lock().unwrap()[0] = Some(sample);
            let (_, xs, ys, _) = engine
                .get_waveform_render_data(py, 0, 16, 12.0 / 48_000.0, 17.0 / 48_000.0)
                .unwrap()
                .unwrap();
            for n in 0..5 {
                assert_eq!(
                    xs.bind(py).get_item(n).unwrap().extract::<f64>().unwrap(),
                    (12 + n) as f64 / 48_000.0
                );
                assert_eq!(
                    ys.bind(py).get_item(n).unwrap().extract::<f32>().unwrap(),
                    (12 + n) as f32
                );
            }
            assert!(
                engine
                    .get_waveform_render_data(py, 0, 16, 0.0, 1.0)
                    .is_err()
            );
            let request = engine.pad_request_ids.lock().unwrap()[0];
            let epoch = engine.prepared_source_epochs[0].load(Ordering::Acquire);
            assert!(admit_sample_analysis(&engine, 0).is_err());
            assert_eq!(engine.pad_request_ids.lock().unwrap()[0], request);
            assert_eq!(
                engine.prepared_source_epochs[0].load(Ordering::Acquire),
                epoch
            );
            assert!(engine.active_tasks.lock().unwrap().is_empty());
        });
    }

    #[test]
    fn load_success_preserves_request_identity_and_f64_activity_without_analysis() {
        Python::initialize();
        Python::attach(|py| {
            let engine = AudioEngine::new().unwrap();
            engine.pad_request_ids.lock().unwrap()[3] = 7;
            engine.loaded_source_generations.lock().unwrap()[3] = (7, 96_000);
            let detected_start = 1_000_003.0 / 96_000.0;
            assert_ne!(f64::from(detected_start as f32), detected_start);
            for suggestion in [Some(detected_start), None] {
                engine
                    .loader_tx
                    .send(LoaderEvent::Success {
                        original_lease: None,
                        timing_epoch: None,
                        id: 3,
                        request_id: 7,
                        duration_s: 20.0,
                        detected_loop_start_s: suggestion,
                        cached_path: "samples/new.wav".to_owned(),
                        analysis: None,
                        analysis_error: None,
                        incoming_defaults: None,
                    })
                    .unwrap();
                let event = engine.poll_loader_events(py).unwrap().unwrap();
                let event = event.bind(py).cast::<PyDict>().unwrap();
                assert_eq!(
                    event
                        .get_item("request_id")
                        .unwrap()
                        .unwrap()
                        .extract::<u64>()
                        .unwrap(),
                    7
                );
                assert_eq!(
                    event
                        .get_item("id")
                        .unwrap()
                        .unwrap()
                        .extract::<usize>()
                        .unwrap(),
                    3
                );
                let value = event.get_item("detected_loop_start_s").unwrap().unwrap();
                assert_eq!(value.extract::<Option<f64>>().unwrap(), suggestion);
                assert!(event.get_item("analysis").unwrap().is_none());
            }
        });
    }

    #[test]
    fn load_success_publishes_exact_long_loaded_duration_seconds() {
        Python::initialize();
        Python::attach(|py| {
            let engine = AudioEngine::new().unwrap();
            engine.pad_request_ids.lock().unwrap()[3] = 7;
            engine.loaded_source_generations.lock().unwrap()[3] = (7, 96_000);
            for rate in [44_100_u32, 48_000, 96_000] {
                for duration in [600_u32, 1_800] {
                    let frames = u64::from(rate) * u64::from(duration) + 7;
                    let duration_s = frames as f64 / f64::from(rate);
                    engine
                        .loader_tx
                        .send(LoaderEvent::Success {
                            original_lease: None,
                            timing_epoch: None,
                            id: 3,
                            request_id: 7,
                            duration_s,
                            detected_loop_start_s: None,
                            cached_path: "samples/long.wav".to_owned(),
                            analysis: None,
                            analysis_error: None,
                            incoming_defaults: None,
                        })
                        .unwrap();
                    let event = engine.poll_loader_events(py).unwrap().unwrap();
                    let event = event.bind(py).cast::<PyDict>().unwrap();
                    let published = event
                        .get_item("duration_s")
                        .unwrap()
                        .unwrap()
                        .extract::<f64>()
                        .unwrap();
                    assert_eq!(published, duration_s);
                    assert_eq!((published * f64::from(rate)).round() as u64, frames);
                }
            }
        });
    }

    #[test]
    fn successful_source_analysis_error_is_current_timing_bound() {
        Python::initialize();
        Python::attach(|py| {
            let engine = AudioEngine::new().unwrap();
            engine.pad_request_ids.lock().unwrap()[3] = 7;
            engine.loaded_source_generations.lock().unwrap()[3] = (7, 48_000);
            engine.prepared_source_epochs[3].store(9, Ordering::Release);
            for epoch in [9, 8] {
                engine
                    .loader_tx
                    .send(LoaderEvent::Success {
                        original_lease: None,
                        timing_epoch: Some(epoch),
                        id: 3,
                        request_id: 7,
                        duration_s: 2800.0,
                        detected_loop_start_s: None,
                        cached_path: "samples/long.wav".into(),
                        analysis: None,
                        analysis_error: Some("automatic analysis budget exceeded".into()),
                        incoming_defaults: None,
                    })
                    .unwrap();
                let event = engine.poll_loader_events(py).unwrap().unwrap();
                let event = event.bind(py).cast::<PyDict>().unwrap();
                assert_eq!(
                    event
                        .get_item("type")
                        .unwrap()
                        .unwrap()
                        .extract::<String>()
                        .unwrap(),
                    "success"
                );
                assert_eq!(
                    event.get_item("analysis_error").unwrap().is_some(),
                    epoch == 9
                );
                assert_eq!(
                    event.get_item("timing_stale").unwrap().is_some(),
                    epoch != 9
                );
            }
        });
    }

    #[test]
    fn waveform_python_boundary_returns_f64_time_and_f32_pcm() {
        Python::initialize();
        Python::attach(|py| {
            let engine = AudioEngine::new().unwrap();
            engine.sample_cache.lock().unwrap()[0] = Some(SampleBuffer {
                residency: None,
                channels: 1,
                samples: Arc::from([0.25_f32, -0.5, 0.75].as_slice()),
            });
            let (is_raw, xs, ys, maximum) = engine
                .get_waveform_render_data(py, 0, 16, 1.0 / 44_100.0, 2.0 / 44_100.0)
                .unwrap()
                .unwrap();
            assert!(is_raw);
            assert!(maximum.is_none());
            assert_eq!(xs.bind(py).getattr("dtype").unwrap().to_string(), "float64");
            assert_eq!(ys.bind(py).getattr("dtype").unwrap().to_string(), "float32");
            assert_eq!(
                xs.bind(py)
                    .getattr("size")
                    .unwrap()
                    .extract::<usize>()
                    .unwrap(),
                1
            );
            assert_eq!(
                xs.bind(py).get_item(0).unwrap().extract::<f64>().unwrap(),
                1.0 / 44_100.0
            );
            assert_eq!(
                ys.bind(py).get_item(0).unwrap().extract::<f32>().unwrap(),
                -0.5
            );
        });
    }

    #[test]
    fn analysis_resampling_keeps_late_music_after_a_silent_intro() {
        let source_rate = 96_000;
        let target_rate = 44_100;
        let input: Vec<f32> = (0..source_rate * 2)
            .map(|frame| {
                if frame < source_rate / 2 {
                    0.0
                } else {
                    (2.0 * std::f32::consts::PI * 440.0 * frame as f32 / source_rate as f32).sin()
                        * 0.5
                }
            })
            .collect();

        let output = resample_mono_to_target(input, source_rate, target_rate).unwrap();

        assert_eq!(output.len(), target_rate as usize * 2);
        assert!(
            output[..target_rate as usize / 4]
                .iter()
                .all(|v| v.abs() < 1e-5)
        );
        let last_second = &output[target_rate as usize..];
        let rms =
            (last_second.iter().map(|v| v * v).sum::<f32>() / last_second.len() as f32).sqrt();
        assert!(rms > 0.3, "late music was lost: RMS={rms}");
    }

    #[test]
    fn analysis_resampling_preserves_transient_times_and_flushes_the_tail() {
        use rubato::{Fft, FixedSync, Resampler};

        let source_rate = 48_000;
        let target_rate = 44_100;
        let chunk = Fft::<f32>::new(
            source_rate as usize,
            target_rate as usize,
            1024,
            1,
            1,
            FixedSync::Input,
        )
        .unwrap()
        .input_frames_next();

        // Short, exact, partial, and many-chunk inputs exercise the delayed
        // tail without assuming rubato's actual FFT chunk size is 1024.
        for input_len in [chunk - 1, chunk, chunk + 1, chunk * 2, chunk * 40 + 17] {
            let impulses = [(0, 0.5), (input_len / 2, -0.7), (input_len - 65, 0.9)];
            let mut input = vec![0.0; input_len];
            for (frame, amplitude) in impulses {
                input[frame] = amplitude;
            }

            let output = resample_mono_to_target(input, source_rate, target_rate).unwrap();
            let ratio = target_rate as f64 / source_rate as f64;
            assert_eq!(output.len(), (input_len as f64 * ratio).ceil() as usize);

            for (source_frame, amplitude) in impulses {
                let expected_frame = (source_frame as f64 * ratio).round() as usize;
                let start = expected_frame.saturating_sub(2);
                let end = (expected_frame + 3).min(output.len());
                let (offset, peak) = output[start..end]
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.abs().total_cmp(&b.abs()))
                    .unwrap();
                assert_eq!(peak.is_sign_positive(), amplitude.is_sign_positive());
                assert!(
                    peak.abs() > amplitude.abs() * 0.5,
                    "transient lost for {input_len} frames at {source_frame}: peak={peak}"
                );
                assert!(
                    (start + offset).abs_diff(expected_frame) <= 1,
                    "transient shifted for {input_len} frames at {source_frame}"
                );
            }
        }
    }

    #[test]
    fn analysis_resampling_accepts_a_buffered_zero_output_tail_at_96khz() {
        let mut input = vec![0.0; 4703];
        input[0] = 0.75;
        input[4702] = -0.9375;
        let mut padded = input.clone();
        padded.resize(6144, 0.0);
        let reference = resample_mono_to_target(padded, 96_000, 44_100).unwrap();
        let output = resample_mono_to_target(input, 96_000, 44_100).unwrap();
        assert_eq!(output.len(), (4703_usize * 44_100).div_ceil(96_000));
        assert_eq!(output, reference[..output.len()]);
        assert!(output[0] > 0.1);
        assert!(output[output.len() - 1] < -0.1);
    }

    #[test]
    fn analysis_resampling_preserves_same_rate_and_empty_input() {
        let input = vec![0.0, 0.25, -0.75, 1.0];
        assert_eq!(
            resample_mono_to_target(input.clone(), 44_100, 44_100).unwrap(),
            input
        );
        assert!(
            resample_mono_to_target(Vec::new(), 48_000, 44_100)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn input_timestamp_accepts_zero_and_none_and_rejects_malformed_python_values() {
        Python::initialize();
        Python::attach(|py| {
            assert_eq!(parse_input_timestamp(None).unwrap(), None);
            let zero = PyInt::new(py, 0_u64).into_any();
            assert_eq!(parse_input_timestamp(Some(&zero)).unwrap(), Some(0));
            let maximum = PyInt::new(py, u64::MAX).into_any();
            assert_eq!(
                parse_input_timestamp(Some(&maximum)).unwrap(),
                Some(u64::MAX)
            );
            for expression in [c"True", c"False", c"1.5", c"'42'"] {
                let value = py.eval(expression, None, None).unwrap();
                let error = parse_input_timestamp(Some(&value)).unwrap_err();
                assert!(error.is_instance_of::<PyTypeError>(py));
            }
            for expression in [c"-1", c"18446744073709551616"] {
                let value = py.eval(expression, None, None).unwrap();
                let error = parse_input_timestamp(Some(&value)).unwrap_err();
                assert!(error.is_instance_of::<PyValueError>(py));
            }
        });
    }

    #[test]
    fn push_control_message_reports_full_queue() {
        Python::initialize();

        let (mut producer, _consumer) = RingBuffer::new(1);
        producer.push(ControlMessage::Ping()).unwrap();

        let error = push_control_message(&mut producer, ControlMessage::StopAll(), "StopAll")
            .expect_err("full command queue should fail");

        assert!(error.to_string().contains("Failed to send StopAll"));
    }

    #[test]
    fn push_parameter_message_reports_full_queue() {
        Python::initialize();

        let (mut producer, _consumer) = RingBuffer::new(1);
        producer
            .push(ControlParameterMessage::SetVolume(0.5))
            .unwrap();

        let error = push_parameter_message(
            &mut producer,
            ControlParameterMessage::SetSpeed(1.0),
            "SetSpeed",
        )
        .expect_err("full parameter queue should fail");

        assert!(error.to_string().contains("Failed to send SetSpeed"));
    }

    #[test]
    fn master_period_public_api_validates_before_requiring_a_stream() {
        Python::initialize();
        Python::attach(|py| {
            let mut engine = AudioEngine::new().unwrap();
            for invalid in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
                let error = engine.set_master_period(invalid).unwrap_err();
                assert!(error.is_instance_of::<PyValueError>(py));
            }
            let error = engine
                .set_master_period(0.224_504_832_555_644_9)
                .unwrap_err();
            assert!(error.is_instance_of::<PyRuntimeError>(py));
            assert!(error.to_string().contains("Audio engine not initialized"));
        });
    }

    #[test]
    fn pad_timing_intent_reads_declared_authority_without_adoption_or_mutation() {
        Python::initialize();
        Python::attach(|py| {
            let engine = AudioEngine::new().unwrap();
            assert_eq!(engine.pad_timing_intent(3).unwrap(), "legacy");
            for (intent, expected) in [
                (
                    analysis::tempo_acceptance::TimingIntent::Automatic,
                    "automatic",
                ),
                (analysis::tempo_acceptance::TimingIntent::Manual, "manual"),
                (analysis::tempo_acceptance::TimingIntent::Tap, "tap"),
                (analysis::tempo_acceptance::TimingIntent::Legacy, "legacy"),
            ] {
                engine.timing_intents.lock().unwrap()[3] = intent;
                let epoch_before = engine.prepared_source_epochs[3].load(Ordering::Acquire);
                assert_eq!(engine.pad_timing_intent(3).unwrap(), expected);
                assert!(engine.current_constant_timing(py, 3).unwrap().is_none());
                assert_eq!(engine.pad_timing_intent(3).unwrap(), expected);
                assert_eq!(
                    engine.prepared_source_epochs[3].load(Ordering::Acquire),
                    epoch_before
                );
            }
            let error = engine.pad_timing_intent(NUM_SAMPLES).unwrap_err();
            assert!(error.is_instance_of::<PyValueError>(py));
        });
    }

    #[test]
    fn master_period_publication_failure_does_not_replace_a_queued_parameter() {
        Python::initialize();
        let (mut producer, mut consumer) = RingBuffer::new(1);
        producer
            .push(ControlParameterMessage::SetMasterBpm(123.5))
            .unwrap();
        let error = push_parameter_message(
            &mut producer,
            ControlParameterMessage::SetMasterPeriod(0.224_504_832_555_644_9),
            "SetMasterPeriod",
        )
        .unwrap_err();
        assert!(error.to_string().contains("Failed to send SetMasterPeriod"));
        assert_eq!(
            consumer.pop().unwrap(),
            ControlParameterMessage::SetMasterBpm(123.5)
        );
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn coupled_speed_master_period_validates_both_before_admission() {
        Python::initialize();
        Python::attach(|py| {
            let mut engine = AudioEngine::new().unwrap();
            for speed in [f64::NAN, f64::INFINITY, SPEED_MIN - 0.01, SPEED_MAX + 0.01] {
                let error = engine.set_speed_and_master_period(speed, 0.5).unwrap_err();
                assert!(error.is_instance_of::<PyValueError>(py));
            }
            for period in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
                let error = engine
                    .set_speed_and_master_period(1.25, period)
                    .unwrap_err();
                assert!(error.is_instance_of::<PyValueError>(py));
            }
            let error = engine.set_speed_and_master_period(1.25, 0.5).unwrap_err();
            assert!(error.is_instance_of::<PyRuntimeError>(py));
        });
    }

    #[test]
    fn coupled_speed_master_period_full_ring_never_partially_enqueues() {
        Python::initialize();
        let (mut producer, mut consumer) = RingBuffer::new(1);
        producer
            .push(ControlParameterMessage::SetVolume(0.25))
            .unwrap();
        let batch = ControlParameterMessage::SetSpeedAndMasterPeriod {
            speed: 1.25,
            period_seconds: 0.224_504_832_555_644_9,
        };
        let error =
            push_parameter_message(&mut producer, batch, "SetSpeedAndMasterPeriod").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Failed to send SetSpeedAndMasterPeriod")
        );
        assert_eq!(
            consumer.pop().unwrap(),
            ControlParameterMessage::SetVolume(0.25)
        );
        assert!(consumer.pop().is_err());
        push_parameter_message(&mut producer, batch, "SetSpeedAndMasterPeriod").unwrap();
        assert_eq!(consumer.pop().unwrap(), batch);
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn publish_loaded_sample_rejects_full_queue_without_cache_insert() {
        let (mut producer, _consumer) = RingBuffer::new(1);
        producer.push(ControlMessage::Ping()).unwrap();
        let producer = Arc::new(Mutex::new(producer));
        let sample_cache = Arc::new(Mutex::new(vec![None; 1]));
        let sample = SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from([0.0_f32, 0.0].as_slice()),
        };

        let ownership = Arc::new(input_runtime_binding::InputRuntimeOwnership::tracked());
        let mut generation = (0, 0);
        let mut digest = None;
        let result = publish_loaded_sample(
            &producer,
            &sample_cache,
            0,
            sample.clone(),
            LoadedSourcePublication {
                initial_mix: None,
                placement: ownership.slot_snapshot(0).unwrap(),
                ownership: &ownership,
                generation: 1,
                rate: 48_000,
                generation_slot: &mut generation,
                digest_slot: &mut digest,
                digest: "a".repeat(64),
                cold: false,
                cold_epoch: None,
                cold_adoption: None,
                replace_assignment: false,
                loop_region: None,
                resident_cancelled: None,
                incoming_defaults: None,
                initial_pitch: cold_load::SourcePitchInitialization::FreshNeutral,
                intent: None,
            },
        );

        assert_eq!(
            result.expect_err("full command queue should reject publication"),
            "Failed to send LoadSample - buffer may be full"
        );
        assert!(sample_cache.lock().unwrap()[0].is_none());
        assert_eq!(generation, (0, 0));
        assert!(digest.is_none());
        assert!(!ownership.source_current(0, &sample, 48_000));
    }

    #[test]
    fn pad_request_ids_increment_and_invalidate_old_work() {
        let ids = Arc::new(Mutex::new(vec![0; 2]));
        let epoch = AtomicU64::new(1);

        let first = next_pad_request_id(&ids, 0, &epoch).expect("first request id");
        let second = next_pad_request_id(&ids, 0, &epoch).expect("second request id");

        assert_eq!(first, 1);
        assert_eq!(second, 2);
        assert_eq!(epoch.load(Ordering::Acquire), 3);
        assert!(!pad_request_matches(&ids, 0, first));
        assert!(pad_request_matches(&ids, 0, second));
        assert!(pad_request_matches(&ids, 1, 0));
    }

    #[test]
    fn pad_request_ids_do_not_wrap_to_zero() {
        let ids = Arc::new(Mutex::new(vec![u64::MAX]));
        let epoch = AtomicU64::new(7);

        assert!(next_pad_request_id(&ids, 0, &epoch).is_err());
        assert_eq!(ids.lock().unwrap()[0], u64::MAX);
        assert_eq!(epoch.load(Ordering::Acquire), 7);
        assert!(pad_request_matches(&ids, 0, u64::MAX));
    }

    #[test]
    fn prepared_epoch_exhaustion_preserves_shared_request_identity() {
        let ids = Arc::new(Mutex::new(vec![7]));
        let epoch = AtomicU64::new(u64::MAX);

        assert!(next_pad_request_id(&ids, 0, &epoch).is_err());
        assert_eq!(ids.lock().unwrap()[0], 7);
        assert_eq!(epoch.load(Ordering::Acquire), u64::MAX);
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.close_mix_capture_admission();
        if let Some(runtime) = self.input_runtime.as_ref() {
            runtime.set_enabled(false);
        }
        let _ = input_runtime_binding::cancel_launches_before_shutdown(
            &self.input_runtime_ownership,
            self.stream_handle.as_ref().map(|handle| &handle.producer),
        );
        self.cold_jobs.close_admission();
        self.cold_cancelled.store(true, Ordering::Release);
        self.waveform_requests.cancel_all();
        let _ = resident_relocation::cancel_all(self);
        self.cold_jobs.shutdown();
        // CPAL quiescence transfers the complete callback image to its existing
        // retirement owner before control's capture join owner can disappear.
        self.input_runtime = None;
        let _ = self.close_audio_stream_checked();
    }
}
