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
use crate::audio_engine::constants::{NUM_SAMPLES, SPEED_MAX, SPEED_MIN, VOLUME_MAX, VOLUME_MIN};
use crate::audio_engine::errors::SampleLoadError;
use crate::audio_engine::sample_loader::{
    SampleLoadProgress, SampleLoadSubtask, decode_audio_file_to_sample_buffer_with_progress,
};
use crate::messages::{AudioMessage, ControlMessage, LoaderEvent};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::path::Path;
use std::sync::{
    Mutex,
    mpsc::{Receiver, Sender, TryRecvError},
};
use std::thread;
use std::time::{Duration, Instant};

mod audio_stream;
mod constants;
mod errors;
mod mixer;
mod sample_loader;
mod voice;

#[derive(Debug, Clone, Copy, PartialEq)]
enum LoadProgressStage {
    Decoding,
    Resampling,
    ChannelMapping,
    Publishing,
}

impl LoadProgressStage {
    fn stage_label(self) -> &'static str {
        match self {
            Self::Decoding => "decoding",
            Self::Resampling => "resampling",
            Self::ChannelMapping => "channel mapping",
            Self::Publishing => "publishing",
        }
    }

    fn range(self, resampling_required: bool) -> (f32, f32) {
        if !resampling_required {
            return match self {
                Self::Decoding => (0.0, 1.0),
                Self::Resampling | Self::ChannelMapping | Self::Publishing => (1.0, 1.0),
            };
        }

        match self {
            Self::Decoding => (0.0, 0.45),
            Self::Resampling => (0.45, 0.90),
            Self::ChannelMapping => (0.90, 0.95),
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
        let stage = format!("Loading ({})", stage.stage_label());
        let _ = self.tx.send(LoaderEvent::Progress {
            id: self.id,
            percent,
            stage,
        });
    }
}

/// AudioEngine provides minimal audio output capabilities using cpal
#[pyclass]
pub struct AudioEngine {
    stream_handle: Option<AudioStreamHandle>,
    is_playing: bool,
    loader_tx: Sender<LoaderEvent>,
    loader_rx: Mutex<Receiver<LoaderEvent>>,
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

        thread::spawn(move || {
            let _ = loader_tx.send(LoaderEvent::Started { id });

            let mut progress = ProgressReporter::new(id, loader_tx.clone());

            let sample = match decode_audio_file_to_sample_buffer_with_progress(
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

            progress.emit(
                LoadProgressStage::Publishing,
                0.0,
                progress.resampling_required.unwrap_or(true),
                true,
            );

            let frames = sample.samples.len() / sample.channels;
            let duration_sec = frames as f32 / output_sample_rate as f32;

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
                .push(ControlMessage::LoadSample { id, sample })
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
                progress.resampling_required.unwrap_or(true),
                true,
            );
            let _ = loader_tx.send(LoaderEvent::Success { id, duration_sec });
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
            LoaderEvent::Success { id, duration_sec } => {
                dict.set_item("type", "success")?;
                dict.set_item("id", id)?;
                dict.set_item("duration_sec", duration_sec)?;
            }
            LoaderEvent::Error { id, error } => {
                dict.set_item("type", "error")?;
                dict.set_item("id", id)?;
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
            })
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
