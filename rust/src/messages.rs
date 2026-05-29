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

    /// Per-pad peak meter update (mono peak, post Gain/Trim and EQ, pre-master).
    PadPeak { id: usize, peak: f32 },

    /// Master output peak meter update (mono peak, post-sum and post-master volume).
    MasterPeak { peak: f32 },

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
            AudioMessage::MasterPeak { peak: _ } => None,
            _ => None,
        }
    }

    pub fn pad_peak(&self) -> Option<f32> {
        match self {
            AudioMessage::PadPeak { id: _, peak } => Some(*peak),
            _ => None,
        }
    }

    pub fn master_peak(&self) -> Option<f32> {
        match self {
            AudioMessage::MasterPeak { peak } => Some(*peak),
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

/// High-level semantics for ordered control messages.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlMessageClass {
    Test,
    PlaybackEvent,
    OrderedState,
    Publication,
}

/// Identity used to coalesce continuous parameter updates.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlParameterKey {
    Volume,
    Speed,
    MasterBpm,
    PadBpm(usize),
    PadGain(usize),
    PadEq(usize),
}

/// Continuous or frequently updated audio parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
// Keep stable Set* message names aligned with the existing command/parameter API.
#[allow(clippy::enum_variant_names)]
pub(crate) enum ControlParameterMessage {
    /// Set the global volume level.
    SetVolume(f32),

    /// Set the global speed multiplier.
    SetSpeed(f32),

    /// Set the current master BPM when BPM lock is enabled.
    SetMasterBpm(f32),

    /// Set per-pad BPM metadata.
    SetPadBpm { id: usize, bpm: Option<f32> },

    /// Set per-pad Gain/Trim in dB.
    SetPadGain { id: usize, gain_db: f32 },

    /// Set per-pad 3-band EQ gains in dB.
    SetPadEq {
        id: usize,
        low_db: f32,
        mid_db: f32,
        high_db: f32,
    },
}

#[cfg(test)]
impl ControlParameterMessage {
    pub(crate) fn key(&self) -> ControlParameterKey {
        match self {
            ControlParameterMessage::SetVolume(_) => ControlParameterKey::Volume,
            ControlParameterMessage::SetSpeed(_) => ControlParameterKey::Speed,
            ControlParameterMessage::SetMasterBpm(_) => ControlParameterKey::MasterBpm,
            ControlParameterMessage::SetPadBpm { id, bpm: _ } => ControlParameterKey::PadBpm(*id),
            ControlParameterMessage::SetPadGain { id, gain_db: _ } => {
                ControlParameterKey::PadGain(*id)
            }
            ControlParameterMessage::SetPadEq {
                id,
                low_db: _,
                mid_db: _,
                high_db: _,
            } => ControlParameterKey::PadEq(*id),
        }
    }
}

/// Message that is emitted from the Python side.
#[derive(Debug, Clone)]
pub enum ControlMessage {
    /// Used for testing message passing functionality.
    Ping(),

    /// Enable or disable BPM lock.
    SetBpmLock(bool),

    /// Enable or disable key lock (preserve pitch under tempo changes).
    SetKeyLock(bool),

    /// Enable or disable Key Lock for one pad.
    SetPadKeyLock { id: usize, enabled: bool },

    /// Set bounded per-pad beatgrid/downbeat timing metadata.
    SetPadTimingMetadata {
        id: usize,
        metadata: PadTimingMetadata,
    },

    /// Request transport downbeat anchoring from a selected playing pad.
    AnchorTransportPhaseFromPad { id: usize },

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
    /// * `id` - Unique identifier for the sample slot (0..NUM_SAMPLES)
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

    /// Seek an active or paused sample voice to a source position in seconds.
    ///
    /// If the sample has no active or paused voice, this has no effect.
    SeekSample { id: usize, position_s: f32 },

    /// Unload a sample slot.
    ///
    /// This stops all active voices for the sample and clears the sample buffer in the slot.
    ///
    /// # Parameters
    /// * `id` - Identifier of the sample slot to unload
    UnloadSample { id: usize },
}

#[cfg(test)]
impl ControlMessage {
    pub(crate) fn class(&self) -> ControlMessageClass {
        match self {
            ControlMessage::Ping() => ControlMessageClass::Test,
            ControlMessage::PlaySample { .. }
            | ControlMessage::PlaySampleExclusive { .. }
            | ControlMessage::StopSample { .. }
            | ControlMessage::StopAll()
            | ControlMessage::PauseSample { .. }
            | ControlMessage::ResumeSample { .. }
            | ControlMessage::SeekSample { .. } => ControlMessageClass::PlaybackEvent,
            ControlMessage::LoadSample { .. } | ControlMessage::PublishPreparedStems { .. } => {
                ControlMessageClass::Publication
            }
            ControlMessage::SetBpmLock(_)
            | ControlMessage::SetKeyLock(_)
            | ControlMessage::SetPadKeyLock { .. }
            | ControlMessage::SetPadTimingMetadata { .. }
            | ControlMessage::AnchorTransportPhaseFromPad { .. }
            | ControlMessage::SetPadLoopRegion { .. }
            | ControlMessage::SetTriggerQuantization(_)
            | ControlMessage::SetStemMixMode { .. }
            | ControlMessage::SetStemEnabledMask { .. }
            | ControlMessage::UnloadSample { .. } => ControlMessageClass::OrderedState,
        }
    }
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
    Started { id: usize, request_id: u64 },

    /// A progress update.
    ///
    /// - `percent` is the best-effort *total* progress across the full load pipeline (0.0..=1.0).
    /// - `stage` is a human-readable stage string (e.g. "Loading (decoding)").
    Progress {
        id: usize,
        request_id: u64,
        percent: f32,
        stage: String,
    },

    /// Loading completed successfully.
    Success {
        id: usize,
        request_id: u64,
        duration_s: f32,
        cached_path: String,
        analysis: Option<SampleAnalysis>,
    },

    /// Loading failed.
    Error {
        id: usize,
        request_id: u64,
        error: String,
    },

    /// A per-pad background task started.
    TaskStarted {
        id: usize,
        request_id: u64,
        task: BackgroundTaskKind,
    },

    /// A per-pad task progress update.
    TaskProgress {
        id: usize,
        request_id: u64,
        task: BackgroundTaskKind,
        percent: f32,
        stage: String,
    },

    /// A per-pad task completed successfully.
    TaskSuccess {
        id: usize,
        request_id: u64,
        task: BackgroundTaskKind,
        analysis: Option<SampleAnalysis>,
    },

    /// A per-pad task failed.
    TaskError {
        id: usize,
        request_id: u64,
        task: BackgroundTaskKind,
        error: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_peak_message_exposes_unclamped_output_peak() {
        let message = AudioMessage::MasterPeak { peak: 1.25 };

        assert_eq!(message.sample_id(), None);
        assert_eq!(message.pad_peak(), None);
        assert_eq!(message.master_peak(), Some(1.25));
    }

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

    #[test]
    fn pad_key_lock_message_carries_bounded_state() {
        let message = ControlMessage::SetPadKeyLock {
            id: 3,
            enabled: true,
        };

        assert!(matches!(
            message,
            ControlMessage::SetPadKeyLock {
                id: 3,
                enabled: true
            }
        ));
    }

    #[test]
    fn control_messages_classify_ordered_and_parameter_semantics() {
        assert_eq!(
            ControlMessage::PlaySample { id: 1, volume: 1.0 }.class(),
            ControlMessageClass::PlaybackEvent
        );
        assert_eq!(
            ControlMessage::SeekSample {
                id: 1,
                position_s: 2.5,
            }
            .class(),
            ControlMessageClass::PlaybackEvent
        );
        assert_eq!(
            ControlMessage::LoadSample {
                id: 1,
                sample: SampleBuffer {
                    channels: 1,
                    samples: Arc::from([0.0_f32].as_slice()),
                },
            }
            .class(),
            ControlMessageClass::Publication
        );
        assert_eq!(
            ControlMessage::SetPadLoopRegion {
                id: 1,
                start_s: 0.0,
                end_s: None,
            }
            .class(),
            ControlMessageClass::OrderedState
        );
        assert_eq!(
            ControlMessage::SetTriggerQuantization(TriggerQuantization::Immediate).class(),
            ControlMessageClass::OrderedState
        );
        assert_eq!(
            ControlMessage::SetPadKeyLock {
                id: 1,
                enabled: true,
            }
            .class(),
            ControlMessageClass::OrderedState
        );
    }

    #[test]
    fn parameter_messages_expose_stable_coalescing_keys() {
        assert_eq!(
            ControlParameterMessage::SetVolume(0.5).key(),
            ControlParameterKey::Volume
        );
        assert_eq!(
            ControlParameterMessage::SetPadGain {
                id: 3,
                gain_db: 0.75,
            }
            .key(),
            ControlParameterKey::PadGain(3)
        );
        assert_eq!(
            ControlParameterMessage::SetPadEq {
                id: 4,
                low_db: 1.0,
                mid_db: 2.0,
                high_db: 3.0,
            }
            .key(),
            ControlParameterKey::PadEq(4)
        );
    }
}
