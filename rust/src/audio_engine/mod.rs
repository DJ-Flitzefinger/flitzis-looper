use crate::audio_engine::analysis::analyze_sample;
use crate::audio_engine::audio_stream::{AudioStreamHandle, create_audio_stream, start_stream};
use crate::audio_engine::constants::{
    NUM_SAMPLES, PAD_EQ_DB_MAX, PAD_EQ_DB_MIN, PAD_GAIN_MAX, PAD_GAIN_MIN, SPEED_MAX, SPEED_MIN,
    VOLUME_MAX, VOLUME_MIN,
};
use crate::audio_engine::errors::SampleLoadError;
use crate::audio_engine::progress::{LoadProgressStage, ProgressReporter};
use crate::audio_engine::sample_loader::{
    SampleLoadProgress, SampleLoadSubtask, cache_audio_file_for_project,
    decode_audio_file_to_sample_buffer,
};
use crate::messages::{
    AudioMessage, BackgroundTaskKind, ControlMessage, LoaderEvent, SampleBuffer, task_to_str,
};
use numpy::{PyArray1, ToPyArray};
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

mod analysis;
mod audio_stream;
mod channels;
mod constants;
mod eq3;
mod errors;
mod mixer;
mod progress;
mod sample_loader;
mod stretch_processor;
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
    ) -> PyResult<()> {
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
        let run_analysis = run_analysis.unwrap_or(true);

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
                        let _ = loader_tx.send(LoaderEvent::Error { id, error: err });
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

            let sample_for_audio = sample.clone();
            if let Ok(mut cache) = sample_cache.lock()
                && let Some(slot) = cache.get_mut(id)
            {
                *slot = Some(sample);
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
                duration_s,
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
                duration_s,
                cached_path,
                analysis,
            } => {
                dict.set_item("type", "success")?;
                dict.set_item("id", id)?;
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

        if let Ok(mut cache) = self.sample_cache.lock()
            && let Some(slot) = cache.get_mut(id)
        {
            *slot = None;
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
