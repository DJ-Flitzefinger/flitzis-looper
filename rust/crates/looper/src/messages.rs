//! Message definitions for communication between Python and Rust audio threads.
//!
//! This module defines the enums that serve as the wire format for messages passed through the
//! ring buffer between the Python thread and the real-time audio thread.

use crate::audio_engine::constant_timing::{AcceptedTimingProjection, PreparedConstantTiming};
use crate::audio_engine::content_placement::SlotSnapshot;
use crate::audio_engine::input_runtime_binding::InputPadBinding;
use crate::audio_engine::prepared_source::PreparedSourcePermit;
use pyo3::prelude::*;
use std::sync::Arc;

/// Resident render layers; the fifth instrumental artifact stays in the complete disk pair.
pub(crate) const STEM_BUFFER_COUNT: usize = 4;
pub(crate) const STEM_MASK_VOCALS: u8 = 1 << 0;
pub(crate) const STEM_MASK_MELODY: u8 = 1 << 1;
pub(crate) const STEM_MASK_BASS: u8 = 1 << 2;
pub(crate) const STEM_MASK_DRUMS: u8 = 1 << 3;
pub(crate) const STEM_COMPONENT_MASK: u8 =
    STEM_MASK_VOCALS | STEM_MASK_MELODY | STEM_MASK_BASS | STEM_MASK_DRUMS;

#[derive(Debug, Clone)]
pub struct SampleBuffer {
    pub channels: usize,
    pub samples: Arc<[f32]>,
    /// Complete source authority is independent of this resident PCM allocation.
    pub residency: Option<Arc<ResidentSourceView>>,
}

/// Immutable complete-content descriptor. This owner never retains complete PCM.
#[derive(Debug, PartialEq, Eq)]
pub struct CompleteSourceIdentity {
    pub frame_count: usize,
    pub channels: usize,
    pub sample_rate_hz: u32,
    pub original_sha256: [u8; 32],
    pub playback_sha256: [u8; 32],
    pub mono_sha256: [u8; 32],
    pub transform_sha256: [u8; 32],
    pub source_zero_frame: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidentContext {
    /// Both interpolation taps, including virtual P seams, wrap within these bounds.
    FiniteLoop,
    /// Native feed is additionally admitted against its actual source read coverage.
    KeyLockFiniteLoop,
    FullTrack,
    /// Native/FIFO continuation is preserved by an explicitly admitted full-track view.
    KeyLockFullTrack,
}

impl ResidentContext {
    pub(crate) fn permits_finite_range(self) -> bool {
        matches!(self, Self::FiniteLoop | Self::KeyLockFiniteLoop)
    }
}

#[derive(Debug, Clone)]
pub struct ResidentSourceView {
    pub source: Arc<CompleteSourceIdentity>,
    pub start_frame: usize,
    pub window_revision: u64,
    pub context: ResidentContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResidentBinding {
    pub(crate) address: usize,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) revision: u64,
    pub(crate) context: ResidentContext,
}

impl SampleBuffer {
    pub(crate) fn valid_residency(&self, rate: u32, channels: usize) -> bool {
        self.channels != 0
            && self.channels == channels
            && !self.samples.is_empty()
            && self.samples.len().is_multiple_of(channels)
            && self.residency.as_ref().is_none_or(|view| {
                view.source.channels == channels
                    && view.source.sample_rate_hz == rate
                    && view.source.source_zero_frame == 0
                    && view.window_revision != 0
                    && view.source.frame_count.checked_mul(channels).is_some()
                    && view.start_frame < self.resident_end()
                    && self.resident_end() <= view.source.frame_count
                    && (view.context.permits_finite_range()
                        || (view.start_frame == 0
                            && self.resident_end() == view.source.frame_count))
            })
    }
    pub(crate) fn resident_binding(&self) -> Option<ResidentBinding> {
        self.residency.as_ref().map(|view| ResidentBinding {
            address: self.samples.as_ptr() as usize,
            start: view.start_frame,
            end: self.resident_end(),
            revision: view.window_revision,
            context: view.context,
        })
    }
    pub(crate) fn source_address(&self) -> usize {
        self.residency
            .as_ref()
            .map_or(self.samples.as_ptr() as usize, |view| {
                Arc::as_ptr(&view.source) as usize
            })
    }

    pub(crate) fn source_sample_count(&self) -> usize {
        self.frame_count() * self.channels
    }

    pub(crate) fn frame_count(&self) -> usize {
        self.residency.as_ref().map_or_else(
            || self.samples.len() / self.channels.max(1),
            |view| view.source.frame_count,
        )
    }

    pub(crate) fn resident_start(&self) -> usize {
        self.residency.as_ref().map_or(0, |view| view.start_frame)
    }

    pub(crate) fn resident_end(&self) -> usize {
        self.resident_start() + self.samples.len() / self.channels.max(1)
    }

    pub(crate) fn window_revision(&self) -> u64 {
        self.residency
            .as_ref()
            .map_or(0, |view| view.window_revision)
    }

    pub(crate) fn same_source(&self, other: &Self) -> bool {
        self.source_address() == other.source_address()
            && self.source_sample_count() == other.source_sample_count()
            && self.channels == other.channels
    }

    pub(crate) fn same_window(&self, other: &Self) -> bool {
        self.same_source(other)
            && self.resident_start() == other.resident_start()
            && self.resident_end() == other.resident_end()
            && self.window_revision() == other.window_revision()
            && self.residency.as_ref().map(|view| view.context)
                == other.residency.as_ref().map(|view| view.context)
    }

    pub(crate) fn sample_at_absolute(&self, frame: usize, channel: usize) -> f32 {
        // Bounds are admitted outside RT and rechecked at publication. Fail closed
        // for an unavailable read rather than indexing another source coordinate.
        if channel >= self.channels {
            return 0.0;
        }
        frame
            .checked_sub(self.resident_start())
            .and_then(|local| local.checked_mul(self.channels))
            .and_then(|base| base.checked_add(channel))
            .and_then(|index| self.samples.get(index))
            .copied()
            .unwrap_or(0.0)
    }

    /// Copy a finite window off-thread; it never pins the complete PCM backing.
    pub(crate) fn window(
        &self,
        start: usize,
        end: usize,
        revision: u64,
        context: ResidentContext,
    ) -> Result<Self, String> {
        let view = self
            .residency
            .as_ref()
            .ok_or("complete source identity is required")?;
        if self.channels == 0
            || start >= end
            || start < self.resident_start()
            || end > self.resident_end()
            || revision == 0
            || (!context.permits_finite_range() && (start != 0 || end != self.frame_count()))
        {
            return Err("resident window is outside its admitted source/context".into());
        }
        let begin = (start - self.resident_start()) * self.channels;
        let finish = (end - self.resident_start()) * self.channels;
        Ok(Self {
            channels: self.channels,
            samples: if begin == 0 && finish == self.samples.len() {
                self.samples.clone()
            } else {
                Arc::from(&self.samples[begin..finish])
            },
            residency: Some(Arc::new(ResidentSourceView {
                source: view.source.clone(),
                start_frame: start,
                window_revision: revision,
                context,
            })),
        })
    }

    #[cfg(test)]
    pub(crate) fn with_complete_source(mut self, rate: u32) -> Self {
        self.residency = Some(Arc::new(ResidentSourceView {
            source: Arc::new(CompleteSourceIdentity {
                frame_count: self.frame_count(),
                channels: self.channels,
                sample_rate_hz: rate,
                original_sha256: [0; 32],
                playback_sha256: [0; 32],
                mono_sha256: [0; 32],
                transform_sha256: [0; 32],
                source_zero_frame: 0,
            }),
            start_frame: 0,
            window_revision: 1,
            context: ResidentContext::FullTrack,
        }));
        self
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedStemSet {
    /// One already accepted complete-set generation; window copies share this owner.
    pub(crate) complete_set_identity: Arc<[u8; 32]>,
    /// Effective native projection for these same-source PCM readers, refreshed
    /// only by successful callback timing adoption/clear. Never a raw revision.
    pub(crate) accepted_timing: Option<AcceptedTimingProjection>,
    pub reference_samples: Arc<[f32]>,
    /// Semantic source view retained with the reference PCM, including when
    /// component windows carry independently rebound scope ownership.
    pub(crate) reference_residency: Option<Arc<ResidentSourceView>>,
    pub publication: PreparedSourcePermit,
    pub source_version_hash: u64,
    pub sample_rate_hz: u32,
    pub channels: usize,
    pub frame_count: usize,
    pub available_mask: u8,
    pub stems: [SampleBuffer; STEM_BUFFER_COUNT],
}

impl PreparedStemSet {
    /// Crop all components and fullmix reference in one off-thread transaction.
    pub(crate) fn window_for(mut self, reference: &SampleBuffer) -> Result<Self, String> {
        if self.frame_count != reference.frame_count() || self.channels != reference.channels {
            return Err("complete stem/source geometry differs".into());
        }
        if let Some(view) = &reference.residency {
            for stem in &mut self.stems {
                if stem.residency.is_none() {
                    stem.residency = Some(Arc::new(ResidentSourceView {
                        source: view.source.clone(),
                        start_frame: 0,
                        window_revision: view.window_revision,
                        context: ResidentContext::FullTrack,
                    }));
                }
                *stem = stem.window(
                    reference.resident_start(),
                    reference.resident_end(),
                    view.window_revision,
                    view.context,
                )?;
            }
        }
        self.reference_samples = reference.samples.clone();
        self.reference_residency = reference.residency.clone();
        Ok(self)
    }
}

/// One prebuilt all-or-none callback payload, allocated by the bounded worker.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ResidentControlIntent {
    pub(crate) loop_region: Option<(usize, Option<usize>)>,
    // Only the original captured launch's readiness request carries its loop intent.
    // Independent loop edits remain source controls even when storage is reused.
    pub(crate) loop_is_launch_readiness: bool,
    pub(crate) seek_position_s: Option<f64>,
    pub(crate) key_lock: Option<bool>,
    pub(crate) key_lock_request_id: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResidentTransaction {
    pub(crate) id: usize,
    pub(crate) sample: SampleBuffer,
    pub(crate) stems: Option<PreparedStemSet>,
    pub(crate) binding: InputPadBinding,
    pub(crate) publication: PreparedSourcePermit,
    pub(crate) expected_window_revision: u64,
    pub(crate) intent: ResidentControlIntent,
    pub(crate) seek_pin: Option<crate::audio_engine::resident_seek::ResidentSeekPin>,
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
    PadPlayhead { id: usize, position_s: f64 },

    /// Fixed original content tags emitted at the effect, never reconstructed at dequeue.
    ContentFeedback {
        id: usize,
        owner: usize,
        cell: usize,
        lifetime: u64,
        transport_revision: u64,
        kind: u8,
        value: f64,
    },
    /// Original ordinary effect identity; the durable callback snapshot owns its state.
    OrdinaryTransportFeedback {
        id: usize,
        owner: usize,
        cell: usize,
        lifetime: u64,
        transport_revision: u64,
        observation_sequence: u64,
    },
}

#[pymethods]
impl AudioMessage {
    pub fn sample_id(&self) -> Option<usize> {
        match self {
            AudioMessage::SampleStarted { id } => Some(*id),
            AudioMessage::SampleStopped { id } => Some(*id),
            AudioMessage::PadPeak { id, peak: _ } => Some(*id),
            AudioMessage::PadPlayhead { id, position_s: _ } => Some(*id),
            AudioMessage::ContentFeedback { id, .. } => Some(*id),
            AudioMessage::OrdinaryTransportFeedback { id, .. } => Some(*id),
            AudioMessage::MasterPeak { peak: _ } => None,
            _ => None,
        }
    }

    pub fn pad_peak(&self) -> Option<f32> {
        match self {
            AudioMessage::PadPeak { id: _, peak } => Some(*peak),
            AudioMessage::ContentFeedback { kind: 3, value, .. } => Some(*value as f32),
            _ => None,
        }
    }

    pub fn master_peak(&self) -> Option<f32> {
        match self {
            AudioMessage::MasterPeak { peak } => Some(*peak),
            _ => None,
        }
    }

    pub fn pad_playhead(&self) -> Option<f64> {
        match self {
            AudioMessage::PadPlayhead { id: _, position_s } => Some(*position_s),
            AudioMessage::ContentFeedback { kind: 4, value, .. } => Some(*value),
            _ => None,
        }
    }

    pub fn feedback_kind(&self) -> Option<&'static str> {
        match self {
            Self::OrdinaryTransportFeedback { .. } => Some("ordinary_transport"),
            Self::ContentFeedback { kind: 1, .. } => Some("started"),
            Self::ContentFeedback { kind: 2, .. } => Some("stopped"),
            Self::ContentFeedback { kind: 3, .. } => Some("peak"),
            Self::ContentFeedback { kind: 4, .. } => Some("playhead"),
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
    pub phase_anchor_s: f64,
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
    SpeedAndMasterPeriod,
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
    #[cfg(test)]
    SetSpeed(f64),

    /// Set the current master BPM when BPM lock is enabled.
    #[cfg(test)]
    SetMasterBpm(f64),

    /// Set the authoritative output seconds per quarter without a BPM roundtrip.
    #[cfg(test)]
    SetMasterPeriod(f64),

    /// Admit a coupled speed/output period in one bounded callback message.
    #[cfg(test)]
    SetSpeedAndMasterPeriod { speed: f64, period_seconds: f64 },

    /// Original control identity survives coalescing and asynchronous rate preparation.
    SetRateIntent {
        revision: u64,
        speed: Option<f64>,
        period_seconds: Option<f64>,
    },

    /// Set per-pad BPM metadata.
    #[cfg(test)]
    SetPadBpm { id: usize, bpm: Option<f64> },

    /// Apply a legacy BPM edit and retire older precise timing in one callback effect.
    SetLegacyPadBpm {
        placement: Option<SlotSnapshot>,
        id: usize,
        bpm: Option<f64>,
        through_epoch: u64,
        expected_source_generation: Option<u64>,
    },

    /// Set per-pad Gain/Trim in dB.
    #[cfg(test)]
    SetPadGain { id: usize, gain_db: f32 },

    /// Apply only to the preparation epoch actually observed at control admission.
    SetPadGainGuarded {
        placement: crate::audio_engine::content_placement::SlotSnapshot,
        id: usize,
        gain_db: f32,
        expected_epoch: u64,
    },

    /// Set per-pad 3-band EQ gains in dB.
    #[cfg(test)]
    SetPadEq {
        id: usize,
        low_db: f32,
        mid_db: f32,
        high_db: f32,
    },

    /// An older queued EQ must never overwrite a newly copied complete image.
    SetPadEqGuarded {
        placement: crate::audio_engine::content_placement::SlotSnapshot,
        id: usize,
        low_db: f32,
        mid_db: f32,
        high_db: f32,
        expected_epoch: u64,
    },
}

#[cfg(test)]
impl ControlParameterMessage {
    pub(crate) fn key(&self) -> ControlParameterKey {
        match self {
            ControlParameterMessage::SetVolume(_) => ControlParameterKey::Volume,
            ControlParameterMessage::SetSpeed(_) => ControlParameterKey::Speed,
            ControlParameterMessage::SetRateIntent {
                speed,
                period_seconds,
                ..
            } => match (speed, period_seconds) {
                (Some(_), Some(_)) => ControlParameterKey::SpeedAndMasterPeriod,
                (Some(_), None) => ControlParameterKey::Speed,
                _ => ControlParameterKey::MasterBpm,
            },
            ControlParameterMessage::SetSpeedAndMasterPeriod { .. } => {
                ControlParameterKey::SpeedAndMasterPeriod
            }
            ControlParameterMessage::SetMasterBpm(_)
            | ControlParameterMessage::SetMasterPeriod(_) => ControlParameterKey::MasterBpm,
            ControlParameterMessage::SetPadBpm { id, bpm: _ }
            | ControlParameterMessage::SetLegacyPadBpm { id, .. } => {
                ControlParameterKey::PadBpm(*id)
            }
            ControlParameterMessage::SetPadGain { id, gain_db: _ }
            | ControlParameterMessage::SetPadGainGuarded { id, .. } => {
                ControlParameterKey::PadGain(*id)
            }
            ControlParameterMessage::SetPadEq {
                id,
                low_db: _,
                mid_db: _,
                high_db: _,
            }
            | ControlParameterMessage::SetPadEqGuarded { id, .. } => {
                ControlParameterKey::PadEq(*id)
            }
        }
    }
}

/// Unresolved capture placement intent; only the admitting callback supplies a clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixCaptureTarget {
    At(u64),
    NextAdmissible,
}

impl MixCaptureTarget {
    pub(crate) fn explicit_frame(self) -> Option<u64> {
        match self {
            Self::At(frame) => Some(frame),
            Self::NextAdmissible => None,
        }
    }

    pub(crate) fn mode(self) -> &'static str {
        match self {
            Self::At(_) => "explicit",
            Self::NextAdmissible => "next_admissible",
        }
    }

    /// The first unrendered frame of this actual admitting callback, without +1.
    pub(crate) fn resolve(self, callback_start_frame: u64) -> u64 {
        match self {
            Self::At(frame) => frame,
            Self::NextAdmissible => callback_start_frame,
        }
    }
}

/// Message that is emitted from the Python side.
#[derive(Debug, Clone)]
pub enum ControlMessage {
    /// The same control-prepared owner is claimed once on the output scheduler.
    StartMixCapture {
        publication: Arc<crate::audio_engine::mix_capture::publication::CapturePublication>,
        target_frame: MixCaptureTarget,
    },
    /// Session-fenced capture stop; independent of ordinary playback StopAll.
    StopMixCapture {
        session_id: u64,
        target_frame: MixCaptureTarget,
    },
    /// One complete control-allocated pitch/optional attack owner.
    PitchAction(Arc<crate::audio_engine::pitch_publication::PitchActionPublication>),
    PreparedStart(Arc<crate::audio_engine::ordinary_start::OrdinaryStartPublication>),
    CapturePadStartTarget(crate::audio_engine::pad_start::CapturedPadStart),
    CaptureGlobalStartTarget(crate::audio_engine::captured_global_start::CapturedGlobalStart),
    /// Used for testing message passing functionality.
    Ping(),

    /// Enable or disable BPM lock.
    SetBpmLock(bool),

    /// Enable or disable key lock (preserve pitch under tempo changes).
    SetKeyLock(bool),

    /// Enable or disable Key Lock for one pad.
    #[cfg(test)]
    SetPadKeyLock {
        id: usize,
        enabled: bool,
    },

    /// Saved intent for one captured empty slot incarnation; no loaded-source acknowledgement.
    SetPadKeyLockSavedIntent {
        placement: SlotSnapshot,
        enabled: bool,
    },

    /// Identity-bound mode command with its own actual callback acknowledgement.
    SetPadKeyLockRequest {
        id: usize,
        enabled: bool,
        request_id: u64,
        binding: InputPadBinding,
        source_generation: u64,
    },

    /// Set bounded per-pad beatgrid/downbeat timing metadata.
    #[cfg(test)]
    SetPadTimingMetadata {
        id: usize,
        metadata: PadTimingMetadata,
    },

    /// Apply a legacy origin edit and retire older precise timing atomically.
    SetLegacyPadTimingMetadata {
        placement: Option<SlotSnapshot>,
        id: usize,
        metadata: PadTimingMetadata,
        through_epoch: u64,
        expected_source_generation: Option<u64>,
    },

    /// Clear precise timing through this native publication epoch without erasing newer state.
    ClearPadConstantTiming {
        placement: Option<SlotSnapshot>,
        id: usize,
        through_epoch: u64,
        expected_source_generation: Option<u64>,
    },

    /// Coupled derived controls from current acknowledged timing.
    RefreshAcceptedTiming(
        std::sync::Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
    ),

    /// Adopt a fixed precise projection after actual current source and permit checks.
    PublishConstantTiming {
        id: usize,
        timing: PreparedConstantTiming,
    },

    /// Request transport downbeat anchoring from a selected playing pad.
    AnchorTransportPhaseFromPad {
        id: usize,
    },

    /// Arm one selected reference for one-time session grid bootstrap.
    BootstrapTransportFromPad {
        id: usize,
    },

    /// Set per-pad loop region in seconds.
    ///
    /// If `end_s` is None, the loop end defaults to the full sample length.
    SetPadLoopRegion {
        id: usize,
        start_s: f64,
        end_s: Option<f64>,
        expected_source_generation: Option<u64>,
    },

    /// Set Rust-side trigger quantization mode for future pad triggers.
    SetTriggerQuantization(TriggerQuantization),

    /// Publish a loaded sample into an audio-thread slot.

    ///
    /// # Parameters
    /// * `id` - Unique identifier for the sample slot (0..NUM_SAMPLES)
    /// * `sample` - Pre-decoded immutable sample buffer (shared handle)
    LoadSample {
        publication: Option<crate::audio_engine::input_runtime_binding::SourcePublication>,
        id: usize,
        sample: SampleBuffer,
    },

    /// Publish validated prepared stems into an audio-thread slot.
    ///
    /// The message carries bounded metadata plus shared immutable buffer handles. It must not
    /// contain file paths, Python objects, or copied full audio payloads.
    PublishPreparedStems {
        id: usize,
        stems: PreparedStemSet,
    },

    /// Select whether a pad renders from the full mix or all prepared stems.
    ///
    /// The source-version hash is used by all-stems mode to reject stale updates. Full-mix mode
    /// ignores the hash and always remains available.
    SetStemMixMode {
        id: usize,
        mode: StemMixMode,
        source_version_hash: u64,
    },

    /// Select full mix and release unneeded paired component residency off-callback.
    SetStemPairFullMix {
        id: usize,
        retired: [Option<PreparedStemSet>; 2],
    },

    /// Productive cold assignment; callback checks its fixed complete source fence.
    LoadColdSample {
        publication: crate::audio_engine::input_runtime_binding::SourcePublication,
        id: usize,
        sample: SampleBuffer,
        source_generation: u64,
        content_lifetime: u64,
        adoption: Arc<std::sync::atomic::AtomicU8>,
        epoch: Option<Arc<std::sync::atomic::AtomicU64>>,
        captured_epoch: u64,
        replace_assignment: bool,
        loop_region: Option<(usize, usize)>,
        resident_cancelled: Option<Arc<std::sync::atomic::AtomicBool>>,
        incoming_defaults: Option<IncomingLoadDefaults>,
        initial_pitch: crate::audio_engine::SourcePitchInitialization,
        initial_mix: Option<crate::audio_engine::SourceMixInitialization>,
    },

    /// Atomic complete content installation into one reserved empty target.
    PadCopy(Box<crate::audio_engine::pad_copy::PadCopyRtWork>),
    BankMutation(Arc<crate::audio_engine::pad_copy::bank::BankMutationPublication>),
    /// Read-only, nonce-bound fresh restored-bank verification on the same FIFO.
    VerifyRecoveredBank(Box<crate::audio_engine::pad_copy::bank::recovery::RecoveredBankProbe>),
    PadPair(Box<crate::audio_engine::pad_pair::PadPairRtWork>),
    HoldAction(Box<crate::audio_engine::hold_action::HoldActionRtWork>),
    RelocateResident(Box<ResidentTransaction>),
    CaptureResidentSeek(Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>),
    CommittedStemWork(Box<crate::audio_engine::committed_stem_residency::CommittedStemRtWork>),

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
    PlaySample {
        id: usize,
        volume: f32,
        received_at_ns: Option<u64>,
        expected_source_generation: Option<u64>,
    },

    /// Stop all active voices, then play a loaded sample as one audio-thread command.
    ///
    /// # Parameters
    /// * `id` - Identifier of the sample to play
    /// * `volume` - Playback volume (0.0 to 1.0)
    PlaySampleExclusive {
        id: usize,
        volume: f32,
        received_at_ns: Option<u64>,
        expected_source_generation: Option<u64>,
    },

    /// One guarded native MIDI loop/play effect, retained through quantized scheduling.
    TriggerInputPad {
        id: usize,
        start_s: f64,
        end_s: Option<f64>,
        exclusive: bool,
        binding: InputPadBinding,
        received_at_ns: u64,
        resident_control: Option<crate::audio_engine::resident_relocation::ResidentLaunchGuard>,
        launch_revision: u64,
    },

    /// Bounded immutable global effect; its storage retires outside realtime processing.
    GlobalPlaybackBatch(Arc<crate::audio_engine::global_playback_batch::GlobalPlaybackBatch>),

    /// Stop all active voices for a sample.
    ///
    /// # Parameters
    /// * `id` - Identifier of the sample to stop
    #[cfg(test)]
    StopSample {
        id: usize,
    },

    /// An admitted stop follows its captured content; an empty stop retains its fixed slot epoch.
    StopSampleForPlacement {
        placement: crate::audio_engine::content_placement::SlotSnapshot,
    },

    /// Stop all currently active voices.
    StopAll(),

    /// Pause playback of a sample without resetting position.
    ///
    /// If the sample is playing, its voice becomes silent but retains its
    /// current playback position. If the sample is not playing, this has no effect.
    #[cfg(test)]
    PauseSample {
        id: usize,
    },

    /// Resume playback of a paused sample from its saved position.
    ///
    /// If the sample was paused, playback continues from that point.
    /// If the sample was not paused, this has no effect.
    #[cfg(test)]
    ResumeSample {
        id: usize,
    },

    /// Seek an active or paused sample voice to a source position in seconds.
    ///
    /// If the sample has no active or paused voice, this has no effect.
    #[cfg(test)]
    SeekSample {
        id: usize,
        position_s: f64,
        expected_source_generation: Option<u64>,
    },

    /// Accepted transport intent retains its original content, across a later placement.
    TransportForPlacement {
        placement: crate::audio_engine::content_placement::SlotSnapshot,
        transport_revision: u64,
        source_epoch: u64,
        effect: TransportContentEffect,
    },

    /// Unload a sample slot.
    ///
    /// This stops all active voices for the sample and clears the sample buffer in the slot.
    ///
    /// # Parameters
    /// * `id` - Identifier of the sample slot to unload
    UnloadSample {
        publication: Option<crate::audio_engine::input_runtime_binding::SourcePublication>,
        id: usize,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum TransportContentEffect {
    Pause,
    Resume,
    Seek {
        position_s: f64,
        expected_source_generation: Option<u64>,
    },
}

#[cfg(test)]
impl ControlMessage {
    pub(crate) fn class(&self) -> ControlMessageClass {
        match self {
            ControlMessage::Ping() => ControlMessageClass::Test,
            ControlMessage::PlaySample { .. }
            | ControlMessage::StartMixCapture { .. }
            | ControlMessage::StopMixCapture { .. }
            | ControlMessage::PitchAction(_)
            | ControlMessage::PreparedStart(_)
            | ControlMessage::CapturePadStartTarget(_)
            | ControlMessage::CaptureGlobalStartTarget(_)
            | ControlMessage::PlaySampleExclusive { .. }
            | ControlMessage::TriggerInputPad { .. }
            | ControlMessage::GlobalPlaybackBatch(_)
            | ControlMessage::StopSample { .. }
            | ControlMessage::StopSampleForPlacement { .. }
            | ControlMessage::StopAll()
            | ControlMessage::PauseSample { .. }
            | ControlMessage::HoldAction(_)
            | ControlMessage::TransportForPlacement { .. }
            | ControlMessage::ResumeSample { .. }
            | ControlMessage::SeekSample { .. } => ControlMessageClass::PlaybackEvent,
            ControlMessage::LoadSample { .. }
            | ControlMessage::LoadColdSample { .. }
            | ControlMessage::PadCopy(_)
            | ControlMessage::BankMutation(_)
            | ControlMessage::VerifyRecoveredBank(_)
            | ControlMessage::PadPair(_)
            | ControlMessage::RelocateResident(_)
            | ControlMessage::CaptureResidentSeek(_)
            | ControlMessage::CommittedStemWork(_)
            | ControlMessage::PublishPreparedStems { .. }
            | ControlMessage::PublishConstantTiming { .. }
            | ControlMessage::RefreshAcceptedTiming(_) => ControlMessageClass::Publication,
            ControlMessage::SetBpmLock(_)
            | ControlMessage::SetKeyLock(_)
            | ControlMessage::SetPadKeyLock { .. }
            | ControlMessage::SetPadKeyLockSavedIntent { .. }
            | ControlMessage::SetPadKeyLockRequest { .. }
            | ControlMessage::SetPadTimingMetadata { .. }
            | ControlMessage::SetLegacyPadTimingMetadata { .. }
            | ControlMessage::ClearPadConstantTiming { .. }
            | ControlMessage::AnchorTransportPhaseFromPad { .. }
            | ControlMessage::BootstrapTransportFromPad { .. }
            | ControlMessage::SetPadLoopRegion { .. }
            | ControlMessage::SetTriggerQuantization(_)
            | ControlMessage::SetStemMixMode { .. }
            | ControlMessage::SetStemPairFullMix { .. }
            | ControlMessage::SetStemEnabledMask { .. }
            | ControlMessage::UnloadSample { .. } => ControlMessageClass::OrderedState,
        }
    }
}

/// Re-export from the analysis crate.
pub(crate) use flitzis_looper_analysis::SampleAnalysis;

/// Validated request-owned scalar defaults, installed with its source before ACK.
#[derive(Clone, Copy, Debug)]
pub(crate) struct IncomingLoadDefaults {
    pub sample_rate_hz: u32,
    pub revision: u64,
    pub key_lock: bool,
    pub start_frame: usize,
    pub end_frame: Option<usize>,
    pub period_seconds: Option<f64>,
    pub origin_frame: f64,
    pub loop_auto: bool,
    pub loop_bars: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackgroundTaskKind {
    Analysis,
    OfflineAnalysis,
    #[cfg(test)]
    StemGeneration,
}

pub fn task_to_str(task: BackgroundTaskKind) -> &'static str {
    match task {
        BackgroundTaskKind::Analysis => "analysis",
        BackgroundTaskKind::OfflineAnalysis => "offline_analysis",
        #[cfg(test)]
        BackgroundTaskKind::StemGeneration => "stem_generation",
    }
}

/// Events emitted from background work (loading and per-pad tasks).
#[derive(Debug, Clone)]
pub enum LoaderEvent {
    /// Original Native request custody; diagnostic ids never grant routing authority.
    NativeOwned {
        owner: crate::audio_engine::NativeAnalysisOwner,
        event: Box<LoaderEvent>,
    },
    /// Validated diagnostic components; never implicitly adopted into a saved grid.
    OfflineAnalysisCompleted {
        id: usize,
        request_id: u64,
        result_json: String,
    },
    /// Source-verified immutable data, never a current audio or project selection.
    SourceBeatmapCompleted {
        id: usize,
        request_id: u64,
        map: crate::audio_engine::SourceBeatmap,
        artifact: Option<crate::audio_engine::SourceBeatmapArtifact>,
    },
    /// Unverified original observations, never accepted geometry or durable selection.
    SourceBeatmapRawCompleted {
        id: usize,
        request_id: u64,
        artifact: crate::audio_engine::SourceBeatmapRawArtifact,
    },
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
        timing_epoch: Option<u64>,
        duration_s: f64,
        /// Frame before first finite near-zero tolerance crossing, in loaded-source seconds.
        /// Clamped at source frame zero; there is no fixed-duration pre-roll.
        /// Independent of beat/downbeat analysis; `None` means no physical suggestion.
        detected_loop_start_s: Option<f64>,
        cached_path: String,
        /// Pre-adoption owner reservation, promoted only by matching metadata.
        original_lease: Option<crate::audio_engine::ProjectAssetLease>,
        analysis: Option<SampleAnalysis>,
        /// Automatic analysis failed independently of successful source adoption.
        /// Delivered only for the matching source and timing request.
        analysis_error: Option<String>,
        incoming_defaults: Option<IncomingLoadDefaults>,
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
impl LoaderEvent {
    /// Raw worker fixtures inspect diagnostics only; control routing retains the envelope.
    pub(crate) fn into_test_payload(self) -> Self {
        match self {
            Self::NativeOwned { event, .. } => event.into_test_payload(),
            event => event,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_stop_is_classified_as_one_ordered_playback_event() {
        let ownership =
            crate::audio_engine::input_runtime_binding::InputRuntimeOwnership::default();
        let placement = ownership.slot_snapshot(3).unwrap();
        assert_eq!(
            ControlMessage::StopSampleForPlacement { placement }.class(),
            ControlMessageClass::PlaybackEvent
        );
    }

    #[test]
    fn seek_and_playhead_messages_preserve_long_frame_addresses() {
        let (mut producer, mut consumer) = rtrb::RingBuffer::new(1);
        for rate in [44_100_u32, 48_000, 96_000] {
            for duration in [600_u32, 1_800] {
                let first = (u64::from(rate) * u64::from(duration)) as usize;
                for frame in first..first + 16 {
                    let position_s = frame as f64 / f64::from(rate);
                    producer
                        .push(ControlMessage::SeekSample {
                            expected_source_generation: None,
                            id: 3,
                            position_s,
                        })
                        .unwrap();
                    let ControlMessage::SeekSample {
                        expected_source_generation: None,
                        id,
                        position_s: accepted,
                    } = consumer.pop().unwrap()
                    else {
                        panic!("seek message expected");
                    };
                    assert_eq!(id, 3);
                    assert_eq!(accepted, position_s);
                    let telemetry = AudioMessage::PadPlayhead {
                        id,
                        position_s: accepted,
                    };
                    assert_eq!(telemetry.sample_id(), Some(3));
                    assert_eq!(
                        (telemetry.pad_playhead().unwrap() * f64::from(rate)).round() as usize,
                        frame,
                    );
                }
            }
        }
    }

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
            expected_source_generation: None,
            id: 3,
            volume: 0.75,
            received_at_ns: None,
        };

        assert!(matches!(
            message,
            ControlMessage::PlaySampleExclusive { expected_source_generation: None, id: 3, volume, received_at_ns: None } if volume == 0.75
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
            residency: None,
            channels: 1,
            samples: Arc::from([0.0_f32, 0.0].as_slice()),
        };
        let stems = PreparedStemSet {
            complete_set_identity: std::sync::Arc::new([0; 32]),
            accepted_timing: None,
            reference_samples: buffer.samples.clone(),
            reference_residency: buffer.residency.clone(),
            publication: PreparedSourcePermit::unrestricted(),
            source_version_hash: 42,
            sample_rate_hz: 44_100,
            channels: 1,
            frame_count: 2,
            available_mask: 0b1111,
            stems: std::array::from_fn(|_| buffer.clone()),
        };
        let message = ControlMessage::PublishPreparedStems { id: 3, stems };

        assert!(matches!(
            message,
            ControlMessage::PublishPreparedStems {
                id: 3,
                stems: PreparedStemSet {
                    complete_set_identity: _,
                    accepted_timing: None,
                    source_version_hash: 42,
                    sample_rate_hz: 44_100,
                    channels: 1,
                    frame_count: 2,
                    available_mask: 0b1111,
                    stems: _,
                    ..
                }
            }
        ));
    }

    #[test]
    fn prepared_stem_publication_reports_ring_buffer_full() {
        let buffer = SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from([0.0_f32, 0.0].as_slice()),
        };
        let stems = PreparedStemSet {
            complete_set_identity: std::sync::Arc::new([0; 32]),
            accepted_timing: None,
            reference_samples: buffer.samples.clone(),
            reference_residency: buffer.residency.clone(),
            publication: PreparedSourcePermit::unrestricted(),
            source_version_hash: 42,
            sample_rate_hz: 44_100,
            channels: 1,
            frame_count: 2,
            available_mask: 0b1111,
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
            ControlMessage::PlaySample {
                expected_source_generation: None,
                id: 1,
                volume: 1.0,
                received_at_ns: None
            }
            .class(),
            ControlMessageClass::PlaybackEvent
        );
        assert_eq!(
            ControlMessage::SeekSample {
                expected_source_generation: None,
                id: 1,
                position_s: 2.5,
            }
            .class(),
            ControlMessageClass::PlaybackEvent
        );
        assert_eq!(
            ControlMessage::LoadSample {
                publication: None,
                id: 1,
                sample: SampleBuffer {
                    residency: None,
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
                expected_source_generation: None,
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
            ControlParameterMessage::SetMasterBpm(123.5).key(),
            ControlParameterMessage::SetMasterPeriod(0.500_000_001).key()
        );
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
