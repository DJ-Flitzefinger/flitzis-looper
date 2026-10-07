use crate::audio_engine::audio_stream::{AudioStreamHandle, create_audio_stream, start_stream};
use crate::audio_engine::constants::{
    NUM_SAMPLES, PAD_EQ_DB_MAX, PAD_EQ_DB_MIN, PAD_GAIN_DB_MAX, PAD_GAIN_DB_MIN, SPEED_MAX,
    SPEED_MIN, VOLUME_MAX, VOLUME_MIN,
};
use crate::audio_engine::errors::SampleLoadError;
use crate::audio_engine::initial_loop_start::detect_initial_loop_start;
use crate::audio_engine::input_mapping::InputRuntime;
use crate::audio_engine::progress::{LoadProgressStage, ProgressReporter};
use crate::audio_engine::sample_loader::{
    SampleLoadProgress, SampleLoadSubtask, cache_audio_file_for_project,
    decode_audio_file_to_sample_buffer,
};
use crate::audio_engine::stem_cache::{
    prepare_stem_buffers_from_cache, project_stem_cache_dir, source_version_hash,
};
use crate::audio_engine::timing::{InputClock, validated_input_timestamp};
use crate::audio_engine::transport::QuantizeGrid;
use flitzis_looper_analysis as analysis;

use crate::audio_engine::channels::map_channels;
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
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{
    Arc, Mutex,
    mpsc::{Receiver, Sender, TryRecvError},
};
use std::thread;

mod analysis_jobs;
mod analysis_pcm;
mod analysis_predictions;
mod audio_stream;
pub use analysis_jobs::OfflineAnalysisJob;
mod buffer_retirement;
mod channels;
pub(crate) mod constant_timing;
mod constants;
pub use constant_timing::ConstantTimingTicket;
pub use constant_timing::SavedConstantTimingTicket;
mod dsp;
mod errors;
pub(crate) mod global_playback_batch;
pub use global_playback_batch::GlobalPlaybackBatchTicket;
mod initial_loop_start;
mod input_mapping;
pub(crate) mod input_runtime_binding;
pub use input_runtime_binding::InputRuntimePadBinding;
pub(crate) mod key_lock_preparation;
#[cfg(test)]
mod key_lock_source_preparation;
mod mixer;
mod native_history_permit;
mod prepared_native_history;
pub(crate) mod prepared_source;
mod productive_source_history;
mod progress;
pub use prepared_source::PreparedSourceTicket;
use prepared_source::{
    enqueue_current_prepared_stems, file_sha256, next_epoch, validate_prepared_ticket,
};
pub(crate) mod rubberband_backend;
mod sample_loader;
mod scalar_grid;
pub(crate) use scalar_grid::ScalarSourceGrid;
mod scheduler;
mod source_grid;
mod source_playback;
mod source_reader;
mod stem_cache;
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

struct PadLoadingGuard {
    id: usize,
    loading_sample_ids: Arc<Mutex<HashSet<usize>>>,
}

impl Drop for PadLoadingGuard {
    fn drop(&mut self) {
        if let Ok(mut set) = self.loading_sample_ids.lock() {
            set.remove(&self.id);
        }
    }
}

struct PadTaskGuard {
    id: usize,
    task: BackgroundTaskKind,
    active_tasks: Arc<Mutex<HashSet<(usize, BackgroundTaskKind)>>>,
}

impl Drop for PadTaskGuard {
    fn drop(&mut self) {
        if let Ok(mut set) = self.active_tasks.lock() {
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
fn admit_sample_analysis(engine: &AudioEngine, id: usize) -> PyResult<(SampleBuffer, u64, u32)> {
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
    if has_active_task_for_id(&tasks, id) {
        return Err(PyValueError::new_err("sample task already running"));
    }
    let cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    let sample = cache[id]
        .clone()
        .ok_or_else(|| PyValueError::new_err("sample is not loaded"))?;
    let generations = engine
        .loaded_source_generations
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source generation lock poisoned"))?;
    let (generation, rate) = generations[id];
    if generation == 0 || rate == 0 {
        return Err(PyValueError::new_err("loaded source identity unavailable"));
    }
    let advance = PadRequestAdvance::prepare(&mut requests[id], &engine.prepared_source_epochs[id])
        .map_err(PyRuntimeError::new_err)?;
    let request_id = advance.commit();
    tasks.insert((id, BackgroundTaskKind::Analysis));
    Ok((sample, request_id, rate))
}

struct LoadedSourcePublication<'a> {
    ownership: &'a input_runtime_binding::InputRuntimeOwnership,
    generation: u64,
    rate: u32,
    generation_slot: &'a mut (u64, u32),
    digest_slot: &'a mut Option<String>,
    digest: String,
}

fn publish_loaded_sample(
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    sample_cache: &Arc<Mutex<Vec<Option<SampleBuffer>>>>,
    id: usize,
    sample: SampleBuffer,
    publication: LoadedSourcePublication<'_>,
) -> Result<(), String> {
    let mut cache = sample_cache
        .lock()
        .map_err(|_| "sample cache lock poisoned")?;
    let slot = cache.get_mut(id).ok_or("id out of range")?;
    let mut producer_guard = producer
        .lock()
        .map_err(|_| "Failed to acquire producer lock".to_string())?;

    producer_guard
        .push(ControlMessage::LoadSample {
            id,
            sample: sample.clone(),
        })
        .map_err(|_| "Failed to send LoadSample - buffer may be full".to_string())?;

    *publication.generation_slot = (publication.generation, publication.rate);
    *publication.digest_slot = Some(publication.digest);
    *slot = Some(sample.clone());
    publication
        .ownership
        .publish_source(id, &sample, publication.rate, publication.generation);

    Ok(())
}

/// Resample mono f32 audio to a target sample rate using rubato.
///
/// Returns the original buffer unchanged if `src_rate` already equals `target_rate`.
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
    mono_f64: Vec<f64>,
    sample_rate_hz: u32,
) -> Result<(f32, analysis::BeatGrid), String> {
    analysis::analyze_bpm(
        &mono_f64,
        sample_rate_hz,
        &analysis::AnalysisConfig::default(),
    )
}

/// Run the key detection pipeline on mono f32 audio at 44100 Hz.
///
/// Returns the key string (e.g., "Am", "C", or "unknown" on failure).
fn run_key_detection(mono_44100: Vec<f32>) -> String {
    #[cfg(debug_assertions)]
    let _timer = std::time::Instant::now();

    match analysis::detect_key(&mono_44100, 44_100) {
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
/// Shared preprocessing (decode → mono → resample to 44100) runs once, then
/// the BPM pipeline and key detection pipeline execute concurrently via
/// `std::thread::scope`. Total analysis time is bounded by the slower pipeline.
fn analyze_sample(sample: &SampleBuffer, sample_rate_hz: u32) -> Result<SampleAnalysis, String> {
    if sample.samples.is_empty() || sample_rate_hz == 0 {
        return Err("analysis failed: empty sample or zero sample rate".to_string());
    }

    // Shared preprocessing: convert to mono.
    let mono = map_channels(sample.samples.to_vec(), sample.channels, 1)
        .map_err(|err| format!("analysis failed: {err}"))?;

    if mono.is_empty() {
        return Err("analysis failed: mono conversion produced empty buffer".to_string());
    }

    // Resample to 44100 Hz if needed (shared step before fork).
    let mono_44100 = resample_mono_to_target(mono, sample_rate_hz, 44_100)
        .map_err(|e| format!("analysis failed: {e}"))?;

    // Clone for parallel pipelines.
    let mono_44100_bpm = mono_44100.clone();
    let mono_44100_key = mono_44100;

    // Convert BPM copy to f64 (qm-dsp expects f64).
    let mono_f64: Vec<f64> = mono_44100_bpm.iter().map(|s| *s as f64).collect();

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
                bpm_result = Some(run_bpm_pipeline(mono_f64, 44_100));
            });

            // Key detection thread.
            s.spawn(|| {
                key_result = Some(run_key_detection(mono_44100_key));
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

/// AudioEngine provides audio output and non-realtime control through CPAL.
#[pyclass]
pub struct AudioEngine {
    stream_handle: Option<AudioStreamHandle>,
    is_playing: bool,
    loader_tx: Sender<LoaderEvent>,
    loader_rx: Mutex<Receiver<LoaderEvent>>,
    sample_cache: Arc<Mutex<Vec<Option<SampleBuffer>>>>,
    loading_sample_ids: Arc<Mutex<HashSet<usize>>>,
    active_tasks: Arc<Mutex<HashSet<(usize, BackgroundTaskKind)>>>,
    pad_request_ids: Arc<Mutex<Vec<u64>>>,
    loaded_source_generations: Arc<Mutex<Vec<(u64, u32)>>>,
    loaded_source_digests: Arc<Mutex<Vec<Option<String>>>>,
    prepared_source_epochs: Vec<Arc<AtomicU64>>,
    timing_intents: Mutex<Vec<analysis::tempo_acceptance::TimingIntent>>,
    current_timing_acknowledgements: Arc<constant_timing::CurrentTimingAcknowledgements>,
    current_constant_timing: Mutex<Vec<Vec<constant_timing::CurrentConstantTimingRecord>>>,
    input_runtime_ownership: Arc<input_runtime_binding::InputRuntimeOwnership>,
    constant_timing_busy: AtomicBool,
    offline_jobs: analysis_jobs::OfflineJobs,
    input_runtime: Option<InputRuntime>,
    input_clock: InputClock,
}

#[pymethods]
impl AudioEngine {
    /// Create a new AudioEngine instance with default audio device.
    #[new]
    pub fn new() -> PyResult<Self> {
        let (loader_tx, loader_rx) = std::sync::mpsc::channel();

        Ok(AudioEngine {
            stream_handle: None,
            is_playing: false,
            loader_tx,
            loader_rx: Mutex::new(loader_rx),
            sample_cache: Arc::new(Mutex::new(vec![None; NUM_SAMPLES])),
            loading_sample_ids: Arc::new(Mutex::new(HashSet::new())),
            active_tasks: Arc::new(Mutex::new(HashSet::new())),
            pad_request_ids: Arc::new(Mutex::new(vec![0; NUM_SAMPLES])),
            loaded_source_generations: Arc::new(Mutex::new(vec![(0, 0); NUM_SAMPLES])),
            loaded_source_digests: Arc::new(Mutex::new(vec![None; NUM_SAMPLES])),
            prepared_source_epochs: (0..NUM_SAMPLES)
                .map(|_| Arc::new(AtomicU64::new(1)))
                .collect(),
            timing_intents: Mutex::new(vec![
                analysis::tempo_acceptance::TimingIntent::Legacy;
                NUM_SAMPLES
            ]),
            current_timing_acknowledgements: Arc::default(),
            current_constant_timing: Mutex::new((0..NUM_SAMPLES).map(|_| Vec::new()).collect()),
            input_runtime_ownership: Arc::new(
                input_runtime_binding::InputRuntimeOwnership::tracked(),
            ),
            constant_timing_busy: AtomicBool::new(false),
            offline_jobs: analysis_jobs::OfflineJobs::default(),
            input_runtime: None,
            input_clock: InputClock::new(),
        })
    }

    /// Initialize and run the audio engine.
    pub fn run(&mut self) -> PyResult<()> {
        if self.stream_handle.is_some() {
            return Err(PyRuntimeError::new_err("AudioEngine already running"));
        }

        self.current_timing_acknowledgements.clear_all();
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
        ) {
            Ok(handle) => {
                start_stream(&handle.stream).map_err(|e| {
                    PyRuntimeError::new_err(format!("Failed to start audio stream: {e}"))
                })?;
                self.input_runtime = Some(InputRuntime::new_with_ownership(
                    handle.producer.clone(),
                    self.input_clock,
                    self.input_runtime_ownership.clone(),
                ));
                self.stream_handle = Some(handle);
                self.is_playing = true;
                Ok(())
            }
            Err(e) => Err(PyRuntimeError::new_err(format!(
                "Failed to create audio stream: {e}"
            ))),
        }
    }

    pub fn output_sample_rate(&self) -> PyResult<u32> {
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        Ok(handle.output_sample_rate)
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

        let frames = sample.samples.len() / sample.channels;
        Ok((handle.output_sample_rate, sample.channels, frames))
    }

    /// Shut down the audio engine.
    pub fn shut_down(&mut self) -> PyResult<()> {
        self.offline_jobs.cancel(None);
        self.input_runtime = None;
        self.stream_handle = None;
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

    #[pyo3(signature = (multi_loop, loaded, loop_starts, loop_ends, bindings=None))]
    pub fn set_input_runtime_state(
        &self,
        py: Python<'_>,
        multi_loop: bool,
        loaded: Vec<bool>,
        loop_starts: Vec<f64>,
        loop_ends: Vec<Option<f64>>,
        bindings: Option<Vec<Option<Py<InputRuntimePadBinding>>>>,
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
        runtime
            .set_runtime_state(
                multi_loop,
                loaded,
                loop_starts,
                loop_ends,
                borrowed.iter().map(|binding| binding.as_deref()).collect(),
            )
            .map_err(PyValueError::new_err)
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

    /// Load an audio file into a sample slot on a background thread.
    ///
    /// # Parameters
    /// * `id` - Sample slot identifier
    /// * `path` - Path to the audio file
    /// * `run_analysis` - Whether to run automatic analysis after loading (default: true)
    pub fn load_sample_async(
        &self,
        id: usize,
        path: String,
        run_analysis: Option<bool>,
    ) -> PyResult<u64> {
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

        let loader_tx = self.loader_tx.clone();
        let producer = handle.producer.clone();
        let output_channels = handle.output_channels;
        let output_sample_rate = handle.output_sample_rate;
        let sample_cache = self.sample_cache.clone();
        let loading_sample_ids = self.loading_sample_ids.clone();
        let pad_request_ids = self.pad_request_ids.clone();
        let loaded_source_generations = self.loaded_source_generations.clone();
        let loaded_source_digests = self.loaded_source_digests.clone();
        let prepared_epoch = self.prepared_source_epochs[id].clone();
        let input_runtime_ownership = self.input_runtime_ownership.clone();
        let run_analysis = run_analysis.unwrap_or(true);

        {
            let mut set = loading_sample_ids
                .lock()
                .map_err(|_| PyRuntimeError::new_err("Failed to acquire loading ids lock"))?;
            if !set.insert(id) {
                return Err(PyValueError::new_err("sample is already loading"));
            }
        }

        let loading_guard = PadLoadingGuard {
            id,
            loading_sample_ids: loading_sample_ids.clone(),
        };

        let request_id = {
            let mut requests = pad_request_ids
                .lock()
                .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
            let advance = PadRequestAdvance::prepare(&mut requests[id], &prepared_epoch)
                .map_err(PyRuntimeError::new_err)?;
            let input_authority = self.input_runtime_ownership.next_authority(id)?;
            let mut cache = sample_cache
                .lock()
                .map_err(|_| PyRuntimeError::new_err("Failed to acquire sample cache lock"))?;
            let mut digests = loaded_source_digests
                .lock()
                .map_err(|_| PyRuntimeError::new_err("source digest lock poisoned"))?;
            self.input_runtime_ownership.revoke_source(id);
            if let Some(slot) = cache.get_mut(id) {
                *slot = None;
            }
            digests[id] = None;
            self.input_runtime_ownership.revoke(id, input_authority);
            advance.commit()
        };
        self.offline_jobs.cancel(Some(id));

        thread::spawn(move || {
            let _loading_guard = loading_guard;

            let _ = loader_tx.send(LoaderEvent::Started { id, request_id });

            let mut progress = ProgressReporter::new(id, request_id, loader_tx.clone());

            let source_digest = match file_sha256(Path::new(&path)) {
                Ok(digest) => digest,
                Err(error) => {
                    let _ = loader_tx.send(LoaderEvent::Error {
                        id,
                        request_id,
                        error,
                    });
                    return;
                }
            };

            let sample = match decode_audio_file_to_sample_buffer(
                Path::new(&path),
                output_channels,
                output_sample_rate,
                |update: SampleLoadProgress| {
                    let stage = match update.subtask {
                        SampleLoadSubtask::Decoding => LoadProgressStage::Decoding,
                        SampleLoadSubtask::Resampling => LoadProgressStage::Resampling,
                        SampleLoadSubtask::ChannelMapping => LoadProgressStage::ChannelMapping,
                    };
                    let force = update.percent <= 0.0 || update.percent >= 1.0;
                    progress.emit(stage, update.percent, update.resampling_required, force);
                },
            ) {
                Ok(sample) => sample,
                Err(SampleLoadError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
                    let _ = loader_tx.send(LoaderEvent::Error {
                        id,
                        request_id,
                        error: format!("File not found: {path}"),
                    });
                    return;
                }
                Err(err) => {
                    let _ = loader_tx.send(LoaderEvent::Error {
                        id,
                        request_id,
                        error: err.to_string(),
                    });
                    return;
                }
            };

            let resampling_required = progress.resampling_required.unwrap_or(true);
            let detected_loop_start_s = detect_initial_loop_start(&sample, output_sample_rate);

            let cached_path = if path.starts_with("samples/") {
                // When restoring from cache (path already in samples directory), use original path without copying
                path.clone()
            } else {
                // When loading a new sample (from file dialog), copy it to samples directory
                match cache_audio_file_for_project(Path::new("samples"), Path::new(&path)) {
                    Ok(path) => path.to_string_lossy().to_string(),
                    Err(err) => {
                        let _ = loader_tx.send(LoaderEvent::Error {
                            id,
                            request_id,
                            error: format!("Failed to cache audio file: {err}"),
                        });
                        return;
                    }
                }
            };

            match file_sha256(Path::new(&cached_path)) {
                Ok(digest) if digest == source_digest => {}
                _ => {
                    let _ = loader_tx.send(LoaderEvent::Error {
                        id,
                        request_id,
                        error: "Source content changed during decoding/project copy".into(),
                    });
                    return;
                }
            }

            let analysis = if run_analysis {
                progress.emit(LoadProgressStage::Analyzing, 0.0, resampling_required, true);

                match analyze_sample(&sample, output_sample_rate) {
                    Ok(result) => {
                        progress.emit(LoadProgressStage::Analyzing, 1.0, resampling_required, true);
                        Some(result)
                    }
                    Err(err) => {
                        let _ = loader_tx.send(LoaderEvent::Error {
                            id,
                            request_id,
                            error: err,
                        });
                        return;
                    }
                }
            } else {
                None
            };

            progress.emit(
                LoadProgressStage::Publishing,
                0.0,
                resampling_required,
                true,
            );

            let frames = sample.samples.len() / sample.channels;
            let duration_s = frames as f64 / f64::from(output_sample_rate);

            let Ok(requests) = pad_request_ids.lock() else {
                return;
            };
            if requests.get(id) != Some(&request_id) {
                return;
            }

            let Ok(mut generations) = loaded_source_generations.lock() else {
                return;
            };
            let Ok(mut digests) = loaded_source_digests.lock() else {
                return;
            };
            if let Err(error) = publish_loaded_sample(
                &producer,
                &sample_cache,
                id,
                sample,
                LoadedSourcePublication {
                    ownership: &input_runtime_ownership,
                    generation: request_id,
                    rate: output_sample_rate,
                    generation_slot: &mut generations[id],
                    digest_slot: &mut digests[id],
                    digest: source_digest,
                },
            ) {
                let _ = loader_tx.send(LoaderEvent::Error {
                    id,
                    request_id,
                    error,
                });
                return;
            }
            drop(requests);

            progress.emit(
                LoadProgressStage::Publishing,
                1.0,
                resampling_required,
                true,
            );
            let _ = loader_tx.send(LoaderEvent::Success {
                id,
                request_id,
                duration_s,
                detected_loop_start_s,
                cached_path,
                analysis,
            });
        });

        Ok(request_id)
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
            id,
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
        let (generation, loaded_rate) = self
            .loaded_source_generations
            .lock()
            .map_err(|_| PyRuntimeError::new_err("source generation lock poisoned"))?[id];
        self.offline_jobs
            .begin(
                id,
                sample,
                loaded_rate,
                generation,
                analysis_jobs::OfflineRequestOwner {
                    request_ids: self.pad_request_ids.clone(),
                    prepared_epoch: self.prepared_source_epochs[id].clone(),
                },
                self.loader_tx.clone(),
            )
            .map_err(PyRuntimeError::new_err)
    }

    /// Declare automatic/manual/TAP/legacy timing authority under the native owner.
    pub fn set_pad_timing_intent(&self, sample_id: usize, intent: &str) -> PyResult<()> {
        if sample_id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        let intent = constant_timing::parse_intent(intent)?;
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        constant_timing::set_intent(self, &handle.producer, sample_id, intent)
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
        if self.stream_handle.is_none() {
            return Ok(None);
        }
        constant_timing::current_metadata(self, py, sample_id)
    }

    /// Export only complete evidence for current acknowledged, content-verified timing.
    pub fn export_current_constant_timing(
        &self,
        py: Python<'_>,
        sample_id: usize,
        source_path: String,
    ) -> PyResult<Option<String>> {
        if self.stream_handle.is_none() {
            return Ok(None);
        }
        py.detach(|| constant_timing::export_current(self, sample_id, source_path))
            .map_err(PyValueError::new_err)
    }

    /// Capture fresh source/request/authority before background restore starts.
    pub fn capture_saved_constant_timing(
        &self,
        sample_id: usize,
        record_json: &str,
        source_path: String,
    ) -> PyResult<SavedConstantTimingTicket> {
        self.stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        constant_timing::capture_saved(self, sample_id, record_json, source_path)
            .map_err(PyValueError::new_err)
    }

    /// Verify actual source/mono/analyzer evidence and enqueue fresh guarded adoption.
    pub fn restore_constant_timing(
        &self,
        py: Python<'_>,
        ticket: &SavedConstantTimingTicket,
    ) -> PyResult<ConstantTimingTicket> {
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        py.detach(|| constant_timing::restore_saved(self, &handle.producer, ticket))
    }

    /// Explicitly run complete native QM evidence on the actual current loaded source.
    pub fn prepare_constant_timing(
        &self,
        py: Python<'_>,
        sample_id: usize,
        timing_error_halfwidth_seconds: f64,
        timing_error_provenance: String,
    ) -> PyResult<ConstantTimingTicket> {
        self.stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        py.detach(|| {
            constant_timing::prepare(
                self,
                sample_id,
                analysis::tempo_evidence::TimingBound {
                    halfwidth_seconds: timing_error_halfwidth_seconds,
                    provenance: timing_error_provenance,
                },
            )
        })
        .map_err(PyRuntimeError::new_err)
    }

    /// Construct accepted timing from explicit independent assertions and publish natively.
    // Keep explicit evidence assertions distinct at the public control boundary.
    #[allow(clippy::too_many_arguments)]
    pub fn publish_constant_timing(
        &self,
        py: Python<'_>,
        ticket: &ConstantTimingTicket,
        hypotheses_json: String,
        origin_seconds: f64,
        origin_provenance: String,
        acceptance_policy_version: String,
        acceptance_provenance: String,
    ) -> PyResult<()> {
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        py.detach(|| {
            constant_timing::publish(
                self,
                &handle.producer,
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
        })
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

        let (sample, request_id, output_sample_rate) = admit_sample_analysis(self, id)?;

        let loader_tx = self.loader_tx.clone();
        let active_tasks = self.active_tasks.clone();
        let pad_request_ids = self.pad_request_ids.clone();

        thread::spawn(move || {
            let _task_guard = PadTaskGuard {
                id,
                task: BackgroundTaskKind::Analysis,
                active_tasks,
            };

            let _ = loader_tx.send(LoaderEvent::TaskStarted {
                id,
                request_id,
                task: BackgroundTaskKind::Analysis,
            });

            let stage = LoadProgressStage::Analyzing.stage_label().to_string();
            let _ = loader_tx.send(LoaderEvent::TaskProgress {
                id,
                request_id,
                task: BackgroundTaskKind::Analysis,
                percent: 0.0,
                stage: stage.clone(),
            });

            let analysis = match analyze_sample(&sample, output_sample_rate) {
                Ok(result) => result,
                Err(error) => {
                    if !pad_request_matches(&pad_request_ids, id, request_id) {
                        return;
                    }
                    let _ = loader_tx.send(LoaderEvent::TaskError {
                        id,
                        request_id,
                        task: BackgroundTaskKind::Analysis,
                        error,
                    });
                    return;
                }
            };

            if !pad_request_matches(&pad_request_ids, id, request_id) {
                return;
            }

            let _ = loader_tx.send(LoaderEvent::TaskProgress {
                id,
                request_id,
                task: BackgroundTaskKind::Analysis,
                percent: 1.0,
                stage,
            });

            let _ = loader_tx.send(LoaderEvent::TaskSuccess {
                id,
                request_id,
                task: BackgroundTaskKind::Analysis,
                analysis: Some(analysis),
            });
        });

        Ok(request_id)
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

    /// Recheck the admission snapshot before and after off-thread artifact preparation.
    pub fn publish_prepared_stems(
        &self,
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

        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
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
        validate_prepared_ticket(
            source_ticket,
            id,
            &source_version,
            requests[id],
            &self.prepared_source_epochs[id],
            &sample,
        )
        .map_err(PyValueError::new_err)?;
        drop(requests);

        let mut stems = py
            .detach(|| {
                prepare_stem_buffers_from_cache(
                    &source_version,
                    &sample,
                    source_ticket.sample_rate_hz,
                    &cache_dir,
                )
            })
            .map_err(PyValueError::new_err)?;
        stems.publication = source_ticket.publication.clone();
        stems.accepted_timing = source_ticket.publication.accepted_projection();

        // Source mutation, timing publication and enqueue all serialize here.
        enqueue_current_prepared_stems(
            self,
            &handle.producer,
            source_ticket,
            &source_version,
            stems,
        )
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
        let loader_rx = self
            .loader_rx
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire loader receiver lock"))?;

        let requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        let (event, timing_stale) = loop {
            let mut event = match loader_rx.try_recv() {
                Ok(event) => event,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return Ok(None),
            };
            let (id, request_id) = match &event {
                LoaderEvent::Started { id, request_id }
                | LoaderEvent::Progress { id, request_id, .. }
                | LoaderEvent::Success { id, request_id, .. }
                | LoaderEvent::Error { id, request_id, .. }
                | LoaderEvent::TaskStarted { id, request_id, .. }
                | LoaderEvent::TaskProgress { id, request_id, .. }
                | LoaderEvent::TaskSuccess { id, request_id, .. }
                | LoaderEvent::TaskError { id, request_id, .. }
                | LoaderEvent::OfflineAnalysisCompleted { id, request_id, .. } => {
                    (*id, *request_id)
                }
            };
            let stale = requests.get(id) != Some(&request_id);
            if stale {
                match &mut event {
                    LoaderEvent::Success { analysis, .. } => {
                        let cache = self
                            .sample_cache
                            .lock()
                            .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
                        let generations = self.loaded_source_generations.lock().map_err(|_| {
                            PyRuntimeError::new_err("source generation lock poisoned")
                        })?;
                        if cache.get(id).is_none_or(Option::is_none)
                            || generations
                                .get(id)
                                .is_none_or(|current| current.0 != request_id)
                        {
                            continue;
                        }
                        *analysis = None;
                    }
                    LoaderEvent::TaskSuccess {
                        task: BackgroundTaskKind::Analysis,
                        analysis,
                        ..
                    } => *analysis = None,
                    LoaderEvent::TaskError {
                        task: BackgroundTaskKind::Analysis,
                        ..
                    } => {}
                    _ => continue,
                }
            }
            break (event, stale);
        };

        let dict = PyDict::new(py);
        if timing_stale {
            dict.set_item("timing_stale", true)?;
        }
        match event {
            LoaderEvent::OfflineAnalysisCompleted {
                id,
                request_id,
                result_json,
            } => {
                dict.set_item("type", "offline_analysis_completed")?;
                dict.set_item("id", id)?;
                dict.set_item("request_id", request_id)?;
                dict.set_item("result_json", result_json)?;
            }
            LoaderEvent::Started { id, request_id } => {
                dict.set_item("type", "started")?;
                dict.set_item("id", id)?;
                dict.set_item("request_id", request_id)?;
            }
            LoaderEvent::Progress {
                id,
                request_id,
                percent,
                stage,
            } => {
                dict.set_item("type", "progress")?;
                dict.set_item("id", id)?;
                dict.set_item("request_id", request_id)?;
                dict.set_item("percent", percent)?;
                dict.set_item("stage", stage)?;
            }
            LoaderEvent::Success {
                id,
                request_id,
                duration_s,
                detected_loop_start_s,
                cached_path,
                analysis,
            } => {
                dict.set_item("type", "success")?;
                dict.set_item("id", id)?;
                dict.set_item("request_id", request_id)?;
                dict.set_item("duration_s", duration_s)?;
                dict.set_item("detected_loop_start_s", detected_loop_start_s)?;
                dict.set_item("cached_path", cached_path)?;

                if let Some(analysis) = analysis {
                    let analysis_dict = PyDict::new(py);
                    analysis_dict.set_item("bpm", analysis.bpm)?;
                    analysis_dict.set_item("key", analysis.key)?;

                    let beat_grid_dict = PyDict::new(py);
                    beat_grid_dict.set_item("beats", &analysis.beat_grid.beats)?;
                    beat_grid_dict.set_item("downbeats", &analysis.beat_grid.downbeats)?;
                    beat_grid_dict.set_item("bars", &analysis.beat_grid.bars)?;
                    analysis_dict.set_item("beat_grid", beat_grid_dict)?;

                    dict.set_item("analysis", analysis_dict)?;
                }
            }
            LoaderEvent::Error {
                id,
                request_id,
                error,
            } => {
                dict.set_item("type", "error")?;
                dict.set_item("id", id)?;
                dict.set_item("request_id", request_id)?;
                dict.set_item("msg", error)?;
            }
            LoaderEvent::TaskStarted {
                id,
                request_id,
                task,
            } => {
                dict.set_item("type", "task_started")?;
                dict.set_item("id", id)?;
                dict.set_item("request_id", request_id)?;
                dict.set_item("task", task_to_str(task))?;
            }
            LoaderEvent::TaskProgress {
                id,
                request_id,
                task,
                percent,
                stage,
            } => {
                dict.set_item("type", "task_progress")?;
                dict.set_item("id", id)?;
                dict.set_item("request_id", request_id)?;
                dict.set_item("task", task_to_str(task))?;
                dict.set_item("percent", percent)?;
                dict.set_item("stage", stage)?;
            }
            LoaderEvent::TaskSuccess {
                id,
                request_id,
                task,
                analysis,
            } => {
                dict.set_item("type", "task_success")?;
                dict.set_item("id", id)?;
                dict.set_item("request_id", request_id)?;
                dict.set_item("task", task_to_str(task))?;

                if let Some(analysis) = analysis {
                    let analysis_dict = PyDict::new(py);
                    analysis_dict.set_item("bpm", analysis.bpm)?;
                    analysis_dict.set_item("key", analysis.key)?;

                    let beat_grid_dict = PyDict::new(py);
                    beat_grid_dict.set_item("beats", &analysis.beat_grid.beats)?;
                    beat_grid_dict.set_item("downbeats", &analysis.beat_grid.downbeats)?;
                    beat_grid_dict.set_item("bars", &analysis.beat_grid.bars)?;
                    analysis_dict.set_item("beat_grid", beat_grid_dict)?;

                    dict.set_item("analysis", analysis_dict)?;
                }
            }
            LoaderEvent::TaskError {
                id,
                request_id,
                task,
                error,
            } => {
                dict.set_item("type", "task_error")?;
                dict.set_item("id", id)?;
                dict.set_item("request_id", request_id)?;
                dict.set_item("task", task_to_str(task))?;
                dict.set_item("msg", error)?;
            }
        }

        Ok(Some(dict.into_any().unbind()))
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
        if let Some(action_key) = event.action_key {
            dict.set_item("action_key", action_key)?;
        }

        Ok(Some(dict.into_any().unbind()))
    }

    /// Trigger playback of a previously loaded sample.
    #[pyo3(signature = (id, volume, *, received_at_ns=None))]
    pub fn play_sample(
        &mut self,
        id: usize,
        volume: f32,
        received_at_ns: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        let received_at_ns = validated_input_timestamp(
            parse_input_timestamp(received_at_ns)?,
            self.input_clock.capture_ns(),
        );
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }

        if !volume.is_finite() || !(VOLUME_MIN..=VOLUME_MAX).contains(&volume) {
            return Err(PyValueError::new_err("volume out of range"));
        }

        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::PlaySample {
                id,
                volume,
                received_at_ns,
            })
            .map_err(|_| PyRuntimeError::new_err("Failed to send PlaySample - buffer may be full"))
    }

    /// Stop all active voices and play a sample as one audio-thread command.
    #[pyo3(signature = (id, volume, *, received_at_ns=None))]
    pub fn play_sample_exclusive(
        &mut self,
        id: usize,
        volume: f32,
        received_at_ns: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        let received_at_ns = validated_input_timestamp(
            parse_input_timestamp(received_at_ns)?,
            self.input_clock.capture_ns(),
        );
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }

        if !volume.is_finite() || !(VOLUME_MIN..=VOLUME_MAX).contains(&volume) {
            return Err(PyValueError::new_err("volume out of range"));
        }

        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::PlaySampleExclusive {
                id,
                volume,
                received_at_ns,
            })
            .map_err(|_| {
                PyRuntimeError::new_err("Failed to send PlaySampleExclusive - buffer may be full")
            })
    }

    /// Stop playback of all active voices.
    pub fn stop_all(&mut self) -> PyResult<()> {
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::StopAll())
            .map_err(|_| PyRuntimeError::new_err("Failed to send Stop - buffer may be full"))
    }

    /// Admit one controller-owned global launch against actual native current bindings.
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
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        let bindings: Vec<_> = entries
            .iter()
            .map(|(binding, _, _)| binding.borrow(py))
            .collect();
        let entries = bindings
            .iter()
            .zip(entries.iter())
            .map(|(binding, (_, start, end))| (&**binding, *start, *end))
            .collect();
        global_playback_batch::enqueue(self, &handle.producer, entries, true, received_at_ns)
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
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        let bindings: Vec<_> = bindings.iter().map(|binding| binding.borrow(py)).collect();
        let entries = bindings
            .iter()
            .map(|binding| (&**binding, 0.0, None))
            .collect();
        global_playback_batch::enqueue(self, &handle.producer, entries, false, received_at_ns)
    }

    /// Set the global volume multiplier.
    pub fn set_volume(&mut self, volume: f32) -> PyResult<()> {
        if !volume.is_finite() || !(VOLUME_MIN..=VOLUME_MAX).contains(&volume) {
            return Err(PyValueError::new_err("volume out of range"));
        }

        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .parameter_producer
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

        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .parameter_producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        push_parameter_message(
            &mut producer_guard,
            ControlParameterMessage::SetSpeed(speed),
            "SetSpeed",
        )
    }

    pub fn set_bpm_lock(&mut self, enabled: bool) -> PyResult<()> {
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        push_control_message(
            &mut producer_guard,
            ControlMessage::SetBpmLock(enabled),
            "SetBpmLock",
        )
    }

    pub fn set_key_lock(&mut self, enabled: bool) -> PyResult<()> {
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        push_control_message(
            &mut producer_guard,
            ControlMessage::SetKeyLock(enabled),
            "SetKeyLock",
        )
    }

    pub fn set_pad_key_lock(&mut self, id: usize, enabled: bool) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }

        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        push_control_message(
            &mut producer_guard,
            ControlMessage::SetPadKeyLock { id, enabled },
            "SetPadKeyLock",
        )
    }

    pub fn set_master_bpm(&mut self, bpm: f64) -> PyResult<()> {
        if !bpm.is_finite() || bpm <= 0.0 || !(60.0 / bpm).is_finite() {
            return Err(PyValueError::new_err("bpm out of range"));
        }

        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        if !((60.0 / bpm) * f64::from(handle.output_sample_rate) * 4.0).is_finite() {
            return Err(PyValueError::new_err("bpm output period out of range"));
        }

        let mut producer_guard = handle
            .parameter_producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        push_parameter_message(
            &mut producer_guard,
            ControlParameterMessage::SetMasterBpm(bpm),
            "SetMasterBpm",
        )
    }

    /// Publish binary64 output seconds per quarter, preserving the current beat epoch.
    pub fn set_master_period(&mut self, period_seconds: f64) -> PyResult<()> {
        if !period_seconds.is_finite() || period_seconds <= 0.0 {
            return Err(PyValueError::new_err("period_seconds out of range"));
        }

        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        if !(period_seconds * f64::from(handle.output_sample_rate) * 4.0).is_finite() {
            return Err(PyValueError::new_err("master output period out of range"));
        }

        let mut producer = handle
            .parameter_producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        push_parameter_message(
            &mut producer,
            ControlParameterMessage::SetMasterPeriod(period_seconds),
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
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        if !(period_seconds * f64::from(handle.output_sample_rate) * 4.0).is_finite() {
            return Err(PyValueError::new_err("master output period out of range"));
        }
        let mut producer = handle
            .parameter_producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;
        push_parameter_message(
            &mut producer,
            ControlParameterMessage::SetSpeedAndMasterPeriod {
                speed,
                period_seconds,
            },
            "SetSpeedAndMasterPeriod",
        )
    }

    pub fn set_pad_bpm(&mut self, id: usize, bpm: Option<f64>) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }

        if bpm
            .is_some_and(|value| !value.is_finite() || value <= 0.0 || !(60.0 / value).is_finite())
        {
            return Err(PyValueError::new_err("bpm out of range"));
        }

        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        if bpm.is_some_and(|value| {
            !((60.0 / value) * f64::from(handle.output_sample_rate)).is_finite()
        }) {
            return Err(PyValueError::new_err("bpm source period out of range"));
        }

        constant_timing::publish_legacy_bpm(self, &handle.parameter_producer, id, bpm)
    }

    pub fn set_pad_timing_metadata(&mut self, id: usize, phase_anchor_s: f64) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }

        if !phase_anchor_s.is_finite() {
            return Err(PyValueError::new_err("phase_anchor_s out of range"));
        }

        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        constant_timing::publish_legacy_origin(self, &handle.producer, id, phase_anchor_s)
    }

    pub fn anchor_transport_phase_from_pad(&mut self, id: usize) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }

        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        push_control_message(
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
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        let mut producer = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;
        push_control_message(
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

        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .parameter_producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        push_parameter_message(
            &mut producer_guard,
            ControlParameterMessage::SetPadGain { id, gain_db },
            "SetPadGain",
        )
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

        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .parameter_producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        push_parameter_message(
            &mut producer_guard,
            ControlParameterMessage::SetPadEq {
                id,
                low_db,
                mid_db,
                high_db,
            },
            "SetPadEq",
        )
    }

    pub fn set_pad_loop_region(
        &mut self,
        id: usize,
        start_s: f64,
        end_s: Option<f64>,
    ) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }

        if !start_s.is_finite() || start_s < 0.0 {
            return Err(PyValueError::new_err("start_s out of range"));
        }

        if end_s.is_some_and(|end_s| !end_s.is_finite() || end_s < 0.0) {
            return Err(PyValueError::new_err("end_s out of range"));
        }

        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;

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
                "Failed to send SetPadLoopRegion - buffer may be full",
            ));
        }
        let input_authority = self.input_runtime_ownership.next_authority(id)?;
        self.input_runtime_ownership.revoke(id, input_authority);
        producer_guard
            .push(ControlMessage::SetPadLoopRegion { id, start_s, end_s })
            .map_err(|_| PyRuntimeError::new_err("reserved single-producer capacity lost"))
    }

    /// Seek an active or paused sample voice to a source position in seconds.
    pub fn seek_sample(&mut self, id: usize, position_s: f64) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }

        if !position_s.is_finite() || position_s < 0.0 {
            return Err(PyValueError::new_err("position_s out of range"));
        }

        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::SeekSample { id, position_s })
            .map_err(|_| PyRuntimeError::new_err("Failed to send SeekSample - buffer may be full"))
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

        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::StopSample { id })
            .map_err(|_| PyRuntimeError::new_err("Failed to send StopSample - buffer may be full"))
    }

    /// Pause playback of a sample without resetting its position.
    ///
    /// If the sample is playing, it becomes silent but retains its current
    /// playback position. If the sample is not playing, this has no effect.
    pub fn pause_sample(&mut self, id: usize) -> PyResult<()> {
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

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::PauseSample { id })
            .map_err(|_| PyRuntimeError::new_err("Failed to send PauseSample - buffer may be full"))
    }

    /// Resume playback of a paused sample from its saved position.
    ///
    /// If the sample was paused, playback continues. If the sample was not
    /// paused, this has no effect.
    pub fn resume_sample(&mut self, id: usize) -> PyResult<()> {
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

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::ResumeSample { id })
            .map_err(|_| {
                PyRuntimeError::new_err("Failed to send ResumeSample - buffer may be full")
            })
    }

    /// Unload a sample slot.
    pub fn unload_sample(&mut self, id: usize) -> PyResult<()> {
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

        let mut requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        let advance =
            PadRequestAdvance::prepare(&mut requests[id], &self.prepared_source_epochs[id])
                .map_err(PyRuntimeError::new_err)?;
        let mut cache = self
            .sample_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
        let mut digests = self
            .loaded_source_digests
            .lock()
            .map_err(|_| PyRuntimeError::new_err("source digest lock poisoned"))?;

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        if producer_guard.is_full() {
            return Err(PyRuntimeError::new_err(
                "Failed to send UnloadSample - buffer may be full",
            ));
        }

        let input_authority = self.input_runtime_ownership.next_authority(id)?;
        self.input_runtime_ownership.revoke(id, input_authority);
        advance.commit();
        self.input_runtime_ownership.revoke_source(id);
        producer_guard
            .push(ControlMessage::UnloadSample { id })
            .map_err(|_| PyRuntimeError::new_err("reserved single-producer capacity lost"))?;
        cache[id] = None;
        digests[id] = None;
        drop(producer_guard);
        drop(digests);
        drop(cache);
        drop(requests);
        self.offline_jobs.cancel(Some(id));

        if let Ok(mut set) = self.loading_sample_ids.lock() {
            set.remove(&id);
        }

        if let Ok(mut set) = self.active_tasks.lock() {
            set.retain(|(task_id, _)| *task_id != id);
        }

        Ok(())
    }

    /// Send a ping message to the audio thread.
    pub fn ping(&mut self) -> PyResult<()> {
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::Ping())
            .map_err(|_| PyRuntimeError::new_err("Failed to send Ping - buffer may be full"))
    }

    /// Receive a message from the audio thread.
    pub fn receive_msg(&mut self) -> PyResult<Option<AudioMessage>> {
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut consumer_guard = handle
            .consumer
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
        let sample_rate = if let Some(handle) = self.stream_handle.as_ref() {
            f64::from(handle.output_sample_rate)
        } else {
            44_100.0
        };

        let channels = sample.channels;
        if channels == 0 {
            return Ok(None);
        }
        let total_frames = sample.samples.len() / channels;
        let region = waveform::source_frame_range(start_s, end_s, sample_rate, total_frames);
        if region.is_empty() {
            return Ok(None);
        }
        let Some(data) = waveform::render_region(
            &sample.samples[region.start * channels..region.end * channels],
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtrb::RingBuffer;

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
                        id: 3,
                        request_id: 7,
                        duration_s: 20.0,
                        detected_loop_start_s: suggestion,
                        cached_path: "samples/new.wav".to_owned(),
                        analysis: None,
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
                            id: 3,
                            request_id: 7,
                            duration_s,
                            detected_loop_start_s: None,
                            cached_path: "samples/long.wav".to_owned(),
                            analysis: None,
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
    fn waveform_python_boundary_returns_f64_time_and_f32_pcm() {
        Python::initialize();
        Python::attach(|py| {
            let engine = AudioEngine::new().unwrap();
            engine.sample_cache.lock().unwrap()[0] = Some(SampleBuffer {
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
            channels: 1,
            samples: Arc::from([0.0_f32, 0.0].as_slice()),
        };

        let ownership = input_runtime_binding::InputRuntimeOwnership::tracked();
        let mut generation = (0, 0);
        let mut digest = None;
        let result = publish_loaded_sample(
            &producer,
            &sample_cache,
            0,
            sample.clone(),
            LoadedSourcePublication {
                ownership: &ownership,
                generation: 1,
                rate: 48_000,
                generation_slot: &mut generation,
                digest_slot: &mut digest,
                digest: "a".repeat(64),
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
