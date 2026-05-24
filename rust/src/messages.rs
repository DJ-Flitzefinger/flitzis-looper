//! Message definitions for communication between Python and Rust audio threads.
//!
//! This module defines the enums that serve as the wire format for messages passed through the
//! ring buffer between the Python thread and the real-time audio thread.

use pyo3::prelude::*;
use std::sync::Arc;
use stratum_dsp::BeatGrid;

pub(crate) const STEM_BUFFER_COUNT: usize = 5;
pub(crate) const STEM_MASK_VOCALS: u8 = 1 << 0;
pub(crate) const STEM_MASK_MELODY: u8 = 1 << 1;
pub(crate) const STEM_MASK_BASS: u8 = 1 << 2;
pub(crate) const STEM_MASK_DRUMS: u8 = 1 << 3;
pub(crate) const STEM_COMPONENT_MASK: u8 =
    STEM_MASK_VOCALS | STEM_MASK_MELODY | STEM_MASK_BASS | STEM_MASK_DRUMS;

#[derive(Debug, Clone)]
pub(crate) struct SampleBuffer {
    pub channels: usize,
    pub samples: Arc<[f32]>,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedStemSet {
    pub source_version_hash: u64,
    pub sample_rate_hz: u32,
    pub channels: usize,
    pub frame_count: usize,
    pub available_mask: u8,
    pub stems: [SampleBuffer; STEM_BUFFER_COUNT],
}

/// Message that is emitted from the audio thread.
#[derive(Debug, Clone)]
#[pyclass]
pub enum AudioMessage {
    /// Response to a Ping message.
    Pong(),

    /// Sample playback started
    SampleStarted { id: usize },

    /// Sample playback stopped
    SampleStopped { id: usize },

    /// Per-pad peak meter update (mono peak, post gain/EQ).
    PadPeak { id: usize, peak: f32 },

    /// Per-pad playback position in seconds (best-effort, low-rate).
    PadPlayhead { id: usize, position_s: f32 },
}

#[pymethods]
impl AudioMessage {
    pub fn sample_id(&self) -> Option<usize> {
        match self {
            AudioMessage::SampleStarted { id } => Some(*id),
            AudioMessage::SampleStopped { id } => Some(*id),
            AudioMessage::PadPeak { id, peak: _ } => Some(*id),
            AudioMessage::PadPlayhead { id, position_s: _ } => Some(*id),
            _ => None,
        }
    }

    pub fn pad_peak(&self) -> Option<f32> {
        match self {
            AudioMessage::PadPeak { id: _, peak } => Some(*peak),
            _ => None,
        }
    }

    pub fn pad_playhead(&self) -> Option<f32> {
        match self {
            AudioMessage::PadPlayhead { id: _, position_s } => Some(*position_s),
            _ => None,
        }
    }
}

/// Quantization mode used by Rust-side pad trigger scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerQuantization {
    Immediate,
    Grid { step_64ths: u16 },
}

/// Per-pad stem render source selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StemMixMode {
    FullMix,
    AllStems,
}

/// Bounded per-pad timing metadata prepared outside the audio callback.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PadTimingMetadata {
    pub phase_anchor_s: f32,
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

    /// Set bounded per-pad beatgrid/downbeat timing metadata.
    SetPadTimingMetadata {
        id: usize,
        metadata: PadTimingMetadata,
    },

    /// Request transport downbeat anchoring from a selected playing pad.
    AnchorTransportPhaseFromPad { id: usize },

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

    /// Set Rust-side trigger quantization mode for future pad triggers.
    SetTriggerQuantization(TriggerQuantization),

    /// Publish a loaded sample into an audio-thread slot.

    ///
    /// # Parameters
    /// * `id` - Unique identifier for the sample slot (0..36)
    /// * `sample` - Pre-decoded immutable sample buffer (shared handle)
    LoadSample { id: usize, sample: SampleBuffer },

    /// Publish validated prepared stems into an audio-thread slot.
    ///
    /// The message carries bounded metadata plus shared immutable buffer handles. It must not
    /// contain file paths, Python objects, or copied full audio payloads.
    PublishPreparedStems { id: usize, stems: PreparedStemSet },

    /// Select whether a pad renders from the full mix or all prepared stems.
    ///
    /// The source-version hash is used by all-stems mode to reject stale updates. Full-mix mode
    /// ignores the hash and always remains available.
    SetStemMixMode {
        id: usize,
        mode: StemMixMode,
        source_version_hash: u64,
    },

    /// Select which prepared component stems are enabled for an all-stems pad.
    ///
    /// The mask uses known stem-kind bits and is source-version guarded. It must not contain file
    /// paths, Python objects, or copied full audio payloads.
    SetStemEnabledMask {
        id: usize,
        enabled_stem_mask: u8,
        source_version_hash: u64,
    },

    /// Play a loaded sample.
    ///
    /// # Parameters
    /// * `id` - Identifier of the sample to play
    /// * `volume` - Playback volume (0.0 to 1.0)
    PlaySample { id: usize, volume: f32 },

    /// Stop all active voices, then play a loaded sample as one audio-thread command.
    ///
    /// # Parameters
    /// * `id` - Identifier of the sample to play
    /// * `volume` - Playback volume (0.0 to 1.0)
    PlaySampleExclusive { id: usize, volume: f32 },

    /// Stop all active voices for a sample.
    ///
    /// # Parameters
    /// * `id` - Identifier of the sample to stop
    StopSample { id: usize },

    /// Stop all currently active voices.
    StopAll(),

    /// Pause playback of a sample without resetting position.
    ///
    /// If the sample is playing, its voice becomes silent but retains its
    /// current playback position. If the sample is not playing, this has no effect.
    PauseSample { id: usize },

    /// Resume playback of a paused sample from its saved position.
    ///
    /// If the sample was paused, playback continues from that point.
    /// If the sample was not paused, this has no effect.
    ResumeSample { id: usize },

    /// Unload a sample slot.
    ///
    /// This stops all active voices for the sample and clears the sample buffer in the slot.
    ///
    /// # Parameters
    /// * `id` - Identifier of the sample slot to unload
    UnloadSample { id: usize },
}

#[derive(Debug, Clone)]
pub(crate) struct SampleAnalysis {
    pub bpm: f32,
    pub key: String,
    pub beat_grid: BeatGrid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackgroundTaskKind {
    Analysis,
    StemGeneration,
}

pub fn task_to_str(task: BackgroundTaskKind) -> &'static str {
    match task {
        BackgroundTaskKind::Analysis => "analysis",
        BackgroundTaskKind::StemGeneration => "stem_generation",
    }
}

/// Events emitted from background work (loading and per-pad tasks).
#[derive(Debug, Clone)]
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
        duration_s: f32,
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
        analysis: Option<SampleAnalysis>,
    },

    /// A per-pad task failed.
    TaskError {
        id: usize,
        task: BackgroundTaskKind,
        error: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_trigger_quantization_message_carries_fixed_size_mode() {
        let message =
            ControlMessage::SetTriggerQuantization(TriggerQuantization::Grid { step_64ths: 4 });

        assert!(matches!(
            message,
            ControlMessage::SetTriggerQuantization(TriggerQuantization::Grid { step_64ths: 4 })
        ));
    }

    #[test]
    fn play_sample_exclusive_message_carries_fixed_size_trigger() {
        let message = ControlMessage::PlaySampleExclusive {
            id: 3,
            volume: 0.75,
        };

        assert!(matches!(
            message,
            ControlMessage::PlaySampleExclusive { id: 3, volume } if volume == 0.75
        ));
    }

    #[test]
    fn pad_timing_metadata_message_carries_fixed_size_anchor() {
        let message = ControlMessage::SetPadTimingMetadata {
            id: 3,
            metadata: PadTimingMetadata {
                phase_anchor_s: 1.25,
            },
        };

        assert!(matches!(
            message,
            ControlMessage::SetPadTimingMetadata {
                id: 3,
                metadata: PadTimingMetadata { phase_anchor_s }
            } if phase_anchor_s == 1.25
        ));
    }

    #[test]
    fn anchor_transport_phase_message_carries_fixed_size_pad_id() {
        let message = ControlMessage::AnchorTransportPhaseFromPad { id: 3 };

        assert!(matches!(
            message,
            ControlMessage::AnchorTransportPhaseFromPad { id: 3 }
        ));
    }

    #[test]
    fn stem_generation_task_has_stable_event_name() {
        assert_eq!(
            task_to_str(BackgroundTaskKind::StemGeneration),
            "stem_generation"
        );
    }

    #[test]
    fn prepared_stem_publication_message_carries_fixed_size_handles() {
        let buffer = SampleBuffer {
            channels: 1,
            samples: Arc::from([0.0_f32, 0.0].as_slice()),
        };
        let stems = PreparedStemSet {
            source_version_hash: 42,
            sample_rate_hz: 44_100,
            channels: 1,
            frame_count: 2,
            available_mask: 0b1_1111,
            stems: std::array::from_fn(|_| buffer.clone()),
        };
        let message = ControlMessage::PublishPreparedStems { id: 3, stems };

        assert!(matches!(
            message,
            ControlMessage::PublishPreparedStems {
                id: 3,
                stems: PreparedStemSet {
                    source_version_hash: 42,
                    sample_rate_hz: 44_100,
                    channels: 1,
                    frame_count: 2,
                    available_mask: 0b1_1111,
                    stems: _
                }
            }
        ));
    }

    #[test]
    fn prepared_stem_publication_reports_ring_buffer_full() {
        let buffer = SampleBuffer {
            channels: 1,
            samples: Arc::from([0.0_f32, 0.0].as_slice()),
        };
        let stems = PreparedStemSet {
            source_version_hash: 42,
            sample_rate_hz: 44_100,
            channels: 1,
            frame_count: 2,
            available_mask: 0b1_1111,
            stems: std::array::from_fn(|_| buffer.clone()),
        };
        let (mut producer, _consumer) = rtrb::RingBuffer::<ControlMessage>::new(1);
        producer.push(ControlMessage::Ping()).unwrap();

        let result = producer.push(ControlMessage::PublishPreparedStems { id: 3, stems });

        assert!(result.is_err());
    }

    #[test]
    fn stem_mix_mode_message_carries_bounded_state() {
        let message = ControlMessage::SetStemMixMode {
            id: 3,
            mode: StemMixMode::AllStems,
            source_version_hash: 42,
        };

        assert!(matches!(
            message,
            ControlMessage::SetStemMixMode {
                id: 3,
                mode: StemMixMode::AllStems,
                source_version_hash: 42
            }
        ));
    }

    #[test]
    fn stem_enabled_mask_message_carries_bounded_state() {
        let message = ControlMessage::SetStemEnabledMask {
            id: 3,
            enabled_stem_mask: STEM_MASK_VOCALS | STEM_MASK_DRUMS,
            source_version_hash: 42,
        };

        assert!(matches!(
            message,
            ControlMessage::SetStemEnabledMask {
                id: 3,
                enabled_stem_mask,
                source_version_hash: 42
            } if enabled_stem_mask == (STEM_MASK_VOCALS | STEM_MASK_DRUMS)
        ));
    }
}
