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
use crate::messages::{AudioMessage, ControlMessage};
use pyo3::exceptions::{PyFileNotFoundError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use std::path::Path;

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
}

#[pymethods]
impl AudioEngine {
    /// Create a new AudioEngine instance with default audio device.
    #[new]
    pub fn new() -> PyResult<Self> {
        Ok(AudioEngine {
            stream_handle: None,
            is_playing: false,
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
