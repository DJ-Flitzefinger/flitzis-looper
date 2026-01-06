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

    /// Per-pad peak meter update (mono peak, post gain/EQ).
    PadPeak { id: usize, peak: f32 },

    /// Per-pad playback position in seconds (best-effort, low-rate).
    PadPlayhead { id: usize, position_s: f32 },
}

#[pymethods]
impl AudioMessage {
    pub fn pad_peak(&self) -> Option<(usize, f32)> {
        match self {
            AudioMessage::PadPeak { id, peak } => Some((*id, *peak)),
            _ => None,
        }
    }

    pub fn pad_playhead(&self) -> Option<(usize, f32)> {
        match self {
            AudioMessage::PadPlayhead { id, position_s } => Some((*id, *position_s)),
            _ => None,
        }
    }
}

/// Message that is emitted from the Python side.
#[derive(Debug, Clone)]
pub enum ControlMessage {
    /// Used for testing message passing functionality.
    Ping(),

    /// Set the global volume level.
    ///
    /// # Parameters
    /// * `volume` - Volume level (0.0 to 1.0)
    SetVolume(f32),

    /// Set the global speed multiplier.
    ///
    /// # Parameters
    /// * `speed` - Speed multiplier (0.5 to 2.0)
    SetSpeed(f32),

    /// Enable or disable BPM lock.
    SetBpmLock(bool),

    /// Enable or disable key lock (preserve pitch under tempo changes).
    SetKeyLock(bool),

    /// Set the current master BPM when BPM lock is enabled.
    SetMasterBpm(f32),

    /// Set per-pad BPM metadata.
    SetPadBpm { id: usize, bpm: Option<f32> },

    /// Set per-pad gain (linear scalar).
    SetPadGain { id: usize, gain: f32 },

    /// Set per-pad 3-band EQ gains in dB.
    SetPadEq {
        id: usize,
        low_db: f32,
        mid_db: f32,
        high_db: f32,
    },

    /// Set per-pad loop region in seconds.
    ///
    /// If `end_s` is None, the loop end defaults to the full sample length.
    SetPadLoopRegion {
        id: usize,
        start_s: f32,
        end_s: Option<f32>,
    },

    /// Publish a loaded sample into an audio-thread slot.

    ///
    /// # Parameters
    /// * `id` - Unique identifier for the sample slot (0..36)
    /// * `sample` - Pre-decoded immutable sample buffer (shared handle)
    LoadSample { id: usize, sample: SampleBuffer },

    /// Play a loaded sample.
    ///
    /// # Parameters
    /// * `id` - Identifier of the sample to play
    /// * `volume` - Playback volume (0.0 to 1.0)
    PlaySample { id: usize, volume: f32 },

    /// Stop all active voices for a sample.
    ///
    /// # Parameters
    /// * `id` - Identifier of the sample to stop
    StopSample { id: usize },

    /// Stop all currently active voices.
    StopAll(),

    /// Unload a sample slot.
    ///
    /// This stops all active voices for the sample and clears the sample buffer in the slot.
    ///
    /// # Parameters
    /// * `id` - Identifier of the sample slot to unload
    UnloadSample { id: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BeatGridData {
    pub beats: Vec<f32>,
    pub downbeats: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SampleAnalysis {
    pub bpm: f32,
    pub key: String,
    pub beat_grid: BeatGridData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackgroundTaskKind {
    Analysis,
}

/// Events emitted from background work (loading and per-pad tasks).
#[derive(Debug, Clone, PartialEq)]
pub enum LoaderEvent {
    /// Loading started for the given sample slot id.
    Started { id: usize },

    /// A progress update.
    ///
    /// - `percent` is the best-effort *total* progress across the full load pipeline (0.0..=1.0).
    /// - `stage` is a human-readable stage string (e.g. "Loading (decoding)").
    Progress {
        id: usize,
        percent: f32,
        stage: String,
    },

    /// Loading completed successfully.
    Success {
        id: usize,
        duration_sec: f32,
        cached_path: String,
        analysis: Option<SampleAnalysis>,
    },

    /// Loading failed.
    Error { id: usize, error: String },

    /// A per-pad background task started.
    TaskStarted { id: usize, task: BackgroundTaskKind },

    /// A per-pad task progress update.
    TaskProgress {
        id: usize,
        task: BackgroundTaskKind,
        percent: f32,
        stage: String,
    },

    /// A per-pad task completed successfully.
    TaskSuccess {
        id: usize,
        task: BackgroundTaskKind,
        analysis: SampleAnalysis,
    },

    /// A per-pad task failed.
    TaskError {
        id: usize,
        task: BackgroundTaskKind,
        error: String,
    },
}
