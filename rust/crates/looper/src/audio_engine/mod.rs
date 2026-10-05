use crate::audio_engine::audio_stream::{AudioStreamHandle, create_audio_stream, start_stream};
use crate::audio_engine::constants::{
    NUM_SAMPLES, PAD_EQ_DB_MAX, PAD_EQ_DB_MIN, PAD_GAIN_DB_MAX, PAD_GAIN_DB_MIN, SPEED_MAX,
    SPEED_MIN, VOLUME_MAX, VOLUME_MIN,
};
use crate::audio_engine::errors::SampleLoadError;
use crate::audio_engine::input_mapping::InputRuntime;
use crate::audio_engine::progress::{LoadProgressStage, ProgressReporter};
use crate::audio_engine::sample_loader::{
    SampleLoadProgress, SampleLoadSubtask, cache_audio_file_for_project,
    decode_audio_file_to_sample_buffer,
};
use crate::audio_engine::stem_cache::{
    prepare_stem_buffers_from_cache, project_stem_cache_dir, source_version_hash,
    write_deterministic_stem_artifacts,
};
use crate::audio_engine::timing::{InputClock, validated_input_timestamp};
use crate::audio_engine::transport::QuantizeGrid;
use flitzis_looper_analysis as analysis;

use crate::audio_engine::channels::map_channels;
use crate::messages::{
    AudioMessage, BackgroundTaskKind, ControlMessage, ControlParameterMessage, LoaderEvent,
    PadTimingMetadata, STEM_COMPONENT_MASK, SampleAnalysis, SampleBuffer, StemMixMode,
    TriggerQuantization, task_to_str,
};
use numpy::{PyArray1, ToPyArray};
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyInt};
use rtrb::Producer;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    mpsc::{Receiver, Sender, TryRecvError},
};
use std::thread;

mod audio_stream;
mod buffer_retirement;
mod channels;
mod constants;
mod dsp;
mod errors;
mod input_mapping;
mod key_lock_preparation;
mod mixer;
mod progress;
mod rubberband_backend;
mod sample_loader;
mod scheduler;
mod source_grid;
mod stem_cache;
mod stretch_processor;
mod timing;
mod transport;
mod voice_slot;

/// Tuple: (is_raw_mode, xs, y_min, y_max)
///
/// - `is_raw_mode` (bool): If True, draw a simple line using `xs` and `y_min`.
/// - `xs`: Time values (seconds).
/// - `y_min`: Min values (or raw samples if in raw mode).
/// - `y_max`: Max values (or None if in raw mode).
type WaveformResult = PyResult<
    Option<(
        bool,
        Py<PyArray1<f32>>,
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

fn next_pad_request_id(pad_request_ids: &Arc<Mutex<Vec<u64>>>, id: usize) -> Result<u64, String> {
    let mut guard = pad_request_ids
        .lock()
        .map_err(|_| "Failed to acquire pad request id lock".to_string())?;
    let Some(current) = guard.get_mut(id) else {
        return Err("id out of range".to_string());
    };
    let next = current.wrapping_add(1);
    *current = if next == 0 { 1 } else { next };
    Ok(*current)
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

fn publish_loaded_sample(
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    sample_cache: &Arc<Mutex<Vec<Option<SampleBuffer>>>>,
    id: usize,
    sample: SampleBuffer,
) -> Result<(), String> {
    let mut producer_guard = producer
        .lock()
        .map_err(|_| "Failed to acquire producer lock".to_string())?;

    producer_guard
        .push(ControlMessage::LoadSample {
            id,
            sample: sample.clone(),
        })
        .map_err(|_| "Failed to send LoadSample - buffer may be full".to_string())?;

    if let Ok(mut cache) = sample_cache.lock()
        && let Some(slot) = cache.get_mut(id)
    {
        *slot = Some(sample);
    }

    Ok(())
}

/// Return the next higher integer power of two from x (or x if already a power of two).
fn next_power_of_two(x: usize) -> usize {
    if x <= 1 {
        return 1;
    }
    if x.is_power_of_two() {
        return x;
    }
    let mut n = 1;
    while n < x {
        n <<= 1;
    }
    n
}

/// Resample mono f32 audio to a target sample rate using rubato.
///
/// Returns the original buffer unchanged if `src_rate` already equals `target_rate`.
fn resample_mono_to_target(
    mono: Vec<f32>,
    src_rate: u32,
    target_rate: u32,
) -> Result<Vec<f32>, String> {
    if src_rate == target_rate {
        return Ok(mono);
    }

    use audioadapter_buffers::owned::InterleavedOwned;
    use rubato::{Fft, FixedSync, Indexing, Resampler};

    let input_frames = mono.len();
    if input_frames == 0 {
        return Ok(mono);
    }

    let channels = 1; // mono

    // Create resampler.
    let mut resampler = Fft::<f32>::new(
        src_rate as usize,
        target_rate as usize,
        1024, // chunk_size
        1,    // sub_chunks
        channels,
        FixedSync::Input,
    )
    .map_err(|e| format!("failed to create resampler: {e}"))?;

    // Input buffer.
    let input_buffer = InterleavedOwned::new_from(mono, channels, input_frames)
        .map_err(|e| format!("failed to create input buffer adapter: {e}"))?;

    // Output buffer sizing.
    let expected_output_len = (resampler.resample_ratio() * input_frames as f64).ceil() as usize;
    let output_frames = resampler
        .output_delay()
        .saturating_add(resampler.output_frames_max())
        .saturating_add(expected_output_len);

    let mut output_buffer = InterleavedOwned::new_from(
        vec![0.0f32; output_frames * channels],
        channels,
        output_frames,
    )
    .map_err(|e| format!("failed to create output buffer adapter: {e}"))?;

    let mut indexing = Indexing {
        input_offset: 0,
        output_offset: 0,
        partial_len: None,
        active_channels_mask: None,
    };

    let mut frames_left = input_frames;
    let mut output_len: usize = 0;

    let next_nbr_input_frames = resampler.input_frames_next();
    while frames_left > next_nbr_input_frames {
        let (_, nbr_out) = resampler
            .process_into_buffer(&input_buffer, &mut output_buffer, Some(&indexing))
            .map_err(|e| format!("resampling failed: {e}"))?;
        output_len += nbr_out;
        frames_left -= next_nbr_input_frames;
    }

    // Process remaining frames.
    if frames_left > 0 {
        indexing.partial_len = Some(frames_left);
        let (_, nbr_out) = resampler
            .process_into_buffer(&input_buffer, &mut output_buffer, Some(&indexing))
            .map_err(|e| format!("resampling failed: {e}"))?;
        output_len += nbr_out;
    }

    // Extract the resampled data.
    let result_data = output_buffer.take_data();
    let mut result = result_data[..output_len * channels].to_vec();

    // Trim delay frames from the end.
    let trim_frames = resampler.output_delay();
    let trim_samples = trim_frames * channels;
    if trim_samples > 0 && result.len() > trim_samples {
        result.truncate(result.len() - trim_samples);
    }

    Ok(result)
}

/// Run the BPM detection pipeline on mono audio.
///
/// Returns `(bpm, beat_grid)` on success.
fn run_bpm_pipeline(
    mono_f64: Vec<f64>,
    sample_rate_hz: u32,
) -> Result<(f32, analysis::BeatGrid), String> {
    let config = analysis::AnalysisConfig::default();

    // Guard: need at least one full frame of audio
    let frame_length = next_power_of_two((sample_rate_hz as f64 / config.max_bin_hz) as usize);
    if mono_f64.len() < frame_length {
        return Ok((
            0.0,
            analysis::BeatGrid {
                beats: Vec::new(),
                downbeats: Vec::new(),
                bars: Vec::new(),
            },
        ));
    }

    // Step 1: Compute onset detection function
    let mut df = analysis::DetectionFunction::new(sample_rate_hz, &config);
    let odf = df.process(&mono_f64);

    if odf.is_empty() {
        return Err("BPM pipeline: no ODF values produced".to_string());
    }

    // Step 2: Estimate beat periods via Viterbi HMM
    let mut beat_period = Vec::new();
    let tracker = analysis::TempoTrackV2::new(sample_rate_hz as f64, config.step_secs);
    tracker.calculate_beat_period(&odf, &mut beat_period, config.input_tempo, false);

    // Step 3: Calculate beat positions via dynamic programming
    let mut beats_frames = Vec::new();
    tracker.calculate_beats(
        &odf,
        &beat_period,
        &mut beats_frames,
        config.alpha,
        config.tightness,
    );

    // Step 4: Calculate BPM from beat intervals
    let bpm = analysis::calculate_bpm(&beats_frames, config.step_secs);

    // Step 5: Downbeat detection
    let mut downbeat_indices = Vec::new();
    let mut bar_indices = Vec::new();
    if !beats_frames.is_empty() {
        let mut downbeat =
            analysis::DownBeat::new(sample_rate_hz as f64, 16, config.step_secs as usize);
        downbeat.find_downbeats(
            &mono_f64,
            mono_f64.len(),
            &beats_frames,
            &mut downbeat_indices,
        );
        bar_indices = downbeat_indices.clone();
    }

    // Convert beat positions from frames to seconds
    let frame_duration = config.step_secs;
    let beats: Vec<f32> = beats_frames
        .iter()
        .map(|f| (*f * frame_duration) as f32)
        .collect();
    let downbeats: Vec<f32> = downbeat_indices
        .iter()
        .filter_map(|idx| beats_frames.get(*idx))
        .map(|f| (*f * frame_duration) as f32)
        .collect();
    let bars: Vec<f32> = bar_indices
        .iter()
        .filter_map(|idx| beats_frames.get(*idx))
        .map(|f| (*f * frame_duration) as f32)
        .collect();

    Ok((
        bpm,
        analysis::BeatGrid {
            beats,
            downbeats,
            bars,
        },
    ))
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
            input_runtime: None,
            input_clock: InputClock::new(),
        })
    }

    /// Initialize and run the audio engine.
    pub fn run(&mut self) -> PyResult<()> {
        if self.stream_handle.is_some() {
            return Err(PyRuntimeError::new_err("AudioEngine already running"));
        }

        match create_audio_stream(self.input_clock) {
            Ok(handle) => {
                start_stream(&handle.stream).map_err(|e| {
                    PyRuntimeError::new_err(format!("Failed to start audio stream: {e}"))
                })?;
                self.input_runtime =
                    Some(InputRuntime::new(handle.producer.clone(), self.input_clock));
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
        dict.set_item("master_bpm", snapshot.master_bpm)?;
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
        self.input_runtime = None;
        self.stream_handle = None;
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

    pub fn set_input_runtime_state(
        &self,
        multi_loop: bool,
        loaded: Vec<bool>,
        loop_starts: Vec<f64>,
        loop_ends: Vec<Option<f64>>,
    ) -> PyResult<()> {
        let runtime = self
            .input_runtime
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        runtime
            .set_runtime_state(multi_loop, loaded, loop_starts, loop_ends)
            .map_err(PyValueError::new_err)
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
        let run_analysis = run_analysis.unwrap_or(true);

        {
            let mut set = loading_sample_ids
                .lock()
                .map_err(|_| PyRuntimeError::new_err("Failed to acquire loading ids lock"))?;
            if !set.insert(id) {
                return Err(PyValueError::new_err("sample is already loading"));
            }
        }

        let request_id =
            next_pad_request_id(&pad_request_ids, id).map_err(PyRuntimeError::new_err)?;

        {
            let mut cache = sample_cache
                .lock()
                .map_err(|_| PyRuntimeError::new_err("Failed to acquire sample cache lock"))?;
            if let Some(slot) = cache.get_mut(id) {
                *slot = None;
            }
        }

        thread::spawn(move || {
            let _loading_guard = PadLoadingGuard {
                id,
                loading_sample_ids,
            };

            let _ = loader_tx.send(LoaderEvent::Started { id, request_id });

            let mut progress = ProgressReporter::new(id, request_id, loader_tx.clone());

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
            let duration_s = frames as f32 / output_sample_rate as f32;

            if !pad_request_matches(&pad_request_ids, id, request_id) {
                return;
            }

            if let Err(error) = publish_loaded_sample(&producer, &sample_cache, id, sample) {
                let _ = loader_tx.send(LoaderEvent::Error {
                    id,
                    request_id,
                    error,
                });
                return;
            }

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
                cached_path,
                analysis,
            });
        });

        Ok(request_id)
    }

    /// Analyze a previously loaded sample on a background thread.
    pub fn analyze_sample_async(&self, id: usize) -> PyResult<u64> {
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

        {
            let loading = self
                .loading_sample_ids
                .lock()
                .map_err(|_| PyRuntimeError::new_err("Failed to acquire loading ids lock"))?;
            if loading.contains(&id) {
                return Err(PyValueError::new_err("sample is currently loading"));
            }
        }

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

        {
            let mut tasks = self
                .active_tasks
                .lock()
                .map_err(|_| PyRuntimeError::new_err("Failed to acquire active tasks lock"))?;
            if has_active_task_for_id(&tasks, id) {
                return Err(PyValueError::new_err("sample task already running"));
            }
            tasks.insert((id, BackgroundTaskKind::Analysis));
        }

        let loader_tx = self.loader_tx.clone();
        let output_sample_rate = handle.output_sample_rate;
        let active_tasks = self.active_tasks.clone();
        let pad_request_ids = self.pad_request_ids.clone();
        let request_id =
            current_pad_request_id(&pad_request_ids, id).map_err(PyRuntimeError::new_err)?;

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

    /// Schedule offline stem generation on a background thread.
    ///
    /// This API currently provides source-version-aware task gating and deterministic cache
    /// artifact writing. Neural inference is intentionally not implemented here yet.
    pub fn generate_stems_async(
        &self,
        id: usize,
        source_version: String,
        cache_dir: String,
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
        let output_sample_rate = handle.output_sample_rate;

        {
            let loading = self
                .loading_sample_ids
                .lock()
                .map_err(|_| PyRuntimeError::new_err("Failed to acquire loading ids lock"))?;
            if loading.contains(&id) {
                return Err(PyValueError::new_err("sample is currently loading"));
            }
        }

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

        {
            let mut tasks = self
                .active_tasks
                .lock()
                .map_err(|_| PyRuntimeError::new_err("Failed to acquire active tasks lock"))?;
            if has_active_task_for_id(&tasks, id) {
                return Err(PyValueError::new_err("sample task already running"));
            }
            tasks.insert((id, BackgroundTaskKind::StemGeneration));
        }

        let loader_tx = self.loader_tx.clone();
        let active_tasks = self.active_tasks.clone();
        let request_id =
            current_pad_request_id(&self.pad_request_ids, id).map_err(PyRuntimeError::new_err)?;

        thread::spawn(move || {
            let _task_guard = PadTaskGuard {
                id,
                task: BackgroundTaskKind::StemGeneration,
                active_tasks,
            };

            let _ = loader_tx.send(LoaderEvent::TaskStarted {
                id,
                request_id,
                task: BackgroundTaskKind::StemGeneration,
            });

            let _ = loader_tx.send(LoaderEvent::TaskProgress {
                id,
                request_id,
                task: BackgroundTaskKind::StemGeneration,
                percent: 0.0,
                stage: "Generating stems".to_string(),
            });

            let result =
                write_deterministic_stem_artifacts(&sample, output_sample_rate, &cache_dir, {
                    let loader_tx = loader_tx.clone();
                    move |percent, stage| {
                        let _ = loader_tx.send(LoaderEvent::TaskProgress {
                            id,
                            request_id,
                            task: BackgroundTaskKind::StemGeneration,
                            percent,
                            stage: stage.to_string(),
                        });
                    }
                });

            match result {
                Ok(()) => {
                    let _ = loader_tx.send(LoaderEvent::TaskSuccess {
                        id,
                        request_id,
                        task: BackgroundTaskKind::StemGeneration,
                        analysis: None,
                    });
                }
                Err(error) => {
                    let _ = loader_tx.send(LoaderEvent::TaskError {
                        id,
                        request_id,
                        task: BackgroundTaskKind::StemGeneration,
                        error,
                    });
                }
            }
        });

        Ok(())
    }

    /// Validate cached stem artifacts and publish prepared handles to the audio thread.
    pub fn publish_prepared_stems(
        &self,
        id: usize,
        source_version: String,
        cache_dir: String,
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

        let stems = prepare_stem_buffers_from_cache(
            &source_version,
            &sample,
            handle.output_sample_rate,
            &cache_dir,
        )
        .map_err(PyValueError::new_err)?;

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::PublishPreparedStems { id, stems })
            .map_err(|_| {
                PyRuntimeError::new_err("Failed to send PublishPreparedStems - buffer may be full")
            })
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

        let event = match loader_rx.try_recv() {
            Ok(event) => event,
            Err(TryRecvError::Empty) => return Ok(None),
            Err(TryRecvError::Disconnected) => return Ok(None),
        };

        let dict = PyDict::new(py);
        match event {
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
                cached_path,
                analysis,
            } => {
                dict.set_item("type", "success")?;
                dict.set_item("id", id)?;
                dict.set_item("request_id", request_id)?;
                dict.set_item("duration_s", duration_s)?;
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
    pub fn set_speed(&mut self, speed: f32) -> PyResult<()> {
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

    pub fn set_master_bpm(&mut self, bpm: f32) -> PyResult<()> {
        if !bpm.is_finite() || bpm <= 0.0 {
            return Err(PyValueError::new_err("bpm out of range"));
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
            ControlParameterMessage::SetMasterBpm(bpm),
            "SetMasterBpm",
        )
    }

    pub fn set_pad_bpm(&mut self, id: usize, bpm: Option<f32>) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }

        if bpm.is_some_and(|value| !value.is_finite() || value <= 0.0) {
            return Err(PyValueError::new_err("bpm out of range"));
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
            ControlParameterMessage::SetPadBpm { id, bpm },
            "SetPadBpm",
        )
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

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        push_control_message(
            &mut producer_guard,
            ControlMessage::SetPadTimingMetadata {
                id,
                metadata: PadTimingMetadata { phase_anchor_s },
            },
            "SetPadTimingMetadata",
        )
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
            ControlMessage::SetPadLoopRegion { id, start_s, end_s },
            "SetPadLoopRegion",
        )
    }

    /// Seek an active or paused sample voice to a source position in seconds.
    pub fn seek_sample(&mut self, id: usize, position_s: f32) -> PyResult<()> {
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

        let _ = next_pad_request_id(&self.pad_request_ids, id).map_err(PyRuntimeError::new_err)?;

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::UnloadSample { id })
            .map_err(|_| {
                PyRuntimeError::new_err("Failed to send UnloadSample - buffer may be full")
            })?;

        if let Ok(mut cache) = self.sample_cache.lock()
            && let Some(slot) = cache.get_mut(id)
        {
            *slot = None;
        }

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
        start_s: f32,
        end_s: f32,
    ) -> WaveformResult {
        // Acquire data
        let sample_arc = {
            let cache = self
                .sample_cache
                .lock()
                .map_err(|_| PyRuntimeError::new_err("Lock fail"))?;
            cache.get(sample_id).and_then(|slot| slot.clone())
        };

        if sample_arc.is_none() {
            return Ok(None);
        }
        let sample = sample_arc.unwrap();

        // Retrieve sample rate
        let sample_rate = if let Some(handle) = self.stream_handle.as_ref() {
            handle.output_sample_rate as f32
        } else {
            44_100.0
        };

        let channels = sample.channels;
        let total_frames = sample.samples.len() / channels;

        // Calculate index ranges

        // Map time (seconds) to indices
        let start_idx = (start_s * sample_rate).floor() as usize;
        let end_idx = (end_s * sample_rate).ceil() as usize;

        // Clamp to buffer bounds
        let start_idx = start_idx.clamp(0, total_frames);
        let end_idx = end_idx.clamp(0, total_frames).max(start_idx); // Ensure no negative range

        let range_len = end_idx - start_idx;
        if range_len == 0 {
            return Ok(None);
        }

        let raw_data = &sample.samples;

        // Mode selection: raw vs envelope

        // If we have fewer samples than 2x pixels, showing an aggregate
        // loses information. Show raw samples instead.
        if range_len < width_px * 2 {
            // === RAW MODE ===

            // Allocate vectors
            let mut xs = Vec::with_capacity(range_len);
            let mut ys = Vec::with_capacity(range_len);

            for i in 0..range_len {
                let frame_idx = start_idx + i;
                let sample_idx = frame_idx * channels;

                // Mixdown logic: (L+R)/2 for stereo, or just L for Mono
                let val = if channels == 2 {
                    (raw_data[sample_idx] + raw_data[sample_idx + 1]) * 0.5
                } else {
                    raw_data[sample_idx]
                };

                xs.push(frame_idx as f32 / sample_rate);
                ys.push(val);
            }

            // Convert to numpy arrays (direct write to Python heap)
            let xs_py = xs.to_pyarray(py).to_owned();
            let ys_py = ys.to_pyarray(py).to_owned();

            Ok(Some((true, xs_py.into(), ys_py.into(), None)))
        } else {
            // === ENVELOPE MODE (Aggregation) ===

            // We want exactly `width_px` data points
            let mut xs = Vec::with_capacity(width_px);
            let mut mins = Vec::with_capacity(width_px);
            let mut maxs = Vec::with_capacity(width_px);

            // How many frames fit into one pixel column?
            let frames_per_bucket = range_len as f32 / width_px as f32;

            for i in 0..width_px {
                // Calculate the slice for this bucket
                let bucket_start_rel = (i as f32 * frames_per_bucket) as usize;
                let bucket_end_rel = ((i + 1) as f32 * frames_per_bucket) as usize;

                let bucket_start = start_idx + bucket_start_rel;
                let bucket_end = (start_idx + bucket_end_rel).min(start_idx + range_len);

                if bucket_start >= bucket_end {
                    continue;
                }

                let mut min_v = f32::MAX;
                let mut max_v = f32::MIN;

                // Inner Loop: Scan the bucket
                // Optimizations:
                // 1. We step by `channels` to stay aligned.
                // 2. We mixdown stereo on the fly to find true peak.
                let mut ptr = bucket_start * channels;
                let end_ptr = bucket_end * channels;

                // Using a while loop with raw indexing is often easier for
                // stride logic than iterators in this specific math context
                while ptr < end_ptr {
                    let val = if channels == 2 {
                        (raw_data[ptr] + raw_data[ptr + 1]) * 0.5
                    } else {
                        raw_data[ptr]
                    };

                    if val < min_v {
                        min_v = val;
                    }
                    if val > max_v {
                        max_v = val;
                    }

                    ptr += channels;
                }

                // If min_v is still MAX, it means the loop didn't run (empty bucket)
                if min_v == f32::MAX {
                    min_v = 0.0;
                    max_v = 0.0;
                }

                // The X coordinate is the time at the START of the bucket
                let time = bucket_start as f32 / sample_rate;

                xs.push(time);
                mins.push(min_v);
                maxs.push(max_v);
            }

            let xs_py = xs.to_pyarray(py).to_owned();
            let mins_py = mins.to_pyarray(py).to_owned();
            let maxs_py = maxs.to_pyarray(py).to_owned();

            Ok(Some((
                false,
                xs_py.into(),
                mins_py.into(),
                Some(maxs_py.into()),
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtrb::RingBuffer;

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
    fn publish_loaded_sample_rejects_full_queue_without_cache_insert() {
        let (mut producer, _consumer) = RingBuffer::new(1);
        producer.push(ControlMessage::Ping()).unwrap();
        let producer = Arc::new(Mutex::new(producer));
        let sample_cache = Arc::new(Mutex::new(vec![None; 1]));
        let sample = SampleBuffer {
            channels: 1,
            samples: Arc::from([0.0_f32, 0.0].as_slice()),
        };

        let result = publish_loaded_sample(&producer, &sample_cache, 0, sample);

        assert_eq!(
            result.expect_err("full command queue should reject publication"),
            "Failed to send LoadSample - buffer may be full"
        );
        assert!(sample_cache.lock().unwrap()[0].is_none());
    }

    #[test]
    fn pad_request_ids_increment_and_invalidate_old_work() {
        let ids = Arc::new(Mutex::new(vec![0; 2]));

        let first = next_pad_request_id(&ids, 0).expect("first request id");
        let second = next_pad_request_id(&ids, 0).expect("second request id");

        assert_eq!(first, 1);
        assert_eq!(second, 2);
        assert!(!pad_request_matches(&ids, 0, first));
        assert!(pad_request_matches(&ids, 0, second));
        assert!(pad_request_matches(&ids, 1, 0));
    }

    #[test]
    fn pad_request_ids_do_not_wrap_to_zero() {
        let ids = Arc::new(Mutex::new(vec![u64::MAX]));

        let next = next_pad_request_id(&ids, 0).expect("wrapped request id");

        assert_eq!(next, 1);
        assert!(pad_request_matches(&ids, 0, 1));
    }
}
