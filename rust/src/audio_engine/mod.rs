//! Audio Engine Module
//!
//! This module provides real-time audio mixing and playback capabilities.
//! It is organized into sub-modules, each with a specific responsibility:
//!
//! - [`audio_stream`]: CPAL audio stream management and real-time callback
//! - [`constants`]: Configuration constants and limits
//! - [`errors`]: Audio-specific error types
//! - [`voice`]: Voice management and lifecycle
//! - [`mixer`]: Real-time mixing engine
//! - [`sample_loader`]: Audio file loading and decoding
//!
//! The main [`AudioEngine`] struct orchestrates these components to provide
//! a high-level audio playback interface for Python.

use crate::audio_engine::audio_stream::{AudioStreamHandle, create_audio_stream, start_stream};
use crate::audio_engine::constants::{
    NUM_SAMPLES, PAD_EQ_DB_MAX, PAD_EQ_DB_MIN, PAD_GAIN_MAX, PAD_GAIN_MIN, SPEED_MAX, SPEED_MIN,
    VOLUME_MAX, VOLUME_MIN,
};
use crate::audio_engine::errors::SampleLoadError;
use crate::audio_engine::sample_loader::{
    SampleLoadProgress, SampleLoadSubtask, cache_sample_buffer_as_project_wav,
    decode_audio_file_to_sample_buffer,
};
use crate::messages::{
    AudioMessage, BackgroundTaskKind, BeatGridData, ControlMessage, LoaderEvent, SampleAnalysis,
    SampleBuffer,
};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    mpsc::{Receiver, Sender, TryRecvError},
};
use std::thread;
use std::time::{Duration, Instant};
use stratum_dsp::{AnalysisConfig, analyze_audio};

mod audio_stream;
mod constants;
mod eq3;
mod errors;
mod mixer;
mod sample_loader;
mod stretch_processor;
mod voice_slot;

#[derive(Debug, Clone, Copy, PartialEq)]
enum LoadProgressStage {
    Decoding,
    Resampling,
    ChannelMapping,
    Analyzing,
    Publishing,
}

impl LoadProgressStage {
    fn stage_label(self) -> &'static str {
        match self {
            Self::Decoding => "decoding",
            Self::Resampling => "resampling",
            Self::ChannelMapping => "channel mapping",
            Self::Analyzing => "Analyzing (bpm/key/beat grid)",
            Self::Publishing => "publishing",
        }
    }

    fn range(self, resampling_required: bool) -> (f32, f32) {
        if !resampling_required {
            return match self {
                Self::Decoding => (0.0, 0.70),
                Self::Resampling => (0.70, 0.70),
                Self::ChannelMapping => (0.70, 0.80),
                Self::Analyzing => (0.80, 0.95),
                Self::Publishing => (0.95, 1.0),
            };
        }

        match self {
            Self::Decoding => (0.0, 0.40),
            Self::Resampling => (0.40, 0.80),
            Self::ChannelMapping => (0.80, 0.85),
            Self::Analyzing => (0.85, 0.95),
            Self::Publishing => (0.95, 1.0),
        }
    }
}

struct ProgressReporter {
    id: usize,
    tx: Sender<LoaderEvent>,
    last_emit: Instant,
    min_interval: Duration,
    resampling_required: Option<bool>,
}

impl ProgressReporter {
    fn new(id: usize, tx: Sender<LoaderEvent>) -> Self {
        let min_interval = Duration::from_millis(100);
        Self {
            id,
            tx,
            last_emit: Instant::now()
                .checked_sub(min_interval)
                .unwrap_or_else(Instant::now),
            min_interval,
            resampling_required: None,
        }
    }

    fn emit(
        &mut self,
        stage: LoadProgressStage,
        local_percent: f32,
        resampling_required: bool,
        force: bool,
    ) {
        let local_percent = if local_percent.is_finite() {
            local_percent.clamp(0.0, 1.0)
        } else {
            0.0
        };

        let now = Instant::now();
        if !force && now.duration_since(self.last_emit) < self.min_interval {
            return;
        }
        self.last_emit = now;

        self.resampling_required.get_or_insert(resampling_required);
        let resampling_required = self.resampling_required.unwrap_or(resampling_required);

        let (start, end) = stage.range(resampling_required);
        let percent = (start + (end - start) * local_percent).clamp(0.0, 1.0);
        let stage = match stage {
            LoadProgressStage::Analyzing => stage.stage_label().to_string(),
            _ => format!("Loading ({})", stage.stage_label()),
        };
        let _ = self.tx.send(LoaderEvent::Progress {
            id: self.id,
            percent,
            stage,
        });
    }
}

fn interleaved_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    match channels {
        0 => Vec::new(),
        1 => samples.to_vec(),
        _ => {
            let mut mono = Vec::with_capacity(samples.len() / channels);
            for frame in samples.chunks_exact(channels) {
                let sum: f32 = frame.iter().copied().sum();
                mono.push(sum / channels as f32);
            }
            mono
        }
    }
}

fn analyze_sample(sample: &SampleBuffer, sample_rate_hz: u32) -> Result<SampleAnalysis, String> {
    let mono = interleaved_to_mono(&sample.samples, sample.channels);

    let result = analyze_audio(&mono, sample_rate_hz, AnalysisConfig::default())
        .map_err(|err| format!("analysis failed: {err}"))?;

    Ok(SampleAnalysis {
        bpm: result.bpm,
        key: result.key.name(),
        beat_grid: BeatGridData {
            beats: result.beat_grid.beats,
            downbeats: result.beat_grid.downbeats,
        },
    })
}

fn task_to_str(task: BackgroundTaskKind) -> &'static str {
    match task {
        BackgroundTaskKind::Analysis => "analysis",
    }
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

/// AudioEngine provides minimal audio output capabilities using cpal
#[pyclass]
pub struct AudioEngine {
    stream_handle: Option<AudioStreamHandle>,
    is_playing: bool,
    loader_tx: Sender<LoaderEvent>,
    loader_rx: Mutex<Receiver<LoaderEvent>>,
    sample_cache: Arc<Mutex<Vec<Option<SampleBuffer>>>>,
    loading_sample_ids: Arc<Mutex<HashSet<usize>>>,
    active_tasks: Arc<Mutex<HashSet<(usize, BackgroundTaskKind)>>>,
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
        })
    }

    /// Initialize and run the audio engine.
    pub fn run(&mut self) -> PyResult<()> {
        if self.stream_handle.is_some() {
            return Err(PyRuntimeError::new_err("AudioEngine already running"));
        }

        match create_audio_stream() {
            Ok(handle) => {
                start_stream(&handle.stream).map_err(|e| {
                    PyRuntimeError::new_err(format!("Failed to start audio stream: {e}"))
                })?;
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

    /// Shut down the audio engine.
    pub fn shut_down(&mut self) -> PyResult<()> {
        self.stream_handle = None;
        self.is_playing = false;
        Ok(())
    }

    /// Load an audio file into a sample slot on a background thread.
    pub fn load_sample_async(&self, id: usize, path: String) -> PyResult<()> {
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

        {
            let mut set = loading_sample_ids
                .lock()
                .map_err(|_| PyRuntimeError::new_err("Failed to acquire loading ids lock"))?;
            if !set.insert(id) {
                return Err(PyValueError::new_err("sample is already loading"));
            }
        }

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

            let _ = loader_tx.send(LoaderEvent::Started { id });

            let mut progress = ProgressReporter::new(id, loader_tx.clone());

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
                        error: format!("File not found: {path}"),
                    });
                    return;
                }
                Err(err) => {
                    let _ = loader_tx.send(LoaderEvent::Error {
                        id,
                        error: err.to_string(),
                    });
                    return;
                }
            };

            let resampling_required = progress.resampling_required.unwrap_or(true);

            let cached_path = match cache_sample_buffer_as_project_wav(
                Path::new("samples"),
                Path::new(&path),
                &sample,
                output_sample_rate,
            ) {
                Ok(path) => path,
                Err(err) => {
                    let _ = loader_tx.send(LoaderEvent::Error {
                        id,
                        error: format!("Failed to write cached WAV: {err}"),
                    });
                    return;
                }
            };
            let cached_path = cached_path.to_string_lossy().replace('\\', "/");

            progress.emit(LoadProgressStage::Analyzing, 0.0, resampling_required, true);

            let analysis = match analyze_sample(&sample, output_sample_rate) {
                Ok(result) => result,
                Err(err) => {
                    let _ = loader_tx.send(LoaderEvent::Error { id, error: err });
                    return;
                }
            };

            progress.emit(LoadProgressStage::Analyzing, 1.0, resampling_required, true);

            progress.emit(
                LoadProgressStage::Publishing,
                0.0,
                resampling_required,
                true,
            );

            let frames = sample.samples.len() / sample.channels;
            let duration_sec = frames as f32 / output_sample_rate as f32;

            let sample_for_audio = sample.clone();
            if let Ok(mut cache) = sample_cache.lock() {
                if let Some(slot) = cache.get_mut(id) {
                    *slot = Some(sample);
                }
            }

            let mut producer_guard = match producer.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    let _ = loader_tx.send(LoaderEvent::Error {
                        id,
                        error: "Failed to acquire producer lock".to_string(),
                    });
                    return;
                }
            };

            if producer_guard
                .push(ControlMessage::LoadSample {
                    id,
                    sample: sample_for_audio,
                })
                .is_err()
            {
                let _ = loader_tx.send(LoaderEvent::Error {
                    id,
                    error: "Failed to send LoadSample - buffer may be full".to_string(),
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
                duration_sec,
                cached_path,
                analysis,
            });
        });

        Ok(())
    }

    /// Analyze a previously loaded sample on a background thread.
    pub fn analyze_sample_async(&self, id: usize) -> PyResult<()> {
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

        {
            let mut tasks = self
                .active_tasks
                .lock()
                .map_err(|_| PyRuntimeError::new_err("Failed to acquire active tasks lock"))?;
            if !tasks.insert((id, BackgroundTaskKind::Analysis)) {
                return Err(PyValueError::new_err("analysis task already running"));
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

        let loader_tx = self.loader_tx.clone();
        let output_sample_rate = handle.output_sample_rate;
        let active_tasks = self.active_tasks.clone();

        thread::spawn(move || {
            let _task_guard = PadTaskGuard {
                id,
                task: BackgroundTaskKind::Analysis,
                active_tasks,
            };

            let _ = loader_tx.send(LoaderEvent::TaskStarted {
                id,
                task: BackgroundTaskKind::Analysis,
            });

            let stage = LoadProgressStage::Analyzing.stage_label().to_string();
            let _ = loader_tx.send(LoaderEvent::TaskProgress {
                id,
                task: BackgroundTaskKind::Analysis,
                percent: 0.0,
                stage: stage.clone(),
            });

            let analysis = match analyze_sample(&sample, output_sample_rate) {
                Ok(result) => result,
                Err(error) => {
                    let _ = loader_tx.send(LoaderEvent::TaskError {
                        id,
                        task: BackgroundTaskKind::Analysis,
                        error,
                    });
                    return;
                }
            };

            let _ = loader_tx.send(LoaderEvent::TaskProgress {
                id,
                task: BackgroundTaskKind::Analysis,
                percent: 1.0,
                stage,
            });

            let _ = loader_tx.send(LoaderEvent::TaskSuccess {
                id,
                task: BackgroundTaskKind::Analysis,
                analysis,
            });
        });

        Ok(())
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
            LoaderEvent::Started { id } => {
                dict.set_item("type", "started")?;
                dict.set_item("id", id)?;
            }
            LoaderEvent::Progress { id, percent, stage } => {
                dict.set_item("type", "progress")?;
                dict.set_item("id", id)?;
                dict.set_item("percent", percent)?;
                dict.set_item("stage", stage)?;
            }
            LoaderEvent::Success {
                id,
                duration_sec,
                cached_path,
                analysis,
            } => {
                dict.set_item("type", "success")?;
                dict.set_item("id", id)?;
                dict.set_item("duration_sec", duration_sec)?;
                dict.set_item("cached_path", cached_path)?;

                let analysis_dict = PyDict::new(py);
                analysis_dict.set_item("bpm", analysis.bpm)?;
                analysis_dict.set_item("key", analysis.key)?;

                let beat_grid_dict = PyDict::new(py);
                beat_grid_dict.set_item("beats", &analysis.beat_grid.beats)?;
                beat_grid_dict.set_item("downbeats", &analysis.beat_grid.downbeats)?;
                analysis_dict.set_item("beat_grid", beat_grid_dict)?;

                dict.set_item("analysis", analysis_dict)?;
            }
            LoaderEvent::Error { id, error } => {
                dict.set_item("type", "error")?;
                dict.set_item("id", id)?;
                dict.set_item("msg", error)?;
            }
            LoaderEvent::TaskStarted { id, task } => {
                dict.set_item("type", "task_started")?;
                dict.set_item("id", id)?;
                dict.set_item("task", task_to_str(task))?;
            }
            LoaderEvent::TaskProgress {
                id,
                task,
                percent,
                stage,
            } => {
                dict.set_item("type", "task_progress")?;
                dict.set_item("id", id)?;
                dict.set_item("task", task_to_str(task))?;
                dict.set_item("percent", percent)?;
                dict.set_item("stage", stage)?;
            }
            LoaderEvent::TaskSuccess { id, task, analysis } => {
                dict.set_item("type", "task_success")?;
                dict.set_item("id", id)?;
                dict.set_item("task", task_to_str(task))?;

                let analysis_dict = PyDict::new(py);
                analysis_dict.set_item("bpm", analysis.bpm)?;
                analysis_dict.set_item("key", analysis.key)?;

                let beat_grid_dict = PyDict::new(py);
                beat_grid_dict.set_item("beats", &analysis.beat_grid.beats)?;
                beat_grid_dict.set_item("downbeats", &analysis.beat_grid.downbeats)?;
                analysis_dict.set_item("beat_grid", beat_grid_dict)?;

                dict.set_item("analysis", analysis_dict)?;
            }
            LoaderEvent::TaskError { id, task, error } => {
                dict.set_item("type", "task_error")?;
                dict.set_item("id", id)?;
                dict.set_item("task", task_to_str(task))?;
                dict.set_item("msg", error)?;
            }
        }

        Ok(Some(dict.into_any().unbind()))
    }

    /// Trigger playback of a previously loaded sample.
    pub fn play_sample(&mut self, id: usize, volume: f32) -> PyResult<()> {
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
            .push(ControlMessage::PlaySample { id, volume })
            .map_err(|_| PyRuntimeError::new_err("Failed to send PlaySample - buffer may be full"))
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
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        let _ = producer_guard.push(ControlMessage::SetVolume(volume));
        Ok(())
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
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        let _ = producer_guard.push(ControlMessage::SetSpeed(speed));
        Ok(())
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

        let _ = producer_guard.push(ControlMessage::SetBpmLock(enabled));
        Ok(())
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

        let _ = producer_guard.push(ControlMessage::SetKeyLock(enabled));
        Ok(())
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
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        let _ = producer_guard.push(ControlMessage::SetMasterBpm(bpm));
        Ok(())
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
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        let _ = producer_guard.push(ControlMessage::SetPadBpm { id, bpm });
        Ok(())
    }

    pub fn set_pad_gain(&mut self, id: usize, gain: f32) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }

        if !gain.is_finite() || !(PAD_GAIN_MIN..=PAD_GAIN_MAX).contains(&gain) {
            return Err(PyValueError::new_err("gain out of range"));
        }

        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        let _ = producer_guard.push(ControlMessage::SetPadGain { id, gain });
        Ok(())
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
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        let _ = producer_guard.push(ControlMessage::SetPadEq {
            id,
            low_db,
            mid_db,
            high_db,
        });
        Ok(())
    }

    pub fn set_pad_loop_region(
        &mut self,
        id: usize,
        start_s: f32,
        end_s: Option<f32>,
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

        let _ = producer_guard.push(ControlMessage::SetPadLoopRegion { id, start_s, end_s });
        Ok(())
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

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::UnloadSample { id })
            .map_err(|_| {
                PyRuntimeError::new_err("Failed to send UnloadSample - buffer may be full")
            })?;

        if let Ok(mut cache) = self.sample_cache.lock() {
            if let Some(slot) = cache.get_mut(id) {
                *slot = None;
            }
        }

        if let Ok(mut set) = self.loading_sample_ids.lock() {
            set.remove(&id);
        }

        if let Ok(mut set) = self.active_tasks.lock() {
            set.remove(&(id, BackgroundTaskKind::Analysis));
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
}
