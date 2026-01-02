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
use crate::audio_engine::sample_loader::decode_audio_file_to_sample_buffer;
use crate::messages::{AudioMessage, ControlMessage, LoaderEvent};
use pyo3::exceptions::{PyFileNotFoundError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::path::Path;
use std::sync::{
    Mutex,
    mpsc::{Receiver, Sender, TryRecvError},
};
use std::thread;

mod audio_stream;
mod constants;
mod errors;
mod mixer;
mod sample_loader;
mod voice;

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

    /// Load an audio file into a sample slot.
    pub fn load_sample(&mut self, id: usize, path: &str) -> PyResult<()> {
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

        let sample = match decode_audio_file_to_sample_buffer(
            Path::new(path),
            handle.output_channels,
            handle.output_sample_rate,
        ) {
            Ok(sample) => sample,
            Err(SampleLoadError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(PyFileNotFoundError::new_err(path.to_string()));
            }
            Err(err) => {
                return Err(PyValueError::new_err(err.to_string()));
            }
        };

        let mut producer_guard = handle
            .producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::LoadSample { id, sample })
            .map_err(|_| PyRuntimeError::new_err("Failed to send LoadSample - buffer may be full"))
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

            let sample = match decode_audio_file_to_sample_buffer(
                Path::new(&path),
                output_channels,
                output_sample_rate,
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

            let _ = loader_tx.send(LoaderEvent::Progress { id, percent: 0.5 });

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
            LoaderEvent::Progress { id, percent } => {
                dict.set_item("type", "progress")?;
                dict.set_item("id", id)?;
                dict.set_item("percent", percent)?;
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
