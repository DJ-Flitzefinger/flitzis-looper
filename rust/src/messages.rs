//! Message definitions for communication between Python and Rust audio threads.
//!
//! This module defines the enums that serve as the wire format for messages passed through the
//! ring buffer between the Python thread and the real-time audio thread.

use pyo3::prelude::*;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) struct SampleBuffer {
    pub channels: usize,
    pub samples: Arc<[f32]>,
}

/// Message that is emitted from the audio thread.
#[derive(Debug, Clone)]
#[pyclass]
pub enum AudioMessage {
    /// Response to a Ping message.
    Pong(),

    /// Indicates the audio playback is stopped.
    Stopped(),
}

/// Message that is emitted from the Python side.
#[derive(Debug, Clone)]
pub enum ControlMessage {
    /// Used for testing message passing functionality.
    Ping(),

    /// Start the audio playback.
    Play(),

    /// Stop the audio playback.
    Stop(),

    /// Set the volume level.
    ///
    /// # Parameters
    /// * `volume` - Volume level (0.0 to 1.0)
    SetVolume(f32),

    /// Publish a loaded sample into an audio-thread slot.
    ///
    /// # Parameters
    /// * `id` - Unique identifier for the sample slot (0..32)
    /// * `sample` - Pre-decoded immutable sample buffer (shared handle)
    LoadSample { id: usize, sample: SampleBuffer },

    /// Play a loaded sample.
    ///
    /// # Parameters
    /// * `id` - Identifier of the sample to play
    /// * `velocity` - Playback velocity (0.0 to 1.0)
    PlaySample { id: usize, velocity: f32 },
}
