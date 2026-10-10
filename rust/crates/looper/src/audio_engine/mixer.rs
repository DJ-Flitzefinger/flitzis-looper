//! Real-time audio mixer implementation.
//!
//! This module provides the [`RtMixer`] struct which handles real-time mixing
//! of multiple audio voices with sample loading, playback, and mixing capabilities.
//!
//! The mixer manages a collection of [`VoiceSlot`](crate::audio_engine::voice_slot::VoiceSlot) instances
//! and operates on [`SampleBuffer`](crate::messages::SampleBuffer) data loaded via
//! [`decode_audio_file_to_sample_buffer`](crate::audio_engine::sample_loader::decode_audio_file_to_sample_buffer).

use crate::audio_engine::buffer_retirement::AudioBufferRetirement;
#[cfg(test)]
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::constant_timing::{
    AcceptedTimingProjection, CurrentTimingAcknowledgements, PreparedConstantTiming,
};
use crate::audio_engine::constants::{
    MAX_VOICES, NUM_SAMPLES, PAD_EQ_DB_MAX, PAD_EQ_DB_MIN, PAD_GAIN_DB_DEFAULT, PAD_GAIN_DB_MAX,
    PAD_GAIN_DB_MIN, PAD_GAIN_SMOOTH_MS, SPEED_MAX, SPEED_MIN, VOLUME_MAX, VOLUME_MIN,
};
use crate::audio_engine::dsp::{DspNodeSlot, DspParameterId, DspParameterSlot, PerPadDspChain};
use crate::audio_engine::key_lock_preparation::{
    KeyLockPreparationError, KeyLockPreparationWorker, create_key_lock_preparation,
};
use crate::audio_engine::native_history_permit::NativeHistoryContext;
use crate::audio_engine::native_source_coverage::normal_loop_feed_available;
use crate::audio_engine::productive_source_history::ProductiveSourceBinding;
use crate::audio_engine::source_grid::SourceGrid;
use crate::audio_engine::source_playback::SourcePlayback;
use crate::audio_engine::source_reader::{
    FrameRange, STEM_TRANSITION_RAMP_FRAMES, SourceReadPlan, StemRenderSelection, StemTransition,
    effective_loop_region, explicit_seek_mode_for_frame, prepared_stem_set_for_render,
    prepared_stem_set_matches_sample, resident_read_context_available,
};
#[cfg(test)]
use crate::audio_engine::source_reader::{
    full_stem_available_mask, render_source_sample, stem_index_mask,
};
use crate::audio_engine::stretch_processor::{DEFAULT_BLOCK_SAMPLES, ProductiveSourceFeed};
use crate::audio_engine::voice_slot::ExplicitSeekMode;
use crate::audio_engine::voice_slot::{
    FrozenStemView, VoiceSlot, VoiceSourceTiming, VoiceStartConfig,
};
#[cfg(test)]
use crate::messages::STEM_BUFFER_COUNT;
use crate::messages::{
    PadTimingMetadata, PreparedStemSet, STEM_COMPONENT_MASK, SampleBuffer, StemMixMode,
};
use cpal::Sample;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

#[path = "mixer_loop_acceptance.rs"]
mod loop_acceptance_snapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RtRenderPadActivity {
    ids: [usize; NUM_SAMPLES],
    seen: [bool; NUM_SAMPLES],
    count: usize,
}

impl Default for RtRenderPadActivity {
    fn default() -> Self {
        Self {
            ids: [0; NUM_SAMPLES],
            seen: [false; NUM_SAMPLES],
            count: 0,
        }
    }
}

impl RtRenderPadActivity {
    pub(crate) fn clear(&mut self) {
        for id in self.ids[..self.count].iter().copied() {
            self.seen[id] = false;
        }
        self.count = 0;
    }

    pub(crate) fn record(&mut self, id: usize) {
        if id >= NUM_SAMPLES || self.seen[id] {
            return;
        }
        if self.count < self.ids.len() {
            self.ids[self.count] = id;
            self.seen[id] = true;
            self.count += 1;
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.ids[..self.count].iter().copied()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.count
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, id: usize) -> bool {
        id < NUM_SAMPLES && self.seen[id]
    }
}

fn pad_eq_db_to_normalized(db: f32) -> f32 {
    if !db.is_finite() {
        return 0.5;
    }
    if db <= PAD_EQ_DB_MIN {
        return 0.0;
    }
    if db <= 0.0 {
        return 0.5 * 10.0_f32.powf(db / 20.0);
    }

    0.5 + 0.5 * (db / PAD_EQ_DB_MAX).clamp(0.0, 1.0)
}

fn gain_db_to_linear(gain_db: f32) -> f32 {
    10.0_f32.powf(gain_db.clamp(PAD_GAIN_DB_MIN, PAD_GAIN_DB_MAX) / 20.0)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SmoothedGain {
    current: f32,
    target: f32,
    step: f32,
    frames_remaining: usize,
}

impl Default for SmoothedGain {
    fn default() -> Self {
        let linear = gain_db_to_linear(PAD_GAIN_DB_DEFAULT);
        Self {
            current: linear,
            target: linear,
            step: 0.0,
            frames_remaining: 0,
        }
    }
}

impl SmoothedGain {
    fn set_target_db(&mut self, gain_db: f32, sample_rate_hz: f32, smooth: bool) {
        let target = gain_db_to_linear(gain_db);
        self.target = target;

        if !smooth || sample_rate_hz <= 0.0 {
            self.current = target;
            self.step = 0.0;
            self.frames_remaining = 0;
            return;
        }

        let smooth_frames = ((sample_rate_hz * PAD_GAIN_SMOOTH_MS) / 1000.0)
            .round()
            .max(1.0) as usize;
        self.step = (target - self.current) / smooth_frames as f32;
        self.frames_remaining = smooth_frames;
    }

    fn next(&mut self) -> f32 {
        if self.frames_remaining == 0 {
            return self.target;
        }

        self.current += self.step;
        self.frames_remaining -= 1;
        if self.frames_remaining == 0 {
            self.current = self.target;
            self.step = 0.0;
        }
        self.current
    }

    #[cfg(test)]
    fn current(&self) -> f32 {
        self.current
    }
}

/// Real-time mixer that handles sample loading and voice management.
///
/// The mixer maintains a sample bank with preloaded audio samples and manages
/// multiple concurrent voices for playback. All operations are designed to be
/// lock-free and real-time safe.
pub struct RtMixer {
    /// Number of output channels (1 for mono, 2 for stereo).
    channels: usize,

    /// Output sample rate in Hz.
    sample_rate_hz: f32,

    /// Global volume multiplier.
    volume: f32,

    /// Global speed multiplier.
    speed: f64,

    /// Enable BPM lock (tempo matching).
    bpm_lock_enabled: bool,
    global_parameters_drained: bool,

    /// Per-pad Key Lock state (preserve pitch when tempo changes).
    pad_key_lock_enabled: [bool; NUM_SAMPLES],

    /// Authoritative output seconds per quarter when BPM lock is enabled.
    master_period_seconds: Option<f64>,

    /// Legacy seconds per quarter, converted once on parameter admission.
    pad_period_seconds: [Option<f64>; NUM_SAMPLES],

    /// Per-pad musical phase anchor derived from bounded beatgrid/downbeat metadata.
    pad_phase_anchor_frame: [f64; NUM_SAMPLES],

    /// Fixed accepted binary64 source projection; legacy numbers stay separate.
    pad_accepted_timing: [Option<AcceptedTimingProjection>; NUM_SAMPLES],
    input_runtime_ownership: Arc<super::input_runtime_binding::InputRuntimeOwnership>,

    /// Fixed callback-owned publication epochs shared with the control resolver.
    current_timing_acknowledgements: Arc<CurrentTimingAcknowledgements>,
    /// Native preparation requests and cancellation share the control owner's monotonic epoch.
    prepared_source_epochs: Vec<Arc<AtomicU64>>,

    /// Per-pad Gain/Trim target in dB.
    pad_gain_db: [f32; NUM_SAMPLES],

    /// Per-pad smoothed linear Gain/Trim multiplier used by the render path.
    pad_gain_smoothers: [SmoothedGain; NUM_SAMPLES],

    /// Per-pad DSP/FX chain with the live DJ isolator EQ node.
    pad_dsp_chains: Box<[PerPadDspChain]>,

    /// Per-pad loop region start frame.
    pad_loop_start_frame: [usize; NUM_SAMPLES],

    /// Per-pad loop region end frame (exclusive), or None for full sample.
    pad_loop_end_frame: [Option<usize>; NUM_SAMPLES],

    /// Best-effort per-pad playhead frame from last render.
    pad_playhead_frame: [Option<usize>; NUM_SAMPLES],

    /// Sample storage with NUM_SAMPLES slots.
    sample_bank: [Option<SampleBuffer>; NUM_SAMPLES],

    /// Prepared stem storage with NUM_SAMPLES slots.
    prepared_stems: Box<[Option<PreparedStemSet>; NUM_SAMPLES]>,

    /// Per-pad stem render source selection.
    stem_mix_mode: [StemMixMode; NUM_SAMPLES],

    /// Source-version hash accepted for all-stems mode per pad.
    stem_mix_source_version_hash: [u64; NUM_SAMPLES],

    /// Per-pad enabled component-stem mask used when all-stems mode is active.
    stem_enabled_mask: [u8; NUM_SAMPLES],

    /// Per-pad bounded transition state for accepted stem source-selection changes.
    stem_transitions: [StemTransition; NUM_SAMPLES],
    release_pair_components: [bool; NUM_SAMPLES],

    /// Active voices with MAX_VOICES slots.
    pub voices: [VoiceSlot; MAX_VOICES],
    // Dropped after voices/lanes at stream teardown, outside callback rendering.
    _key_lock_preparation_worker: KeyLockPreparationWorker,
}

impl RtMixer {
    /// Creates a new RtMixer with the specified number of channels.
    ///
    /// # Parameters
    ///
    /// - `channels`: Number of output channels (1 for mono, 2 for stereo)
    ///
    /// # Returns
    ///
    /// A new `RtMixer` instance with empty sample bank and no active voices.
    #[cfg(test)]
    pub fn new(channels: usize, sample_rate_hz: f32) -> Self {
        Self::try_new(channels, sample_rate_hz).expect("failed to prepare test mixer")
    }

    /// Observe the actual adopted bank without manufacturing a control-side Ready state.
    #[cfg(test)]
    pub(super) fn bank_for_measurement(&self) -> &[Option<SampleBuffer>; NUM_SAMPLES] {
        &self.sample_bank
    }

    #[cfg(test)]
    pub(super) fn stems_for_measurement(&self) -> &[Option<PreparedStemSet>; NUM_SAMPLES] {
        &self.prepared_stems
    }

    #[cfg(test)]
    pub(super) fn stem_demand_for_measurement(&self, id: usize) -> (StemMixMode, u8, u64) {
        (
            self.stem_mix_mode[id],
            self.stem_enabled_mask[id],
            self.stem_mix_source_version_hash[id],
        )
    }

    #[cfg(test)]
    pub(super) fn key_lock_for_measurement(&self, id: usize) -> bool {
        self.pad_key_lock_enabled[id]
    }

    #[cfg(test)]
    pub(super) fn stem_transition_for_measurement(&self, id: usize) -> StemTransition {
        self.stem_transitions[id]
    }

    pub(crate) fn try_new(
        channels: usize,
        sample_rate_hz: f32,
    ) -> Result<Self, KeyLockPreparationError> {
        let sample_rate_hz = if sample_rate_hz.is_finite() && sample_rate_hz > 0.0 {
            sample_rate_hz
        } else {
            44_100.0
        };

        let preparation_rate = sample_rate_hz.round().clamp(8_000.0, u32::MAX as f32) as u32;
        let (lanes, worker) = create_key_lock_preparation(channels, preparation_rate, MAX_VOICES)?;
        let mut lanes = lanes.into_iter();

        Ok(Self {
            channels,
            sample_rate_hz,
            volume: VOLUME_MAX,
            speed: 1.0,
            bpm_lock_enabled: false,
            global_parameters_drained: true,
            pad_key_lock_enabled: std::array::from_fn(|_| false),
            master_period_seconds: None,
            pad_period_seconds: std::array::from_fn(|_| None),
            pad_phase_anchor_frame: std::array::from_fn(|_| 0.0),
            pad_accepted_timing: std::array::from_fn(|_| None),
            input_runtime_ownership: Arc::default(),
            current_timing_acknowledgements: Arc::default(),
            prepared_source_epochs: (0..NUM_SAMPLES).map(|_| Arc::default()).collect(),
            pad_gain_db: std::array::from_fn(|_| PAD_GAIN_DB_DEFAULT),
            pad_gain_smoothers: std::array::from_fn(|_| SmoothedGain::default()),
            pad_dsp_chains: (0..NUM_SAMPLES)
                .map(|id| PerPadDspChain::new(id, sample_rate_hz, channels))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            pad_loop_start_frame: std::array::from_fn(|_| 0),
            pad_loop_end_frame: std::array::from_fn(|_| None),
            pad_playhead_frame: std::array::from_fn(|_| None),
            sample_bank: std::array::from_fn(|_| None),
            prepared_stems: Box::new(std::array::from_fn(|_| None)),
            stem_mix_mode: std::array::from_fn(|_| StemMixMode::FullMix),
            stem_mix_source_version_hash: std::array::from_fn(|_| 0),
            stem_enabled_mask: std::array::from_fn(|_| STEM_COMPONENT_MASK),
            stem_transitions: std::array::from_fn(|_| StemTransition::default()),
            release_pair_components: [false; NUM_SAMPLES],
            voices: std::array::from_fn(|_| {
                VoiceSlot::with_preparation_lane(
                    channels,
                    lanes.next().expect("one lane per voice"),
                )
            }),
            _key_lock_preparation_worker: worker,
        })
    }

    pub(crate) fn set_current_timing_acknowledgements(
        &mut self,
        acknowledgements: Arc<CurrentTimingAcknowledgements>,
    ) {
        self.current_timing_acknowledgements = acknowledgements;
    }

    pub(crate) fn set_input_runtime_ownership(
        &mut self,
        ownership: Arc<super::input_runtime_binding::InputRuntimeOwnership>,
    ) {
        self.input_runtime_ownership = ownership;
    }

    pub(crate) fn set_prepared_source_epochs(&mut self, epochs: Vec<Arc<AtomicU64>>) {
        assert_eq!(epochs.len(), NUM_SAMPLES);
        self.prepared_source_epochs = epochs;
    }

    /// Fixed pointer/shape/full projection checks plus immediately published revocation.
    pub(crate) fn input_binding_current(
        &self,
        id: usize,
        binding: super::input_runtime_binding::InputPadBinding,
    ) -> bool {
        !self.input_runtime_ownership.resident_control_pending(id)
            && self.input_runtime_ownership.current(id, binding)
            && self.source_binding_current(id, binding)
            && self.input_runtime_ownership.current(id, binding)
    }

    pub(crate) fn set_global_parameters_drained(&mut self, drained: bool) {
        self.global_parameters_drained = drained;
    }

    pub(crate) fn accepted_refresh_master_matches(
        &self,
        binding: super::input_runtime_binding::InputPadBinding,
        period: f64,
    ) -> bool {
        self.global_parameters_drained
            && self.bpm_lock_enabled
            && binding.accepted.is_some_and(|accepted| {
                period.to_bits() == (accepted.period_seconds / self.speed).to_bits()
            })
    }

    /// Controller batches share source/authority/accepted guards without owning MIDI intent.
    pub(crate) fn source_binding_current(
        &self,
        id: usize,
        binding: super::input_runtime_binding::InputPadBinding,
    ) -> bool {
        self.input_runtime_ownership.authority_current(id, binding)
            && self
                .input_runtime_ownership
                .binding_source_current(id, binding)
            && binding.sample_rate_hz as f32 == self.sample_rate_hz
            && self.sample_bank[id].as_ref().is_some_and(|sample| {
                sample.source_address() == binding.source_address
                    && sample.source_sample_count() == binding.sample_count
                    && sample.channels == binding.channels
                    && sample.resident_binding() == binding.resident
            })
            && self.pad_accepted_timing[id] == binding.accepted
            && self.current_timing_acknowledgements.current_epoch(id)
                == binding
                    .accepted
                    .map_or(0, |accepted| accepted.publication_epoch)
            && self.input_runtime_ownership.authority_current(id, binding)
            && self
                .input_runtime_ownership
                .binding_source_current(id, binding)
    }

    /// Derived bank geometry cannot rewrite a still-active or paused previous-source pin.
    pub(crate) fn refresh_binding_current(
        &self,
        id: usize,
        binding: super::input_runtime_binding::InputPadBinding,
    ) -> bool {
        self.source_binding_current(id, binding)
            && self
                .voices
                .iter()
                .filter(|voice| voice.active && voice.sample_id == id)
                .all(|voice| {
                    voice.sample.as_ref().is_some_and(|sample| {
                        sample.source_address() == binding.source_address
                            && sample.source_sample_count() == binding.sample_count
                            && sample.channels == binding.channels
                            && sample.resident_binding() == binding.resident
                    }) && self.timing_for_voice(voice).accepted == binding.accepted
                })
            && self.source_binding_current(id, binding)
    }

    #[cfg(test)]
    pub(crate) fn loop_region_frames(&self, id: usize) -> (usize, Option<usize>) {
        (self.pad_loop_start_frame[id], self.pad_loop_end_frame[id])
    }

    pub(crate) fn set_loop_region_frames(&mut self, id: usize, start: usize, end: Option<usize>) {
        if id < NUM_SAMPLES && self.loop_context_available(id, start, end) {
            self.pad_loop_start_frame[id] = start;
            self.pad_loop_end_frame[id] = end;
        }
    }

    fn loop_context_available(&self, id: usize, start: usize, end: Option<usize>) -> bool {
        if self.finite_key_lock_voice_active(id) {
            return false;
        }
        let bank = self.sample_bank[id].as_ref();
        let available = |sample: &SampleBuffer| {
            effective_loop_region(start, end, sample.frame_count()).is_some_and(|region| {
                resident_read_context_available(
                    sample,
                    region,
                    ExplicitSeekMode::Normal,
                    self.pad_key_lock_enabled[id],
                )
            })
        };
        bank.is_none_or(available)
            && self
                .voices
                .iter()
                .filter(|voice| voice.active && voice.sample_id == id)
                .all(|voice| {
                    voice.sample.as_ref().is_some_and(|sample| {
                        if bank.is_some_and(|bank| sample.same_source(bank)) {
                            available(sample)
                        } else {
                            voice.source_loop_region.is_some_and(|region| {
                                resident_read_context_available(
                                    sample,
                                    region,
                                    voice.explicit_seek_mode,
                                    self.pad_key_lock_enabled[id],
                                )
                            })
                        }
                    })
                })
    }

    pub(crate) fn loop_intent_context_available(
        &self,
        id: usize,
        start_s: f64,
        end_s: Option<f64>,
    ) -> bool {
        if id >= NUM_SAMPLES
            || !start_s.is_finite()
            || start_s < 0.0
            || end_s.is_some_and(|end| !end.is_finite() || end < 0.0)
        {
            return false;
        }
        let start = (start_s * f64::from(self.sample_rate_hz)).round() as usize;
        let end = end_s.map(|end| {
            ((end * f64::from(self.sample_rate_hz)).round() as usize).max(start.saturating_add(1))
        });
        self.loop_context_available(id, start, end)
    }

    fn key_lock_context_available(&self, id: usize) -> bool {
        let full = |sample: &SampleBuffer| {
            sample.resident_start() == 0 && sample.resident_end() == sample.frame_count()
        };
        self.sample_bank[id].as_ref().is_none_or(|sample| {
            full(sample)
                || self
                    .effective_loop_region(id, sample.frame_count())
                    .is_some_and(|region| {
                        self.normal_key_lock_bank_available(
                            id,
                            sample,
                            self.prepared_stems[id].as_ref(),
                            region,
                        )
                    })
        }) && self
            .voices
            .iter()
            .filter(|voice| voice.active && voice.sample_id == id)
            .all(|voice| {
                voice.sample.as_ref().is_some_and(|sample| {
                    full(sample)
                        || (self.pad_key_lock_enabled[id]
                            && self.sample_bank[id]
                                .as_ref()
                                .is_some_and(|bank| bank.same_source(sample))
                            && !voice.paused
                            && self.normal_key_lock_voice_available(
                                id,
                                voice,
                                sample,
                                self.prepared_stems[id].as_ref(),
                            ))
                })
            })
    }

    fn normal_key_lock_bank_available(
        &self,
        id: usize,
        sample: &SampleBuffer,
        stems: Option<&PreparedStemSet>,
        region: FrameRange,
    ) -> bool {
        let timing = self.timing_for_sample_id(id);
        let mut playback = SourcePlayback::new(
            region.start,
            ExplicitSeekMode::Normal,
            self.tempo_ratio_for_sample_id(id),
        );
        playback.configure_domain(timing.loop_domain(sample.frame_count(), region));
        normal_loop_feed_available(
            sample,
            stems,
            self.sample_rate_hz as u32,
            timing.accepted,
            SourceReadPlan {
                channels: self.channels,
                sample_frames: sample.frame_count(),
                frame_pos: playback.position().frame,
                loop_region: region,
                loop_period: playback.loop_period(),
                seek_mode: ExplicitSeekMode::Normal,
                selection: self.stem_render_selection(id),
                transition: self.stem_transitions[id],
            },
            &playback,
        )
    }

    fn normal_key_lock_voice_available(
        &self,
        id: usize,
        voice: &VoiceSlot,
        sample: &SampleBuffer,
        stems: Option<&PreparedStemSet>,
    ) -> bool {
        self.normal_key_lock_voice_selection_available(
            id,
            voice,
            sample,
            stems,
            (self.stem_render_selection(id), self.stem_transitions[id]),
        )
    }

    fn normal_key_lock_voice_selection_available(
        &self,
        id: usize,
        voice: &VoiceSlot,
        sample: &SampleBuffer,
        stems: Option<&PreparedStemSet>,
        (selection, transition): (StemRenderSelection, StemTransition),
    ) -> bool {
        let playback = &voice.source_playback;
        let Some(region) = voice
            .source_loop_region
            .or_else(|| self.effective_loop_region(id, sample.frame_count()))
        else {
            return false;
        };
        let position = playback.position();
        normal_loop_feed_available(
            sample,
            stems,
            self.sample_rate_hz as u32,
            self.timing_for_voice(voice).accepted,
            SourceReadPlan {
                channels: self.channels,
                sample_frames: sample.frame_count(),
                frame_pos: position.frame,
                loop_region: region,
                loop_period: playback.loop_period(),
                seek_mode: position.seek_mode,
                selection,
                transition,
            },
            playback,
        )
    }

    /// Validate every live/paused pin against the exact source and effective projection.
    pub(crate) fn global_stop_bindings_current(
        &self,
        entries: &[super::global_playback_batch::GlobalPlaybackEntry],
    ) -> bool {
        entries
            .iter()
            .all(|entry| self.source_binding_current(entry.id, entry.binding))
            && self
                .voices
                .iter()
                .filter(|voice| voice.active)
                .all(|voice| {
                    entries
                        .iter()
                        .find(|entry| entry.id == voice.sample_id)
                        .is_some_and(|entry| {
                            voice.sample.as_ref().is_some_and(|sample| {
                                sample.source_address() == entry.binding.source_address
                                    && sample.source_sample_count() == entry.binding.sample_count
                                    && sample.channels == entry.binding.channels
                            }) && self.timing_for_voice(voice).accepted == entry.binding.accepted
                        })
                })
            && entries
                .iter()
                .all(|entry| self.source_binding_current(entry.id, entry.binding))
    }

    /// Loads a sample into the sample bank at the specified slot.
    ///
    /// # Parameters
    ///
    /// - `id`: Sample slot ID (0 to NUM_SAMPLES-1)
    /// - `sample`: Sample buffer to load
    ///
    /// # Safety
    ///
    /// The sample must have the same number of channels as the mixer.
    /// Invalid IDs are silently ignored.
    #[cfg(test)]
    pub(crate) fn load_sample(&mut self, id: usize, sample: SampleBuffer) {
        let mut retirement = ImmediateAudioBufferRetirement;
        self.load_sample_rt(id, sample, &mut retirement);
    }

    pub(crate) fn claim_cold(&self, id: usize, generation: u64) -> bool {
        self.input_runtime_ownership.claim_cold(id, generation)
    }
    pub(crate) fn reject_claimed_cold(&self, id: usize, generation: u64) {
        self.input_runtime_ownership
            .reject_claimed_cold(id, generation);
    }

    pub(crate) fn accept_cold(&self, id: usize, generation: u64) {
        self.input_runtime_ownership.accept_cold(id, generation);
    }
    pub(crate) fn cancel_cold(&self, id: usize, generation: u64) {
        if id < NUM_SAMPLES {
            self.input_runtime_ownership.cancel_cold(id, generation);
        }
    }

    pub(crate) fn cold_source_current(
        &self,
        id: usize,
        sample: &SampleBuffer,
        generation: u64,
    ) -> bool {
        id < NUM_SAMPLES
            && sample.valid_residency(self.sample_rate_hz as u32, self.channels)
            && (!self.pad_key_lock_enabled[id]
                || (sample.resident_start() == 0 && sample.resident_end() == sample.frame_count()))
            && self.input_runtime_ownership.source_generation(
                id,
                sample,
                self.sample_rate_hz as u32,
            ) == Some(generation)
    }

    pub(crate) fn load_sample_rt(
        &mut self,
        id: usize,
        sample: SampleBuffer,
        retirement: &mut impl AudioBufferRetirement,
    ) -> bool {
        if id >= NUM_SAMPLES {
            retirement.retire_sample(sample);
            return false;
        }

        if !sample.valid_residency(self.sample_rate_hz as u32, self.channels)
            || (self.pad_key_lock_enabled[id]
                && (sample.resident_start() != 0 || sample.resident_end() != sample.frame_count()))
        {
            retirement.retire_sample(sample);
            return false;
        }

        // A timing mutation and bank replacement can share a callback with no intervening
        // render (including paused voices). Freeze the old pin's latest effective timing first.
        let previous_timing = self.timing_for_sample_id(id);
        if let Some(previous) = self.sample_bank[id].as_ref()
            && !previous.same_source(&sample)
        {
            let previous_region = effective_loop_region(
                self.pad_loop_start_frame[id],
                self.pad_loop_end_frame[id],
                previous.frame_count(),
            );
            for voice in &mut self.voices {
                if voice.is_playing_sample(id)
                    && voice
                        .sample
                        .as_ref()
                        .is_some_and(|active| active.same_source(previous))
                {
                    voice.source_timing = previous_timing;
                    voice.source_loop_region = previous_region;
                    voice.retire_frozen_stems(retirement);
                    voice.frozen_stems = Some(FrozenStemView {
                        set: self.prepared_stems[id].clone(),
                        selection: StemRenderSelection::from_state(
                            self.stem_mix_mode[id],
                            self.stem_mix_source_version_hash[id],
                            self.stem_enabled_mask[id],
                        ),
                        transition: self.stem_transitions[id],
                    });
                }
            }
        }

        if let Some(old_sample) = self.sample_bank[id].take() {
            retirement.retire_sample(old_sample);
        }
        if let Some(old_stems) = self.prepared_stems[id].take() {
            retirement.retire_prepared_stems(old_stems);
        }

        self.sample_bank[id] = Some(sample);
        if let Some(sample) = &self.sample_bank[id]
            && sample
                .residency
                .as_ref()
                .is_some_and(|view| view.context == crate::messages::ResidentContext::FiniteLoop)
        {
            self.pad_loop_start_frame[id] = sample.resident_start();
            self.pad_loop_end_frame[id] = Some(sample.resident_end());
        }
        self.pad_accepted_timing[id] = None;
        self.current_timing_acknowledgements.clear(id);
        self.stem_enabled_mask[id] = STEM_COMPONENT_MASK;
        self.stem_transitions[id].clear();
        true
    }

    /// Replace immutable storage for the same effective source/complete StemSet.
    /// Voice clocks, native/FIFO/filter histories, selection and rate epochs stay owned.
    pub(crate) fn capture_resident_seek_rt(
        &mut self,
        capture: Arc<super::resident_seek::ResidentSeekCapture>,
        retirement: &mut impl AudioBufferRetirement,
        output_frame: u64,
    ) {
        let id = capture.id;
        if id >= NUM_SAMPLES
            || !capture.publication.current()
            || !self.source_binding_current(id, capture.binding)
        {
            capture.publication.cancel_unclaimed();
        } else if let Some((index, voice)) = self
            .voices
            .iter()
            .enumerate()
            .find(|(_, voice)| voice.active && voice.sample_id == id)
        {
            let pin = voice.sample.as_ref().and_then(|sample| {
                let region = voice
                    .source_loop_region
                    .or_else(|| self.effective_loop_region(id, sample.frame_count()))?;
                (voice.generation != u64::MAX).then(|| super::resident_seek::ResidentSeekPin {
                    sample: sample.clone(),
                    loop_region: region,
                    timing: self.timing_for_voice(voice),
                    voice_index: index,
                    generation: voice.generation,
                    key_lock: self.pad_key_lock_enabled[id],
                    stems: voice
                        .frozen_stems
                        .as_ref()
                        .and_then(|view| view.set.clone()),
                })
            });
            if let Some(pin) = pin {
                let target =
                    self.source_frame_from_seconds(capture.position_s, pin.sample.frame_count());
                let mode =
                    explicit_seek_mode_for_frame(target, pin.loop_region, pin.sample.frame_count());
                if resident_read_context_available(&pin.sample, pin.loop_region, mode, pin.key_lock)
                {
                    // Covered exact source addressing needs no new PCM. It still commits
                    // through this command's claimed, guarded native ACK.
                    if capture.publication.claim_resident_capture() {
                        if capture.publication.current()
                            && self.source_binding_current(id, capture.binding)
                            && self.seek_sample_with_output_frame(
                                id,
                                capture.position_s,
                                Some(output_frame),
                            )
                        {
                            capture
                                .publication
                                .record_resident_seek(self.pad_playhead_seconds(id));
                            capture.publication.mark_accepted_preserved_window();
                        } else {
                            capture.publication.mark_rejected();
                        }
                        self.input_runtime_ownership
                            .finish_resident_control(id, capture.publication.expected);
                    }
                    retirement.retire_sample(pin.sample);
                    if let Some(stems) = pin.stems {
                        retirement.retire_prepared_stems(stems);
                    }
                } else if let Some(pin) = capture.publish(pin) {
                    retirement.retire_sample(pin.sample);
                    if let Some(stems) = pin.stems {
                        retirement.retire_prepared_stems(stems);
                    }
                    capture.publication.cancel_unclaimed();
                }
            } else {
                capture.publication.cancel_unclaimed();
            }
        } else {
            // The pre-existing stopped seek contract is an acknowledged no-op.
            if capture.publication.claim_resident_capture() {
                if capture.publication.current() && self.source_binding_current(id, capture.binding)
                {
                    capture.publication.mark_accepted_preserved_window();
                } else {
                    capture.publication.mark_rejected();
                }
                self.input_runtime_ownership
                    .finish_resident_control(id, capture.publication.expected);
            }
        }
        retirement.retire_resident_capture(capture);
    }

    pub(crate) fn relocate_resident_at_output_frame(
        &mut self,
        transaction: Box<crate::messages::ResidentTransaction>,
        retirement: &mut impl AudioBufferRetirement,
        output_frame: u64,
    ) -> bool {
        self.relocate_resident_with_output_frame(transaction, retirement, Some(output_frame))
    }

    fn relocate_resident_with_output_frame(
        &mut self,
        transaction: Box<crate::messages::ResidentTransaction>,
        retirement: &mut impl AudioBufferRetirement,
        output_frame: Option<u64>,
    ) -> bool {
        let id = transaction.id;
        let sample = &transaction.sample;
        let stems = &transaction.stems;
        let binding = transaction.binding;
        let publication = &transaction.publication;
        let expected_window_revision = transaction.expected_window_revision;
        let intent = transaction.intent;
        if let Some(pin) = &transaction.seek_pin {
            let current = id < NUM_SAMPLES
                && intent.loop_region.is_none()
                && intent.key_lock.is_none()
                && intent
                    .seek_position_s
                    .is_some_and(|position| position.is_finite() && position >= 0.0)
                && publication.current()
                && self.source_binding_current(id, binding)
                && self.pad_key_lock_enabled[id] == pin.key_lock
                && sample.valid_residency(self.sample_rate_hz as u32, self.channels)
                && pin.sample.same_source(sample)
                && sample.resident_start() == 0
                && sample.resident_end() == sample.frame_count()
                && self
                    .voices
                    .iter()
                    .filter(|voice| voice.active && voice.sample_id == id)
                    .count()
                    == 1
                && match (pin.stems.as_ref(), stems.as_ref()) {
                    (None, None) => true,
                    (Some(captured), Some(next)) => {
                        Arc::ptr_eq(&captured.complete_set_identity, &next.complete_set_identity)
                            && next.accepted_timing == pin.timing.accepted
                            && prepared_stem_set_matches_sample(
                                next,
                                sample,
                                self.channels,
                                self.sample_rate_hz,
                                sample.frame_count(),
                            )
                    }
                    _ => false,
                }
                && self.voices.get(pin.voice_index).is_some_and(|voice| {
                    let timing = self.timing_for_voice(voice);
                    voice.active
                        && voice.sample_id == id
                        && voice.generation == pin.generation
                        && voice
                            .sample
                            .as_ref()
                            .is_some_and(|old| old.same_window(&pin.sample))
                        && voice.source_loop_region == Some(pin.loop_region)
                        && timing.accepted == pin.timing.accepted
                        && timing.legacy_period_seconds == pin.timing.legacy_period_seconds
                        && timing.legacy_origin_frame == pin.timing.legacy_origin_frame
                        && match (
                            voice
                                .frozen_stems
                                .as_ref()
                                .and_then(|view| view.set.as_ref()),
                            pin.stems.as_ref(),
                        ) {
                            (None, None) => true,
                            (Some(current), Some(captured)) => Arc::ptr_eq(
                                &current.complete_set_identity,
                                &captured.complete_set_identity,
                            ),
                            _ => false,
                        }
                });
            let accepted = current
                && publication.claim_resident()
                && publication.current()
                && self.source_binding_current(id, binding);
            if accepted {
                let voice = &mut self.voices[pin.voice_index];
                if let Some(old) = voice.sample.replace(sample.clone()) {
                    retirement.retire_sample(old);
                }
                if let Some(view) = &mut voice.frozen_stems {
                    if let Some(old) = view.set.take() {
                        retirement.retire_prepared_stems(old);
                    }
                    view.set = stems.clone();
                }
                let did_seek = self.seek_sample_with_output_frame(
                    id,
                    intent.seek_position_s.expect("validated seek pin"),
                    output_frame,
                );
                publication.record_resident_seek(
                    did_seek.then(|| self.pad_playhead_seconds(id)).flatten(),
                );
                publication.mark_accepted_preserved_window();
            } else {
                publication.mark_rejected();
            }
            if id < NUM_SAMPLES {
                self.input_runtime_ownership
                    .finish_resident_control(id, publication.expected);
            }
            retirement.retire_resident_transaction(transaction);
            return accepted;
        }
        let seek_only = intent.loop_region.is_none() && intent.key_lock.is_none();
        let previous_pin_seek = seek_only
            && intent
                .seek_position_s
                .is_some_and(|position| position.is_finite() && position >= 0.0)
            && id < NUM_SAMPLES
            && publication.current()
            && self.source_binding_current(id, binding)
            && sample.valid_residency(self.sample_rate_hz as u32, self.channels)
            && self.sample_bank[id].as_ref().is_some_and(|bank| {
                bank.same_source(sample) && bank.window_revision() == expected_window_revision
            })
            && self.voices.iter().any(|voice| {
                voice.active
                    && voice.sample_id == id
                    && voice
                        .sample
                        .as_ref()
                        .is_some_and(|old| !old.same_source(sample))
            })
            && self
                .voices
                .iter()
                .filter(|voice| voice.active && voice.sample_id == id)
                .all(|voice| {
                    voice.sample.as_ref().is_some_and(|old| {
                        old.resident_start() == 0 && old.resident_end() == old.frame_count()
                    })
                });
        if previous_pin_seek {
            let adopted = publication.claim_resident()
                && publication.current()
                && self.source_binding_current(id, binding)
                && self.seek_sample_with_output_frame(
                    id,
                    intent.seek_position_s.expect("validated seek"),
                    output_frame,
                );
            if adopted {
                publication.record_resident_seek(self.pad_playhead_seconds(id));
                publication.mark_accepted_preserved_window();
            } else {
                publication.mark_rejected();
            }
            self.input_runtime_ownership
                .finish_resident_control(id, publication.expected);
            retirement.retire_resident_transaction(transaction);
            return adopted;
        }
        let key_lock = intent
            .key_lock
            .unwrap_or_else(|| self.pad_key_lock_enabled.get(id).copied().unwrap_or(false));
        let loop_region = self
            .sample_bank
            .get(id)
            .and_then(Option::as_ref)
            .and_then(|old| {
                let (start, end) = intent
                    .loop_region
                    .unwrap_or((self.pad_loop_start_frame[id], self.pad_loop_end_frame[id]));
                effective_loop_region(start, end, old.frame_count())
            });
        let valid = id < NUM_SAMPLES
            && publication.current()
            && sample.valid_residency(self.sample_rate_hz as u32, self.channels)
            && self.source_binding_current(id, binding)
            && (!key_lock
                || (sample.resident_start() == 0 && sample.resident_end() == sample.frame_count())
                || loop_region.is_some_and(|region| {
                    (intent.key_lock == Some(true)
                        || self.sample_bank[id].as_ref().is_some_and(|bank| {
                            bank.residency.as_ref().is_some_and(|view| {
                                matches!(
                                    view.context,
                                    crate::messages::ResidentContext::KeyLockFiniteLoop
                                        | crate::messages::ResidentContext::KeyLockFullTrack
                                )
                            })
                        }))
                        && self.normal_key_lock_bank_available(id, sample, stems.as_ref(), region)
                        && self
                            .voices
                            .iter()
                            .filter(|voice| voice.active && voice.sample_id == id)
                            .all(|voice| {
                                // This first finite vertical admits stopped setup and storage-only
                                // current-source refresh. Broader edits/old voices remain guarded.
                                intent.seek_position_s.is_none()
                                    && intent.loop_region.is_none()
                                    && key_lock == self.pad_key_lock_enabled[id]
                                    && !voice.paused
                                    && voice
                                        .sample
                                        .as_ref()
                                        .is_some_and(|old| old.same_source(sample))
                                    && self.normal_key_lock_voice_available(
                                        id,
                                        voice,
                                        sample,
                                        stems.as_ref(),
                                    )
                            })
                }))
            && self.sample_bank[id].as_ref().is_some_and(|old| {
                old.same_source(sample)
                    && old.window_revision() == expected_window_revision
                    && (sample.window_revision() > expected_window_revision
                        || (intent.seek_position_s.is_none()
                            && old.same_window(sample)
                            && Arc::ptr_eq(&old.samples, &sample.samples)))
                    && loop_region.is_some_and(|region| {
                        resident_read_context_available(
                            sample,
                            region,
                            ExplicitSeekMode::Normal,
                            key_lock,
                        )
                    })
            })
            && intent.seek_position_s.is_none_or(|position| {
                position.is_finite()
                    && position >= 0.0
                    && sample.resident_start() == 0
                    && sample.resident_end() == sample.frame_count()
            })
            && match (&self.prepared_stems[id], stems) {
                (None, None) => true,
                (Some(old), Some(next)) => {
                    Arc::ptr_eq(&old.complete_set_identity, &next.complete_set_identity)
                        && (sample.window_revision() > expected_window_revision
                            || old
                                .stems
                                .iter()
                                .zip(&next.stems)
                                .all(|(old, next)| Arc::ptr_eq(&old.samples, &next.samples)))
                        && next.accepted_timing == self.pad_accepted_timing[id]
                        && prepared_stem_set_matches_sample(
                            next,
                            sample,
                            self.channels,
                            self.sample_rate_hz,
                            sample.frame_count(),
                        )
                }
                _ => false,
            }
            && self
                .voices
                .iter()
                .filter(|voice| voice.active && voice.sample_id == id)
                .all(|voice| {
                    voice.sample.as_ref().is_some_and(|old| {
                        if !old.same_source(sample) {
                            // A readiness change for the future bank does not
                            // adopt that bank into an older effective source.
                            // A later explicit native start retires its old pin.
                            return intent.seek_position_s.is_none()
                                && key_lock == self.pad_key_lock_enabled[id]
                                && voice.source_loop_region.is_some()
                                && voice.frozen_stems.is_some();
                        }
                        loop_region.is_some_and(|region| {
                            resident_read_context_available(
                                sample,
                                region,
                                if intent.loop_region.is_some() {
                                    ExplicitSeekMode::Normal
                                } else {
                                    voice.explicit_seek_mode
                                },
                                key_lock,
                            )
                        })
                    })
                });
        if !valid
            || !publication.claim_resident()
            || !publication.current()
            || !self.source_binding_current(id, binding)
        {
            publication.mark_rejected();
            if id < NUM_SAMPLES {
                self.input_runtime_ownership
                    .finish_resident_control(id, publication.expected);
            }
            retirement.retire_resident_transaction(transaction);
            return false;
        }
        for voice in &mut self.voices {
            if voice.active
                && voice.sample_id == id
                && voice
                    .sample
                    .as_ref()
                    .is_some_and(|old| old.same_source(sample))
                && let Some(old) = voice.sample.replace(sample.clone())
            {
                retirement.retire_sample(old);
            }
        }
        self.input_runtime_ownership.publish_window(id, sample);
        if let Some(old) = self.sample_bank[id].replace(sample.clone()) {
            retirement.retire_sample(old);
        }
        if let Some(old) = self.prepared_stems[id].take() {
            retirement.retire_prepared_stems(old);
        }
        self.prepared_stems[id] = stems.clone();
        if let Some((start, end)) = intent.loop_region {
            self.pad_loop_start_frame[id] = start;
            self.pad_loop_end_frame[id] = end;
            for voice in &mut self.voices {
                if voice.active
                    && voice.sample_id == id
                    && voice
                        .sample
                        .as_ref()
                        .is_some_and(|old| old.same_source(sample))
                {
                    voice.source_loop_region = None;
                    voice.clear_explicit_seek();
                }
            }
        }
        if let Some(enabled) = intent.key_lock
            && self.pad_key_lock_enabled[id] != enabled
        {
            self.pad_key_lock_enabled[id] = enabled;
            self.invalidate_prepared_for_pad(id);
        }
        if let Some(position) = intent.seek_position_s {
            let did_seek = self.seek_sample_with_output_frame(id, position, output_frame);
            publication
                .record_resident_seek(did_seek.then(|| self.pad_playhead_seconds(id)).flatten());
        }
        publication.mark_accepted();
        self.input_runtime_ownership
            .finish_resident_control(id, publication.expected);
        retirement.retire_resident_transaction(transaction);
        true
    }

    #[cfg(test)]
    pub(crate) fn publish_prepared_stems(&mut self, id: usize, stems: PreparedStemSet) -> bool {
        let mut retirement = ImmediateAudioBufferRetirement;
        self.publish_prepared_stems_rt(id, stems, &mut retirement)
    }

    pub(crate) fn publish_prepared_stems_rt(
        &mut self,
        id: usize,
        stems: PreparedStemSet,
        retirement: &mut impl AudioBufferRetirement,
    ) -> bool {
        if let Some(reason) = self.prepared_stem_rejection(id, &stems) {
            stems.publication.mark_rejected_reason(reason);
            retirement.retire_prepared_stems(stems);
            return false;
        }

        if let Some(old_stems) = self.prepared_stems[id].take() {
            retirement.retire_prepared_stems(old_stems);
        }

        stems.publication.mark_accepted();
        self.prepared_stems[id] = Some(stems);
        self.release_pair_components[id] = false;
        self.stem_transitions[id].clear();
        self.invalidate_prepared_for_pad(id);
        true
    }

    /// Bounded source/permit checks, fixed metadata assignment and off-callback retirement.
    pub(crate) fn publish_constant_timing_rt(
        &mut self,
        id: usize,
        timing: PreparedConstantTiming,
        retirement: &mut impl AudioBufferRetirement,
    ) -> bool {
        let projection = timing.projection;
        let valid = id < NUM_SAMPLES
            && timing.publication.current()
            && projection.sample_rate_hz as f32 == self.sample_rate_hz
            && SourceGrid::from_period(
                projection.period_seconds * f64::from(projection.sample_rate_hz),
                projection.origin_seconds * f64::from(projection.sample_rate_hz),
            )
            .is_some()
            && self.sample_bank[id].as_ref().is_some_and(|sample| {
                sample.channels == timing.reference.channels
                    && sample.same_source(&timing.reference)
            });
        if valid {
            self.invalidate_prepared_for_pad(id);
            self.pad_accepted_timing[id] = Some(projection);
            // PCM remains immutable and source-bound. Every retained stem reads
            // the newly adopted full revision through the same SourcePlayback.
            if let Some(stems) = &mut self.prepared_stems[id] {
                stems.accepted_timing = Some(projection);
            }
            self.current_timing_acknowledgements
                .acknowledge(id, projection.publication_epoch);
            timing.publication.mark_accepted();
        } else {
            timing.publication.mark_rejected();
        }
        retirement.retire_constant_timing(timing);
        valid
    }

    pub(crate) fn clear_constant_timing(&mut self, id: usize, through_epoch: u64) {
        if let Some(slot) = self.pad_accepted_timing.get_mut(id)
            && slot.is_some_and(|projection| projection.publication_epoch <= through_epoch)
        {
            *slot = None;
            if let Some(stems) = &mut self.prepared_stems[id] {
                stems.accepted_timing = None;
            }
            self.current_timing_acknowledgements.clear(id);
            self.invalidate_prepared_for_pad(id);
        }
    }

    pub(crate) fn set_stem_mix_mode(
        &mut self,
        id: usize,
        mode: StemMixMode,
        source_version_hash: u64,
    ) -> bool {
        if id >= NUM_SAMPLES {
            return false;
        }

        let previous = self.stem_render_selection(id);
        let next =
            StemRenderSelection::from_state(mode, source_version_hash, self.stem_enabled_mask[id]);
        if !self.stem_selection_context_available(id, previous, next) {
            return false;
        }
        match mode {
            StemMixMode::FullMix => {
                self.stem_mix_mode[id] = StemMixMode::FullMix;
                self.stem_mix_source_version_hash[id] = 0;
                let next = self.stem_render_selection(id);
                self.arm_stem_transition(id, previous, next);
                true
            }
            StemMixMode::AllStems => {
                let Some(stems) = self.prepared_stems[id].as_ref() else {
                    return false;
                };
                if source_version_hash == 0 || stems.source_version_hash != source_version_hash {
                    return false;
                }

                self.stem_mix_mode[id] = StemMixMode::AllStems;
                self.stem_mix_source_version_hash[id] = source_version_hash;
                let next = self.stem_render_selection(id);
                self.arm_stem_transition(id, previous, next);
                true
            }
        }
    }

    pub(crate) fn set_stem_pair_full_mix_rt(
        &mut self,
        id: usize,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        if self.set_stem_mix_mode(id, StemMixMode::FullMix, 0) {
            self.release_pair_components[id] = true;
            self.retire_unused_pair_components(retirement);
        }
    }

    fn retire_unused_pair_components(&mut self, retirement: &mut impl AudioBufferRetirement) {
        for id in 0..NUM_SAMPLES {
            if !self.release_pair_components[id]
                || self.stem_mix_mode[id] != StemMixMode::FullMix
                || (self.sample_is_active(id) && self.stem_transitions[id].is_active())
            {
                continue;
            }
            if self.prepared_stems[id].is_some() && retirement.available_retirement_slots() == 0 {
                return; // Keep the exact owner; retry in a later bounded render pass.
            }
            if let Some(stems) = self.prepared_stems[id].take() {
                retirement.retire_prepared_stems(stems);
            }
            // Keep the full-mix policy through an older window worker's return.
            // A new ordinary stem publication explicitly clears it above.
            self.stem_transitions[id].clear();
        }
    }

    pub(crate) fn set_stem_enabled_mask(
        &mut self,
        id: usize,
        enabled_stem_mask: u8,
        source_version_hash: u64,
    ) -> bool {
        if id >= NUM_SAMPLES || enabled_stem_mask & !STEM_COMPONENT_MASK != 0 {
            return false;
        }

        let Some(stems) = self.prepared_stems[id].as_ref() else {
            return false;
        };
        if source_version_hash == 0 || stems.source_version_hash != source_version_hash {
            return false;
        }
        let previous = self.stem_render_selection(id);
        let next = StemRenderSelection::from_state(
            self.stem_mix_mode[id],
            self.stem_mix_source_version_hash[id],
            enabled_stem_mask,
        );
        if !self.stem_selection_context_available(id, previous, next) {
            return false;
        }
        self.stem_enabled_mask[id] = enabled_stem_mask;
        let next = self.stem_render_selection(id);
        self.arm_stem_transition(id, previous, next);
        true
    }

    fn stem_render_selection(&self, id: usize) -> StemRenderSelection {
        if id >= NUM_SAMPLES {
            return StemRenderSelection::full_mix();
        }

        StemRenderSelection::from_state(
            self.stem_mix_mode[id],
            self.stem_mix_source_version_hash[id],
            self.stem_enabled_mask[id],
        )
    }

    fn arm_stem_transition(
        &mut self,
        id: usize,
        previous: StemRenderSelection,
        next: StemRenderSelection,
    ) {
        if id >= NUM_SAMPLES || previous == next {
            return;
        }

        self.invalidate_prepared_for_pad(id);

        if self.sample_is_active(id) {
            self.stem_transitions[id] =
                StemTransition::start(previous, STEM_TRANSITION_RAMP_FRAMES);
        } else {
            self.stem_transitions[id].clear();
        }
    }

    fn prepared_stem_rejection(
        &self,
        id: usize,
        stems: &PreparedStemSet,
    ) -> Option<super::prepared_source::PreparedStemRejection> {
        use super::prepared_source::PreparedStemRejection;
        if id >= NUM_SAMPLES || self.channels == 0 {
            return Some(PreparedStemRejection::InvalidGeometry);
        }
        if self.sample_is_active(id) {
            return Some(PreparedStemRejection::PadPlaying);
        }
        if let Some(reason) = stems.publication.current_rejection() {
            return Some(reason);
        }
        if stems.accepted_timing != self.pad_accepted_timing[id] {
            return Some(PreparedStemRejection::TimingChanged);
        }

        let Some(sample) = self.sample_bank[id].as_ref() else {
            return Some(PreparedStemRejection::SourceRequestChanged);
        };

        let sample_frames = sample.frame_count();
        (!prepared_stem_set_matches_sample(
            stems,
            sample,
            self.channels,
            self.sample_rate_hz,
            sample_frames,
        ))
        .then_some(PreparedStemRejection::WindowChanged)
    }

    fn sample_is_active(&self, id: usize) -> bool {
        self.voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == id)
    }

    fn finite_key_lock_voice_active(&self, id: usize) -> bool {
        self.pad_key_lock_enabled[id]
            && self.voices.iter().any(|voice| {
                voice.active
                    && voice.sample_id == id
                    && voice.sample.as_ref().is_some_and(|sample| {
                        !super::native_source_coverage::complete_view_available(sample)
                    })
            })
    }

    /// Scalar selection changes can reuse only the already installed set and the
    /// proved current-source NormalLoop trajectory. Publication/owner rules are unchanged.
    fn stem_selection_context_available(
        &self,
        id: usize,
        previous: StemRenderSelection,
        next: StemRenderSelection,
    ) -> bool {
        if previous == next || !self.pad_key_lock_enabled[id] {
            return true;
        }
        let transition = StemTransition::start(previous, STEM_TRANSITION_RAMP_FRAMES);
        self.voices
            .iter()
            .filter(|voice| voice.active && voice.sample_id == id)
            .all(|voice| {
                voice.sample.as_ref().is_some_and(|sample| {
                    super::native_source_coverage::complete_view_available(sample)
                        || (!voice.paused
                            && self.sample_bank[id]
                                .as_ref()
                                .is_some_and(|bank| bank.same_source(sample))
                            && self.input_runtime_ownership.source_current(
                                id,
                                sample,
                                self.sample_rate_hz as u32,
                            )
                            && self.input_runtime_ownership.source_timing_available(
                                id,
                                self.timing_for_voice(voice).accepted,
                                &self.current_timing_acknowledgements,
                            )
                            && self.normal_key_lock_voice_selection_available(
                                id,
                                voice,
                                sample,
                                self.prepared_stems[id].as_ref(),
                                (next, transition),
                            ))
                })
            })
    }

    pub(crate) fn can_play_sample(&self, id: usize, velocity: f32) -> bool {
        if id >= NUM_SAMPLES
            || !velocity.is_finite()
            || !(VOLUME_MIN..=VOLUME_MAX).contains(&velocity)
            || self.input_runtime_ownership.resident_control_pending(id)
        {
            return false;
        }
        let Some(sample) = self.sample_bank[id].as_ref() else {
            return false;
        };
        self.input_runtime_ownership
            .source_current(id, sample, self.sample_rate_hz as u32)
            && self
                .effective_loop_region(id, sample.frame_count())
                .is_some_and(|region| {
                    resident_read_context_available(
                        sample,
                        region,
                        ExplicitSeekMode::Normal,
                        self.pad_key_lock_enabled[id],
                    ) && (!self.pad_key_lock_enabled[id]
                        || (sample.resident_start() == 0
                            && sample.resident_end() == sample.frame_count())
                        || self.normal_key_lock_bank_available(
                            id,
                            sample,
                            self.prepared_stems[id].as_ref(),
                            region,
                        ))
                })
            && self.input_runtime_ownership.source_timing_available(
                id,
                self.pad_accepted_timing[id],
                &self.current_timing_acknowledgements,
            )
            && self
                .input_runtime_ownership
                .source_current(id, sample, self.sample_rate_hz as u32)
    }

    /// Compare the bounded stop revision without changing source or DSP ownership.
    pub(crate) fn launch_current(&self, id: usize, revision: u64) -> bool {
        self.input_runtime_ownership.launch_current(id, revision)
    }

    /// Reserve replaced PCM and any frozen stem pin before any launch/loop side effect.
    pub(crate) fn can_adopt_sample(
        &self,
        id: usize,
        retirement: &mut impl AudioBufferRetirement,
    ) -> bool {
        let Some(sample) = self.sample_bank.get(id).and_then(Option::as_ref) else {
            return false;
        };
        let needed = self
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(id))
            .map_or(0, |voice| {
                usize::from(
                    voice
                        .sample
                        .as_ref()
                        .is_some_and(|old| !old.same_source(sample)),
                ) + usize::from(
                    voice
                        .frozen_stems
                        .as_ref()
                        .is_some_and(|view| view.set.is_some()),
                )
            });
        needed == 0 || retirement.available_retirement_slots() >= needed
    }

    /// Starts playback of a loaded sample.
    ///
    /// # Parameters
    ///
    /// - `id`: Sample slot ID to play
    /// - `velocity`: Playback volume (0.0 to 1.0)
    ///
    /// If no free voice slot is available, the playback request is silently dropped.
    #[cfg(test)]
    pub(crate) fn play_sample(&mut self, id: usize, velocity: f32) -> bool {
        let mut retirement = ImmediateAudioBufferRetirement;
        self.play_sample_rt(id, velocity, &mut retirement)
    }

    #[cfg(test)]
    pub(crate) fn play_sample_rt(
        &mut self,
        id: usize,
        velocity: f32,
        retirement: &mut impl AudioBufferRetirement,
    ) -> bool {
        self.play_sample_with_phase_rt(id, velocity, None, None, retirement)
    }

    pub(crate) fn play_sample_at_output_frame_rt(
        &mut self,
        id: usize,
        velocity: f32,
        output_frame: u64,
        retirement: &mut impl AudioBufferRetirement,
    ) -> bool {
        self.play_sample_with_phase_rt(id, velocity, None, Some(output_frame), retirement)
    }

    #[cfg(test)]
    pub(crate) fn play_sample_at_output_frame(
        &mut self,
        id: usize,
        velocity: f32,
        output_frame: u64,
    ) -> bool {
        let mut retirement = ImmediateAudioBufferRetirement;
        self.play_sample_at_output_frame_rt(id, velocity, output_frame, &mut retirement)
    }

    #[cfg(test)]
    pub(crate) fn play_sample_phase_aligned(
        &mut self,
        id: usize,
        velocity: f32,
        target_master_beat: f64,
    ) -> bool {
        let mut retirement = ImmediateAudioBufferRetirement;
        self.play_sample_with_phase_rt(
            id,
            velocity,
            Some(target_master_beat),
            None,
            &mut retirement,
        )
    }

    fn play_sample_with_phase_rt(
        &mut self,
        id: usize,
        velocity: f32,
        target_master_beat: Option<f64>,
        start_output_frame: Option<u64>,
        retirement: &mut impl AudioBufferRetirement,
    ) -> bool {
        let Some(config) = self.prepare_sample_start(
            id,
            velocity,
            target_master_beat,
            start_output_frame,
            retirement,
        ) else {
            return false;
        };
        self.play_prepared_sample_rt(config, retirement)
    }

    pub(crate) fn prepare_play_sample_rt(
        &self,
        id: usize,
        velocity: f32,
        output_frame: u64,
        retirement: &mut impl AudioBufferRetirement,
    ) -> Option<VoiceStartConfig> {
        self.prepare_sample_start(id, velocity, None, Some(output_frame), retirement)
    }

    fn prepare_sample_start(
        &self,
        id: usize,
        velocity: f32,
        target_master_beat: Option<f64>,
        start_output_frame: Option<u64>,
        retirement: &mut impl AudioBufferRetirement,
    ) -> Option<VoiceStartConfig> {
        if id >= NUM_SAMPLES || !self.can_adopt_sample(id, retirement) {
            return None;
        }

        let sample = self.sample_bank[id].as_ref()?;
        let sample = sample.clone();

        let tempo_ratio = self.tempo_ratio_for_sample_id(id);
        let source_timing = self.timing_for_sample_id(id);

        let sample_frames = sample.frame_count();
        let initial_frame_pos = target_master_beat
            .map(|phase| self.phase_aligned_initial_sample_frame(id, sample_frames, phase))
            .unwrap_or_else(|| self.effective_loop_start_frame(id, sample_frames));

        // This is the admission point. No callback bank/source can change until this transaction
        // commits; a later control revocation cannot partially reject an admitted exclusive start.
        self.can_play_sample(id, velocity)
            .then_some(VoiceStartConfig {
                sample_id: id,
                sample,
                initial_frame_pos,
                volume: velocity,
                initial_tempo_ratio: tempo_ratio,
                start_output_frame,
                source_timing,
            })
    }

    pub(crate) fn apply_prepared_loop_region(
        &mut self,
        config: &mut VoiceStartConfig,
        start_s: f64,
        end_s: Option<f64>,
    ) {
        self.set_pad_loop_region(config.sample_id, start_s, end_s);
        config.initial_frame_pos =
            self.effective_loop_start_frame(config.sample_id, config.sample.frame_count());
    }

    pub(crate) fn play_prepared_sample_rt(
        &mut self,
        config: VoiceStartConfig,
        retirement: &mut impl AudioBufferRetirement,
    ) -> bool {
        let id = config.sample_id;

        // Sample is already playing? -> reset play position
        for voice_slot in &mut self.voices {
            if voice_slot.active && voice_slot.sample_id == id {
                self.stem_transitions[id].clear();
                // Retrigger is explicit even when its integer phase happens to match a loop edge.
                self.pad_dsp_chains[id].reset();
                if voice_slot
                    .sample
                    .as_ref()
                    .is_some_and(|old| old.same_source(&config.sample))
                {
                    voice_slot.source_timing = config.source_timing;
                    voice_slot.restart(
                        config.initial_frame_pos,
                        config.volume,
                        config.initial_tempo_ratio,
                        config.start_output_frame,
                        retirement,
                    );
                } else {
                    voice_slot.start_rt(config, retirement);
                }
                return true;
            }
        }

        // Start new voice slot
        for voice_slot in &mut self.voices {
            if !voice_slot.active {
                self.stem_transitions[id].clear();
                self.pad_dsp_chains[id].reset();
                voice_slot.start_rt(config, retirement);
                return true;
            }
        }

        // No free voice slot: drop deterministically.
        false
    }

    /// Sets the global volume multiplier.
    ///
    /// # Parameters
    ///
    /// - `volume`: Volume multiplier (0.0 to 1.0)
    ///
    /// Invalid values (NaN, infinite, or out of range) are silently ignored.
    pub fn set_volume(&mut self, volume: f32) {
        if !volume.is_finite() || !(VOLUME_MIN..=VOLUME_MAX).contains(&volume) {
            return;
        }

        self.volume = volume;
    }

    /// Sets the global speed multiplier.
    ///
    /// # Parameters
    ///
    /// - `speed`: Speed multiplier
    ///
    /// Invalid values (NaN, infinite, or out of range) are silently ignored.
    pub fn set_speed(&mut self, speed: f64) {
        if !speed.is_finite() || !(SPEED_MIN..=SPEED_MAX).contains(&speed) {
            return;
        }

        self.invalidate_prepared_for_all();
        self.speed = speed;
    }

    pub fn set_bpm_lock(&mut self, enabled: bool) {
        self.invalidate_prepared_for_all();
        self.bpm_lock_enabled = enabled;
        if !enabled {
            self.master_period_seconds = None;
        }
    }

    pub fn set_key_lock(&mut self, enabled: bool) {
        if enabled && (0..NUM_SAMPLES).any(|id| !self.key_lock_context_available(id)) {
            return;
        }
        self.invalidate_prepared_for_all();
        self.pad_key_lock_enabled.fill(enabled);
    }

    pub fn set_pad_key_lock(&mut self, id: usize, enabled: bool) {
        if id >= NUM_SAMPLES {
            return;
        }
        if enabled && !self.key_lock_context_available(id) {
            return;
        }

        self.pad_key_lock_enabled[id] = enabled;
        self.invalidate_prepared_for_pad(id);
    }

    #[cfg(test)]
    pub fn set_master_bpm(&mut self, bpm: f64) {
        if !bpm.is_finite() || bpm <= 0.0 {
            return;
        }

        self.set_master_period(60.0 / bpm);
    }

    pub(crate) fn set_master_period(&mut self, period: f64) {
        if period.is_finite() && period > 0.0 {
            self.invalidate_prepared_for_all();
            self.master_period_seconds = Some(period);
        }
    }

    pub fn set_pad_bpm(&mut self, id: usize, bpm: Option<f64>) {
        if id >= NUM_SAMPLES {
            return;
        }

        let bpm = bpm.and_then(|value| {
            if !value.is_finite() || value <= 0.0 {
                None
            } else {
                let period = 60.0 / value;
                (period.is_finite() && period > 0.0).then_some(period)
            }
        });

        self.pad_period_seconds[id] = bpm;
        self.invalidate_prepared_for_pad(id);
    }

    pub fn set_pad_timing_metadata(&mut self, id: usize, metadata: PadTimingMetadata) {
        if id >= NUM_SAMPLES {
            return;
        }

        let frame = metadata.phase_anchor_s * self.sample_rate_hz as f64;
        if frame.is_finite() {
            self.pad_phase_anchor_frame[id] = frame.round();
            self.invalidate_prepared_for_pad(id);
        }
    }

    fn invalidate_prepared_for_pad(&mut self, id: usize) {
        for voice in &mut self.voices {
            if voice.is_playing_sample(id) {
                voice.stretch.invalidate_prepared();
            }
        }
    }

    fn invalidate_prepared_for_all(&mut self) {
        for voice in &mut self.voices {
            voice.stretch.invalidate_prepared();
        }
    }

    pub(crate) fn phase_aligned_initial_sample_frame(
        &self,
        id: usize,
        sample_frames: usize,
        target_master_beat: f64,
    ) -> usize {
        if id >= NUM_SAMPLES {
            return 0;
        }

        let fallback_frame = self.effective_loop_start_frame(id, sample_frames);
        let Some(grid) = self.source_grid(id) else {
            return fallback_frame;
        };
        let Some(region) = self.phase_alignment_loop_region(id, sample_frames) else {
            return fallback_frame;
        };
        grid.source_at_master_beat(target_master_beat, region.start, region.end)
            .unwrap_or(fallback_frame)
    }

    fn source_grid(&self, id: usize) -> Option<SourceGrid> {
        self.timing_for_sample_id(id)
            .grid(f64::from(self.sample_rate_hz))
    }

    fn timing_for_sample_id(&self, id: usize) -> VoiceSourceTiming {
        VoiceSourceTiming {
            accepted: self.pad_accepted_timing[id],
            legacy_period_seconds: self.pad_period_seconds[id],
            legacy_origin_frame: self.pad_phase_anchor_frame[id],
        }
    }

    fn timing_for_voice(&self, voice: &VoiceSlot) -> VoiceSourceTiming {
        let current_source = voice
            .sample
            .as_ref()
            .zip(self.sample_bank[voice.sample_id].as_ref())
            .is_some_and(|(active, bank)| active.same_source(bank));
        if current_source {
            self.timing_for_sample_id(voice.sample_id)
        } else {
            voice.source_timing
        }
    }

    #[cfg(test)]
    fn pad_phase_anchor_frame(&self, id: usize) -> Option<f64> {
        self.pad_phase_anchor_frame.get(id).copied()
    }

    fn source_frame_from_seconds(&self, position_s: f64, sample_frames: usize) -> usize {
        if !position_s.is_finite() || position_s < 0.0 {
            return 0;
        }

        let frame = position_s * f64::from(self.sample_rate_hz);
        if !frame.is_finite() || frame <= 0.0 {
            return 0;
        }

        if frame >= sample_frames as f64 {
            return sample_frames;
        }

        frame.round() as usize
    }

    fn effective_loop_start_frame(&self, id: usize, sample_frames: usize) -> usize {
        self.effective_loop_region(id, sample_frames)
            .map(|region| region.start)
            .unwrap_or(0)
    }

    fn effective_loop_region(&self, id: usize, sample_frames: usize) -> Option<FrameRange> {
        if id >= NUM_SAMPLES || sample_frames == 0 {
            return None;
        }

        effective_loop_region(
            self.pad_loop_start_frame[id],
            self.pad_loop_end_frame[id],
            sample_frames,
        )
    }

    fn phase_alignment_loop_region(&self, id: usize, sample_frames: usize) -> Option<FrameRange> {
        if id >= NUM_SAMPLES || sample_frames == 0 {
            return None;
        }

        let start = self.pad_loop_start_frame[id];
        if start >= sample_frames {
            return None;
        }

        let end = self.pad_loop_end_frame[id]
            .unwrap_or(sample_frames)
            .min(sample_frames);
        if end <= start {
            return None;
        }

        Some(FrameRange { start, end })
    }

    pub fn set_pad_gain(&mut self, id: usize, gain_db: f32) {
        if id >= NUM_SAMPLES {
            return;
        }

        if !gain_db.is_finite() || !(PAD_GAIN_DB_MIN..=PAD_GAIN_DB_MAX).contains(&gain_db) {
            return;
        }

        self.pad_gain_db[id] = gain_db;
        let smooth = self.sample_is_active(id);
        self.pad_gain_smoothers[id].set_target_db(gain_db, self.sample_rate_hz, smooth);
    }

    pub fn set_pad_eq(&mut self, id: usize, low_db: f32, mid_db: f32, high_db: f32) {
        if id >= NUM_SAMPLES {
            return;
        }

        let all = [low_db, mid_db, high_db];
        if all
            .iter()
            .any(|v| !v.is_finite() || !(PAD_EQ_DB_MIN..=PAD_EQ_DB_MAX).contains(v))
        {
            return;
        }

        let low = pad_eq_db_to_normalized(low_db);
        let mid = pad_eq_db_to_normalized(mid_db);
        let high = pad_eq_db_to_normalized(high_db);

        self.set_pad_dsp_parameter(id, DspParameterSlot::Slot0, low);
        self.set_pad_dsp_parameter(id, DspParameterSlot::Slot1, mid);
        self.set_pad_dsp_parameter(id, DspParameterSlot::Slot2, high);

        if !self.sample_is_active(id) {
            self.pad_dsp_chains[id].reset();
        }
    }

    fn set_pad_dsp_parameter(
        &mut self,
        id: usize,
        slot: DspParameterSlot,
        normalized_target: f32,
    ) -> bool {
        let Some(parameter_id) = DspParameterId::per_pad(id, DspNodeSlot::Slot0, slot) else {
            return false;
        };

        self.pad_dsp_chains[id].set_parameter(parameter_id, normalized_target)
    }

    pub fn set_pad_loop_region(&mut self, id: usize, start_s: f64, end_s: Option<f64>) {
        if id >= NUM_SAMPLES {
            return;
        }

        if !start_s.is_finite() || start_s < 0.0 {
            return;
        }

        let start_frame = (start_s * self.sample_rate_hz as f64).round();
        let start_frame = if start_frame.is_finite() && start_frame >= 0.0 {
            start_frame as usize
        } else {
            0
        };

        let end_frame = end_s.and_then(|end_s| {
            if !end_s.is_finite() || end_s < 0.0 {
                return None;
            }
            let end_frame = (end_s * self.sample_rate_hz as f64).round();
            if !end_frame.is_finite() || end_frame < 0.0 {
                None
            } else {
                Some(end_frame as usize)
            }
        });

        let end_frame = if let Some(mut end) = end_frame {
            if end <= start_frame {
                end = start_frame.saturating_add(1);
            }
            Some(end)
        } else {
            None
        };

        if !self.loop_context_available(id, start_frame, end_frame) {
            return;
        }

        self.pad_loop_start_frame[id] = start_frame;
        self.pad_loop_end_frame[id] = end_frame;

        let bank = self.sample_bank[id].as_ref();
        for voice_slot in &mut self.voices {
            if voice_slot.is_playing_sample(id)
                && voice_slot
                    .sample
                    .as_ref()
                    .zip(bank)
                    .is_some_and(|(sample, bank)| sample.same_source(bank))
            {
                voice_slot.clear_explicit_seek();
            }
        }
    }

    #[cfg(test)]
    pub fn seek_sample(&mut self, id: usize, position_s: f64) -> bool {
        self.seek_sample_with_output_frame(id, position_s, None)
    }

    pub(crate) fn seek_sample_at_output_frame(
        &mut self,
        id: usize,
        position_s: f64,
        output_frame: u64,
    ) -> bool {
        self.seek_sample_with_output_frame(id, position_s, Some(output_frame))
    }

    fn seek_sample_with_output_frame(
        &mut self,
        id: usize,
        position_s: f64,
        output_frame: Option<u64>,
    ) -> bool {
        if id >= NUM_SAMPLES || !position_s.is_finite() || position_s < 0.0 || self.channels == 0 {
            return false;
        }

        // A successfully replaced bank can coexist with a retained old voice until retrigger.
        // Seek addresses that voice's immutable source extent, never the future launch source.
        let Some(voice) = self.voices.iter().find(|voice| voice.is_playing_sample(id)) else {
            return false;
        };
        let Some(sample) = voice.sample.as_ref() else {
            return false;
        };
        let sample_frames = sample.frame_count();
        if sample_frames == 0 {
            return false;
        }

        // Normal-loop finite native supply does not yet authorize a discontinuous
        // seek, even when the requested coordinate lies inside the held range.
        if self.pad_key_lock_enabled[id]
            && (sample.resident_start() != 0 || sample.resident_end() != sample_frames)
        {
            return false;
        }

        let Some(loop_region) = voice
            .source_loop_region
            .or_else(|| self.effective_loop_region(id, sample_frames))
        else {
            return false;
        };
        let target_frame = self.source_frame_from_seconds(position_s, sample_frames);
        let seek_mode = explicit_seek_mode_for_frame(target_frame, loop_region, sample_frames);
        if !resident_read_context_available(
            sample,
            loop_region,
            seek_mode,
            self.pad_key_lock_enabled[id],
        ) {
            return false;
        }

        // Explicit seek is a discontinuity even if it names the same exact current phase.
        self.pad_dsp_chains[id].reset();

        let mut did_seek = false;
        for voice_slot in &mut self.voices {
            if voice_slot.is_playing_sample(id) {
                voice_slot.seek(target_frame, seek_mode, output_frame);
                did_seek = true;
            }
        }

        if did_seek {
            self.pad_playhead_frame[id] = Some(target_frame);
        }

        did_seek
    }

    pub fn pad_playhead_seconds(&self, id: usize) -> Option<f64> {
        if id >= NUM_SAMPLES {
            return None;
        }
        let frame = self.pad_playhead_frame[id]?;
        Some(frame as f64 / f64::from(self.sample_rate_hz))
    }

    #[cfg(test)]
    pub(crate) fn active_pad_bar_phase_beats(&self, id: usize) -> Option<f64> {
        if id >= NUM_SAMPLES || self.channels == 0 {
            return None;
        }

        let voice = self
            .voices
            .iter()
            .find(|voice| voice.active && !voice.paused && voice.sample_id == id)?;
        let sample = voice.sample.as_ref()?;
        let sample_frames = sample.frame_count();

        if sample_frames == 0 || voice.frame_pos >= sample_frames {
            return None;
        }
        self.timing_for_voice(voice)
            .grid(f64::from(self.sample_rate_hz))?
            .bar_phase_at_source(voice.frame_pos as f64)
    }

    #[cfg(test)]
    pub(crate) fn output_bpm_for_sample_id(&self, id: usize) -> Option<f64> {
        self.output_period_for_sample_id(id)
            .map(|period| 60.0 / period)
    }

    fn source_period_for_sample_id(&self, id: usize) -> Option<f64> {
        if id >= NUM_SAMPLES {
            return None;
        }
        self.pad_accepted_timing[id]
            .map(|projection| projection.period_seconds)
            .or(self.pad_period_seconds[id])
    }

    fn output_period_for_sample_id(&self, id: usize) -> Option<f64> {
        let period = self.source_period_for_sample_id(id)? / self.tempo_ratio_for_sample_id(id);
        (period.is_finite() && period > 0.0).then_some(period)
    }

    pub(crate) fn transport_reference_period_for_sample_id(&self, id: usize) -> Option<f64> {
        if self.bpm_lock_enabled {
            // Rate clipping cannot redefine the requested master period.
            self.master_period_seconds
        } else {
            if let Some(voice) = self
                .voices
                .iter()
                .find(|voice| voice.active && voice.sample_id == id)
            {
                let source_period = self.timing_for_voice(voice).period_seconds()?;
                let period =
                    source_period / self.tempo_ratio_for_source_period(Some(source_period));
                (period.is_finite() && period > 0.0).then_some(period)
            } else {
                self.output_period_for_sample_id(id)
            }
        }
    }

    pub(crate) fn active_pad_beat_position(&self, id: usize) -> Option<f64> {
        if id >= NUM_SAMPLES || self.channels == 0 {
            return None;
        }
        let voice = self
            .voices
            .iter()
            .find(|voice| voice.active && !voice.paused && voice.sample_id == id)?;
        let sample_frames = voice.sample.as_ref()?.frame_count();
        let region = voice
            .source_loop_region
            .or_else(|| self.effective_loop_region(id, sample_frames))?;
        let mut playback = voice.source_playback;
        let timing = self.timing_for_voice(voice);
        playback.configure_domain(timing.loop_domain(sample_frames, region));
        let position = playback.position();
        timing
            .grid(f64::from(self.sample_rate_hz))?
            .beat_at_source(position.frame as f64 + position.fraction)
    }

    fn tempo_ratio_for_sample_id(&self, sample_id: usize) -> f64 {
        self.tempo_ratio_for_source_period(self.source_period_for_sample_id(sample_id))
    }

    fn tempo_ratio_for_source_period(&self, source_period: Option<f64>) -> f64 {
        let mut ratio = self.speed;

        if self.bpm_lock_enabled
            && let (Some(master_period), Some(source_period)) =
                (self.master_period_seconds, source_period)
        {
            ratio = source_period / master_period;
        }

        if !ratio.is_finite() {
            ratio = 1.0;
        }

        ratio.clamp(SPEED_MIN, SPEED_MAX)
    }

    /// Stops all voices playing a specific sample.
    ///
    /// # Parameters
    ///
    /// - `id`: Sample slot ID to stop
    #[cfg(test)]
    pub(crate) fn stop_sample(&mut self, id: usize) {
        let mut retirement = ImmediateAudioBufferRetirement;
        self.stop_sample_rt(id, &mut retirement);
    }

    pub(crate) fn stop_sample_rt(
        &mut self,
        id: usize,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        if id >= NUM_SAMPLES {
            return;
        }

        self.pad_dsp_chains[id].reset();

        for voice_slot in &mut self.voices {
            if voice_slot.is_playing_sample(id) {
                voice_slot.stop_rt(retirement);
            }
        }
    }

    /// Pause playback of a specific sample without resetting position.
    ///
    /// If the sample is playing, its voice becomes silent but retains its
    /// current playback position. If the sample is not playing, this has no effect.
    #[cfg(test)]
    pub fn pause_sample(&mut self, id: usize) {
        self.pause_sample_with_output_frame(id, None);
    }

    pub(crate) fn pause_sample_at_output_frame(&mut self, id: usize, output_frame: u64) {
        self.pause_sample_with_output_frame(id, Some(output_frame));
    }

    fn pause_sample_with_output_frame(&mut self, id: usize, _output_frame: Option<u64>) {
        if id >= NUM_SAMPLES {
            return;
        }

        for voice_slot in &mut self.voices {
            if voice_slot.is_playing_sample(id) {
                voice_slot.pause();
            }
        }
    }

    /// Resume playback of a paused sample from its saved position.
    ///
    /// If the sample was paused, playback continues from that point.
    /// If the sample was not paused, this has no effect.
    #[cfg(test)]
    pub fn resume_sample(&mut self, id: usize) {
        self.resume_sample_with_output_frame(id, None);
    }

    pub(crate) fn resume_sample_at_output_frame(&mut self, id: usize, output_frame: u64) {
        self.resume_sample_with_output_frame(id, Some(output_frame));
    }

    fn resume_sample_with_output_frame(&mut self, id: usize, _output_frame: Option<u64>) {
        if id >= NUM_SAMPLES {
            return;
        }

        for voice_slot in &mut self.voices {
            if voice_slot.is_playing_sample(id) {
                voice_slot.resume();
            }
        }
    }

    /// Unloads a sample from the sample bank.
    ///
    /// This stops all voices playing the sample and removes it from the bank.
    ///
    /// # Parameters
    ///
    /// - `id`: Sample slot ID to unload
    #[cfg(test)]
    pub(crate) fn unload_sample(&mut self, id: usize) {
        let mut retirement = ImmediateAudioBufferRetirement;
        self.unload_sample_rt(id, &mut retirement);
    }

    pub(crate) fn unload_sample_rt(
        &mut self,
        id: usize,
        retirement: &mut impl AudioBufferRetirement,
    ) -> bool {
        if id >= NUM_SAMPLES {
            return false;
        }

        self.stop_sample_rt(id, retirement);
        if let Some(sample) = self.sample_bank[id].take() {
            retirement.retire_sample(sample);
        }
        if let Some(stems) = self.prepared_stems[id].take() {
            retirement.retire_prepared_stems(stems);
        }
        self.stem_enabled_mask[id] = STEM_COMPONENT_MASK;
        self.stem_transitions[id].clear();
        self.pad_phase_anchor_frame[id] = 0.0;
        self.pad_accepted_timing[id] = None;
        self.current_timing_acknowledgements.clear(id);
        true
    }

    pub(crate) fn max_realtime_render_frames(&self) -> usize {
        (DEFAULT_BLOCK_SAMPLES / 2).max(1)
    }

    /// Renders audio frames to the output buffer.
    ///
    /// Mixes all active voices into the output buffer. The output buffer must
    /// contain interleaved audio samples with `channels` per frame.
    ///
    /// # Parameters
    ///
    /// - `output`: Output buffer to fill with mixed audio samples
    /// - `peaks`: Pad peaks
    #[cfg(test)]
    pub(crate) fn render(&mut self, output: &mut [f32], pad_peaks: &mut [f32; NUM_SAMPLES]) {
        let mut retirement = ImmediateAudioBufferRetirement;
        self.render_rt(output, pad_peaks, &mut retirement);
    }

    #[cfg(test)]
    pub(crate) fn render_at_output_frame(
        &mut self,
        output_start_frame: u64,
        output: &mut [f32],
        pad_peaks: &mut [f32; NUM_SAMPLES],
    ) {
        let mut retirement = ImmediateAudioBufferRetirement;
        self.render_rt_at_output_frame(output, pad_peaks, output_start_frame, &mut retirement);
    }

    #[cfg(test)]
    pub(crate) fn render_rt(
        &mut self,
        output: &mut [f32],
        pad_peaks: &mut [f32; NUM_SAMPLES],
        retirement: &mut impl AudioBufferRetirement,
    ) {
        let mut pad_activity = RtRenderPadActivity::default();
        self.render_rt_with_output_frame(output, pad_peaks, None, &mut pad_activity, retirement);
    }

    #[cfg(test)]
    pub(crate) fn render_rt_at_output_frame(
        &mut self,
        output: &mut [f32],
        pad_peaks: &mut [f32; NUM_SAMPLES],
        output_start_frame: u64,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        let mut pad_activity = RtRenderPadActivity::default();
        self.render_rt_at_output_frame_tracking_pads(
            output,
            pad_peaks,
            output_start_frame,
            &mut pad_activity,
            retirement,
        );
    }

    pub(crate) fn render_rt_at_output_frame_tracking_pads(
        &mut self,
        output: &mut [f32],
        pad_peaks: &mut [f32; NUM_SAMPLES],
        output_start_frame: u64,
        pad_activity: &mut RtRenderPadActivity,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        self.render_rt_with_output_frame(
            output,
            pad_peaks,
            Some(output_start_frame),
            pad_activity,
            retirement,
        );
    }

    fn render_rt_with_output_frame(
        &mut self,
        output: &mut [f32],
        pad_peaks: &mut [f32; NUM_SAMPLES],
        output_start_frame: Option<u64>,
        pad_activity: &mut RtRenderPadActivity,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        pad_peaks.fill(f32::EQUILIBRIUM);
        output.fill(Sample::EQUILIBRIUM);
        self.pad_playhead_frame.fill(None);

        if self.channels == 0 {
            return;
        }

        let frames = output.len() / self.channels;
        if frames == 0 {
            return;
        }

        let max_frames = self.max_realtime_render_frames();
        if frames > max_frames {
            let mut rendered_frames = 0;
            let mut chunk_peaks = [f32::EQUILIBRIUM; NUM_SAMPLES];

            while rendered_frames < frames {
                let chunk_frames = (frames - rendered_frames).min(max_frames);
                let start = rendered_frames * self.channels;
                let end = start + chunk_frames * self.channels;
                let chunk_output_start_frame =
                    output_start_frame.map(|frame| frame.saturating_add(rendered_frames as u64));

                self.render_rt_chunk(
                    &mut output[start..end],
                    &mut chunk_peaks,
                    chunk_output_start_frame,
                    pad_activity,
                    retirement,
                );
                for id in pad_activity.iter() {
                    pad_peaks[id] = pad_peaks[id].max(chunk_peaks[id]);
                }

                rendered_frames += chunk_frames;
            }

            return;
        }

        self.render_rt_chunk(
            output,
            pad_peaks,
            output_start_frame,
            pad_activity,
            retirement,
        );
    }

    fn render_rt_chunk(
        &mut self,
        output: &mut [f32],
        pad_peaks: &mut [f32; NUM_SAMPLES],
        output_start_frame: Option<u64>,
        pad_activity: &mut RtRenderPadActivity,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        pad_peaks.fill(f32::EQUILIBRIUM);

        if self.channels == 0 {
            return;
        }

        let frames = output.len() / self.channels;
        if frames == 0 {
            return;
        }

        let channels = self.channels;
        let sample_rate_hz = self.sample_rate_hz;
        let volume = self.volume;
        let pad_key_lock_enabled = &self.pad_key_lock_enabled;
        // Derive each target once before borrowing voices; start and render use
        // the same authoritative period/rate path.
        let voice_timings: [VoiceSourceTiming; MAX_VOICES] =
            std::array::from_fn(|index| self.timing_for_voice(&self.voices[index]));
        let tempo_ratios: [f64; MAX_VOICES] = std::array::from_fn(|index| {
            self.tempo_ratio_for_source_period(voice_timings[index].period_seconds())
        });
        let pad_gain_smoothers = &mut self.pad_gain_smoothers;
        let pad_dsp_chains = &mut self.pad_dsp_chains;
        let pad_loop_start_frame = &self.pad_loop_start_frame;
        let pad_loop_end_frame = &self.pad_loop_end_frame;
        let pad_playhead_frame = &mut self.pad_playhead_frame;
        let prepared_stem_slots = &self.prepared_stems;
        let stem_mix_mode = &self.stem_mix_mode;
        let stem_mix_source_version_hash = &self.stem_mix_source_version_hash;
        let stem_enabled_mask = &self.stem_enabled_mask;
        let stem_transitions = &mut self.stem_transitions;
        let bank_sources = &self.sample_bank;
        let ownership = &self.input_runtime_ownership;
        let acknowledgements = &self.current_timing_acknowledgements;
        let prepared_source_epochs = &self.prepared_source_epochs;

        for (voice_index, voice) in self.voices.iter_mut().enumerate() {
            if !voice.active {
                // An in-flight worker may finish after stop/unload. Drain its fenced owner even
                // when no voice renders again; all large destruction remains on the worker.
                voice.stretch.retire_prepared();
                continue;
            }
            pad_activity.record(voice.sample_id);

            let is_paused = voice.paused;
            if is_paused {
                voice.stretch.retire_prepared();
            }

            let Some(sample) = voice.sample.clone() else {
                pad_dsp_chains[voice.sample_id].reset();
                voice.stop_rt(retirement);
                continue;
            };
            voice.source_timing = voice_timings[voice_index];

            if !is_paused {
                let sample_frames = sample.frame_count();
                if sample_frames == 0 {
                    pad_dsp_chains[voice.sample_id].reset();
                    voice.stop_rt(retirement);
                    continue;
                }
                let frozen = voice.frozen_stems.as_ref().filter(|_| {
                    !bank_sources[voice.sample_id]
                        .as_ref()
                        .is_some_and(|bank| bank.same_source(&sample))
                });
                let prepared_stem_set = prepared_stem_set_for_render(
                    frozen.map_or_else(
                        || prepared_stem_slots[voice.sample_id].as_ref(),
                        |view| view.set.as_ref(),
                    ),
                    &sample,
                    channels,
                    sample_rate_hz,
                    sample_frames,
                    voice.source_timing.accepted,
                );
                let current_selection = frozen.map_or_else(
                    || {
                        StemRenderSelection::from_state(
                            stem_mix_mode[voice.sample_id],
                            stem_mix_source_version_hash[voice.sample_id],
                            stem_enabled_mask[voice.sample_id],
                        )
                    },
                    |view| view.selection,
                );
                let uses_frozen_stems = frozen.is_some();
                let mut source_transition =
                    frozen.map_or(stem_transitions[voice.sample_id], |view| view.transition);

                voice.source_playback.set_target(tempo_ratios[voice_index]);

                let Some(loop_region) = voice.source_loop_region.or_else(|| {
                    effective_loop_region(
                        pad_loop_start_frame[voice.sample_id],
                        pad_loop_end_frame[voice.sample_id],
                        sample_frames,
                    )
                }) else {
                    pad_dsp_chains[voice.sample_id].reset();
                    voice.stop_rt(retirement);
                    continue;
                };
                voice
                    .source_playback
                    .configure_domain(voice.source_timing.loop_domain(sample_frames, loop_region));
                let mut rendered = 0;
                // Each iteration consumes output frames and crosses at most one fixed rate step.
                // The enclosing render chunk is bounded by max_realtime_render_frames().
                while rendered < frames {
                    let output_frame =
                        output_start_frame.map(|frame| frame.saturating_add(rendered as u64));
                    let remaining = output_frame.map_or(frames - rendered, |frame| {
                        voice
                            .stretch
                            .chunk_until_prepared_adoption(frame, frames - rendered)
                    });
                    let (chunk_frames, tempo_ratio) = voice.source_playback.chunk(remaining);
                    let position = voice.source_playback.position();
                    let source_plan = SourceReadPlan {
                        channels,
                        sample_frames,
                        frame_pos: position.frame,
                        loop_region,
                        loop_period: voice.source_playback.loop_period(),
                        seek_mode: position.seek_mode,
                        selection: current_selection,
                        transition: source_transition,
                    };
                    let next_position = voice.source_playback.position_at(chunk_frames);
                    let context = NativeHistoryContext {
                        id: voice.sample_id,
                        ownership,
                        acknowledgements,
                        preparation_epoch: &prepared_source_epochs[voice.sample_id],
                    };
                    let permit = bank_sources[voice.sample_id]
                        .as_ref()
                        .filter(|bank| bank.same_source(&sample))
                        .and_then(|_| {
                            context.capture(
                                &sample,
                                sample_rate_hz as u32,
                                voice.source_timing.accepted,
                            )
                        });
                    voice.stretch.process_source(
                        ProductiveSourceFeed {
                            sample: &sample,
                            stems: prepared_stem_set,
                            sample_rate_hz: sample_rate_hz as u32,
                            accepted: voice.source_timing.accepted,
                            plan: source_plan,
                            playback: &voice.source_playback,
                            permit: permit.as_ref(),
                            output_frame,
                        },
                        chunk_frames,
                        pad_key_lock_enabled[voice.sample_id],
                    );
                    source_transition.advance_fractional(chunk_frames as f64 * tempo_ratio);
                    let pad_dsp_chain = &mut pad_dsp_chains[voice.sample_id];
                    pad_dsp_chain.bind_source(
                        ProductiveSourceBinding::new(
                            &sample,
                            sample_rate_hz as u32,
                            voice.source_timing.accepted,
                        ),
                        position,
                        next_position,
                        chunk_frames,
                        Some(source_plan.domain()),
                    );
                    let pad_gain_smoother = &mut pad_gain_smoothers[voice.sample_id];
                    let output_buffers = voice.stretch.output_buffers();
                    for frame in 0..chunk_frames {
                        let out_base = (rendered + frame) * channels;
                        let trim_gain = pad_gain_smoother.next();
                        pad_dsp_chain.begin_frame();
                        for (channel, buffer) in output_buffers.iter().enumerate().take(channels) {
                            let sample = buffer[frame] * trim_gain;
                            let sample = pad_dsp_chain.process_sample(channel, sample);
                            let contribution = sample * voice.volume;
                            output[out_base + channel] += contribution * volume;
                            pad_peaks[voice.sample_id] =
                                pad_peaks[voice.sample_id].max(contribution.abs());
                        }
                    }
                    voice.source_playback.advance(chunk_frames);
                    rendered += chunk_frames;
                }
                let position = voice.source_playback.position();
                voice.frame_pos = position.frame;
                voice.explicit_seek_mode = position.seek_mode;
                if uses_frozen_stems {
                    if let Some(view) = &mut voice.frozen_stems {
                        view.transition = source_transition;
                    }
                } else {
                    stem_transitions[voice.sample_id] = source_transition;
                }
            } else {
                if let Some(region) = voice.source_loop_region.or_else(|| {
                    effective_loop_region(
                        pad_loop_start_frame[voice.sample_id],
                        pad_loop_end_frame[voice.sample_id],
                        sample.frame_count(),
                    )
                }) {
                    voice.source_playback.configure_domain(
                        voice
                            .source_timing
                            .loop_domain(sample.frame_count(), region),
                    );
                    let position = voice.source_playback.position();
                    voice.frame_pos = position.frame;
                    voice.explicit_seek_mode = position.seek_mode;
                }
            }
            pad_playhead_frame[voice.sample_id] = Some(voice.frame_pos);
        }
        self.retire_unused_pair_components(retirement);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::messages::{STEM_MASK_BASS, STEM_MASK_DRUMS, STEM_MASK_MELODY, STEM_MASK_VOCALS};

    use super::*;

    fn create_test_sample(channels: usize, frames: usize, value: f32) -> SampleBuffer {
        let samples = vec![value; channels * frames];
        SampleBuffer {
            residency: None,
            channels,
            samples: Arc::from(samples.into_boxed_slice()),
        }
    }

    fn create_sine_sample(sample_rate_hz: f32, frames: usize, frequency_hz: f32) -> SampleBuffer {
        let samples: Vec<f32> = (0..frames)
            .map(|frame| {
                (frame as f32 * frequency_hz * std::f32::consts::TAU / sample_rate_hz).sin()
            })
            .collect();

        SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from(samples.into_boxed_slice()),
        }
    }

    fn create_sine_prepared_stems(
        reference: &SampleBuffer,
        sample_rate_hz: u32,
        frames: usize,
        frequency_hz: f32,
    ) -> PreparedStemSet {
        let source = create_sine_sample(sample_rate_hz as f32, frames, frequency_hz);
        let silence = create_test_sample(1, frames, 0.0);

        PreparedStemSet {
            complete_set_identity: std::sync::Arc::new([0; 32]),
            accepted_timing: None,
            reference_samples: reference.samples.clone(),
            publication: crate::audio_engine::prepared_source::PreparedSourcePermit::unrestricted(),
            source_version_hash: 42,
            sample_rate_hz,
            channels: 1,
            frame_count: frames,
            available_mask: full_stem_available_mask(),
            stems: [source, silence.clone(), silence.clone(), silence],
        }
    }

    fn create_frame_number_sample(frames: usize) -> SampleBuffer {
        SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from(
                (0..frames)
                    .map(|frame| frame as f32)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
        }
    }

    fn rms(samples: &[f32]) -> f32 {
        let sum = samples.iter().map(|sample| sample * sample).sum::<f32>();
        (sum / samples.len() as f32).sqrt()
    }

    fn estimate_frequency(samples: &[f32], sample_rate_hz: f32) -> f32 {
        let mut crossings = Vec::new();
        for index in 1..samples.len() {
            let previous = samples[index - 1];
            let current = samples[index];
            if previous <= 0.0 && current > 0.0 {
                let denom = current - previous;
                let frac = if denom.abs() > f32::EPSILON {
                    -previous / denom
                } else {
                    0.0
                };
                crossings.push(index as f32 - 1.0 + frac);
            }
        }

        if crossings.len() < 2 {
            return 0.0;
        }

        let span = crossings[crossings.len() - 1] - crossings[0];
        if span <= 0.0 {
            return 0.0;
        }

        (crossings.len() - 1) as f32 * sample_rate_hz / span
    }

    fn render_chunks(mixer: &mut RtMixer, chunks: usize, frames_per_chunk: usize) -> Vec<f32> {
        let mut result = Vec::with_capacity(chunks * frames_per_chunk);
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        for _ in 0..chunks {
            let mut output = vec![0.0; frames_per_chunk];
            mixer.render(&mut output, &mut pad_peaks);
            result.extend_from_slice(&output);
        }

        result
    }

    fn render_frames_with_pattern(
        mixer: &mut RtMixer,
        output_frame: &mut u64,
        total_frames: usize,
        pattern: &[usize],
    ) {
        assert!(!pattern.is_empty());
        let mut remaining = total_frames;
        let mut pattern_index = 0;
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];

        while remaining > 0 {
            let frames = pattern[pattern_index % pattern.len()].min(remaining);
            pattern_index += 1;
            if frames == 0 {
                continue;
            }

            let mut output = vec![0.0; frames];
            mixer.render_at_output_frame(*output_frame, &mut output, &mut pad_peaks);
            *output_frame = (*output_frame).saturating_add(frames as u64);
            remaining -= frames;
        }
    }

    fn active_voice_frame(mixer: &RtMixer, id: usize) -> Option<usize> {
        mixer
            .voices
            .iter()
            .find(|voice| voice.active && voice.sample_id == id)
            .map(|voice| voice.frame_pos)
    }

    fn four_bar_loop_frames(sample_rate_hz: f32, bpm: f64) -> usize {
        ((sample_rate_hz as f64 * 60.0 / bpm as f64) * 16.0).round() as usize
    }

    fn circular_phase_error_frames(left_phase: f64, right_phase: f64, cycle_frames: f64) -> f64 {
        let difference = (left_phase - right_phase).abs() % cycle_frames;
        difference.min(cycle_frames - difference)
    }

    fn bpm_locked_phase_error_frames(
        mixer: &RtMixer,
        left_id: usize,
        left_bpm: f64,
        right_id: usize,
        right_bpm: f64,
        master_bpm: f64,
        sample_rate_hz: f32,
    ) -> f64 {
        let left_loop_frames = four_bar_loop_frames(sample_rate_hz, left_bpm);
        let right_loop_frames = four_bar_loop_frames(sample_rate_hz, right_bpm);
        let output_cycle_frames = four_bar_loop_frames(sample_rate_hz, master_bpm) as f64;
        let left_ratio = (master_bpm / left_bpm).clamp(SPEED_MIN, SPEED_MAX);
        let right_ratio = (master_bpm / right_bpm).clamp(SPEED_MIN, SPEED_MAX);
        let left_frame = active_voice_frame(mixer, left_id).expect("left voice active");
        let right_frame = active_voice_frame(mixer, right_id).expect("right voice active");
        let left_phase = (left_frame % left_loop_frames) as f64 / left_ratio;
        let right_phase = (right_frame % right_loop_frames) as f64 / right_ratio;

        circular_phase_error_frames(left_phase, right_phase, output_cycle_frames)
    }

    fn active_voice_rubberband_block_size(mixer: &RtMixer, id: usize) -> usize {
        mixer
            .voices
            .iter()
            .find(|voice| voice.active && voice.sample_id == id)
            .map(|voice| voice.stretch.rubberband_block_size())
            .unwrap_or(1)
    }

    #[derive(Default)]
    struct CollectingRetirement {
        samples: Vec<SampleBuffer>,
        stems: Vec<PreparedStemSet>,
    }

    impl AudioBufferRetirement for CollectingRetirement {
        fn retire_resident_capture(
            &mut self,
            _: std::sync::Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>,
        ) {
        }
        fn retire_resident_cancellation(
            &mut self,
            _: std::sync::Arc<std::sync::atomic::AtomicBool>,
        ) {
        }
        fn retire_resident_transaction(&mut self, _: Box<crate::messages::ResidentTransaction>) {}
        fn retire_cold_adoption(&mut self, _: Arc<std::sync::atomic::AtomicU8>) {}
        fn retire_accepted_timing_refresh(
            &mut self,
            _: Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
        ) {
        }
        fn retire_global_playback_batch(
            &mut self,
            _: Arc<super::super::global_playback_batch::GlobalPlaybackBatch>,
        ) {
        }
        fn retire_sample(&mut self, sample: SampleBuffer) {
            self.samples.push(sample);
        }

        fn retire_prepared_stems(&mut self, stems: PreparedStemSet) {
            self.stems.push(stems);
        }

        fn retire_constant_timing(&mut self, timing: PreparedConstantTiming) {
            self.samples.push(timing.reference);
        }

        fn available_retirement_slots(&mut self) -> usize {
            usize::MAX
        }
    }

    fn create_test_prepared_stems(
        reference: &SampleBuffer,
        channels: usize,
        sample_rate_hz: u32,
        frames: usize,
    ) -> PreparedStemSet {
        let buffer = create_test_sample(channels, frames, 0.25);
        PreparedStemSet {
            complete_set_identity: std::sync::Arc::new([0; 32]),
            accepted_timing: None,
            reference_samples: reference.samples.clone(),
            publication: crate::audio_engine::prepared_source::PreparedSourcePermit::unrestricted(),
            source_version_hash: 42,
            sample_rate_hz,
            channels,
            frame_count: frames,
            available_mask: full_stem_available_mask(),
            stems: std::array::from_fn(|_| buffer.clone()),
        }
    }

    fn create_test_prepared_stems_with_values(
        reference: &SampleBuffer,
        channels: usize,
        sample_rate_hz: u32,
        frames: usize,
        values: [f32; STEM_BUFFER_COUNT],
    ) -> PreparedStemSet {
        PreparedStemSet {
            complete_set_identity: std::sync::Arc::new([0; 32]),
            accepted_timing: None,
            reference_samples: reference.samples.clone(),
            publication: crate::audio_engine::prepared_source::PreparedSourcePermit::unrestricted(),
            source_version_hash: 42,
            sample_rate_hz,
            channels,
            frame_count: frames,
            available_mask: full_stem_available_mask(),
            stems: values.map(|value| create_test_sample(channels, frames, value)),
        }
    }

    #[test]
    fn retained_stem_pcm_follows_only_successful_current_timing_adoption() {
        let source = create_test_sample(1, 20_000, 0.25);
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, source.clone());
        let stems = create_test_prepared_stems(&source, 1, 44_100, 20_000);
        let pcm_owner = stems.stems[0].samples.clone();
        assert!(mixer.publish_prepared_stems(0, stems));
        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
        mixer.set_pad_loop_region(0, 0.012_345_6, Some(0.321_987_6));
        mixer.set_speed(1.234_567_89);
        assert!(mixer.play_sample(0, 1.0));
        mixer.render(&mut [0.0; 17], &mut [0.0; NUM_SAMPLES]);
        let position = mixer
            .voices
            .iter()
            .find(|voice| voice.active)
            .unwrap()
            .source_playback
            .position();
        assert!(position.fraction > 0.0);
        let projection = AcceptedTimingProjection {
            revision: [0x17; 32],
            period_seconds: 60.0 / 119.999_123_456,
            origin_seconds: -0.123_456_789,
            sample_rate_hz: 44_100,
            publication_epoch: 1,
        };
        let mut retirement = CollectingRetirement::default();
        for next in [
            projection,
            AcceptedTimingProjection {
                revision: [0x18; 32],
                publication_epoch: 2,
                ..projection
            },
        ] {
            assert!(mixer.publish_constant_timing_rt(
                0,
                PreparedConstantTiming {
                    reference: source.clone(),
                    publication: super::super::prepared_source::PreparedSourcePermit::for_epoch(
                        Arc::new(std::sync::atomic::AtomicU64::new(next.publication_epoch)),
                        next.publication_epoch,
                    ),
                    projection: next,
                },
                &mut retirement
            ));
            let retained = mixer.prepared_stems[0].as_ref().unwrap();
            assert_eq!(retained.accepted_timing, Some(next));
            assert!(Arc::ptr_eq(&retained.stems[0].samples, &pcm_owner));
            assert_eq!(
                mixer
                    .voices
                    .iter()
                    .find(|voice| voice.active)
                    .unwrap()
                    .source_playback
                    .position(),
                position
            );
        }
        let effective = mixer.prepared_stems[0].as_ref().unwrap().accepted_timing;
        let mut invalid = projection;
        invalid.revision = [0x19; 32];
        invalid.publication_epoch = 3;
        assert!(!mixer.publish_constant_timing_rt(
            0,
            PreparedConstantTiming {
                reference: create_test_sample(1, 20_000, 0.25),
                publication: super::super::prepared_source::PreparedSourcePermit::unrestricted(),
                projection: invalid,
            },
            &mut retirement
        ));
        assert_eq!(
            mixer.prepared_stems[0].as_ref().unwrap().accepted_timing,
            effective
        );
        mixer.clear_constant_timing(0, 1); // an older queued clear cannot erase revision 2
        assert_eq!(
            mixer.prepared_stems[0].as_ref().unwrap().accepted_timing,
            effective
        );
        mixer.clear_constant_timing(0, 3);
        let retained = mixer.prepared_stems[0].as_ref().unwrap();
        assert_eq!(retained.accepted_timing, None);
        assert!(Arc::ptr_eq(&retained.stems[0].samples, &pcm_owner));
        assert_eq!(
            mixer
                .voices
                .iter()
                .find(|voice| voice.active)
                .unwrap()
                .source_playback
                .position(),
            position
        );
        assert!(retirement.stems.is_empty());
        assert_eq!(retirement.samples.len(), 3);
        let mut output = [0.0; 11];
        mixer.render(&mut output, &mut [0.0; NUM_SAMPLES]);
        assert!(output.iter().all(|value| *value == 1.0));
    }

    #[test]
    fn test_render_splits_oversized_blocks_to_preserve_stretch_capacity() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.set_speed(2.0);
        mixer.load_sample(0, create_test_sample(1, 5_000, 0.5));
        assert!(mixer.play_sample(0, 1.0));

        let frames = mixer.max_realtime_render_frames() * 2 + 37;
        let mut output = vec![0.0; frames];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];

        mixer.render(&mut output, &mut pad_peaks);

        assert!(output.iter().all(|sample| (*sample - 0.5).abs() < 1e-5));
        assert_eq!(active_voice_frame(&mixer, 0), Some(frames * 2));
    }

    #[test]
    fn test_unload_sample_rt_defers_loaded_sample_retirement() {
        let samples: Arc<[f32]> = Arc::from(vec![0.5_f32; 32].into_boxed_slice());
        let weak = Arc::downgrade(&samples);
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(
            0,
            SampleBuffer {
                residency: None,
                channels: 1,
                samples,
            },
        );
        let mut retirement = CollectingRetirement::default();

        assert!(mixer.unload_sample_rt(0, &mut retirement));

        assert!(mixer.sample_bank[0].is_none());
        assert_eq!(retirement.samples.len(), 1);
        assert!(weak.upgrade().is_some());

        drop(retirement);

        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn test_rejected_prepared_stems_are_retired() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        assert!(mixer.play_sample(0, 1.0));

        let stem_samples: Arc<[f32]> = Arc::from(vec![0.25_f32; 32].into_boxed_slice());
        let weak = Arc::downgrade(&stem_samples);
        let stems = PreparedStemSet {
            complete_set_identity: std::sync::Arc::new([0; 32]),
            accepted_timing: None,
            reference_samples: mixer.sample_bank[0].as_ref().unwrap().samples.clone(),
            publication: crate::audio_engine::prepared_source::PreparedSourcePermit::unrestricted(),
            source_version_hash: 42,
            sample_rate_hz: 44_100,
            channels: 1,
            frame_count: 32,
            available_mask: full_stem_available_mask(),
            stems: std::array::from_fn(|_| SampleBuffer {
                residency: None,
                channels: 1,
                samples: stem_samples.clone(),
            }),
        };
        drop(stem_samples);
        let mut retirement = CollectingRetirement::default();

        assert!(!mixer.publish_prepared_stems_rt(0, stems, &mut retirement));

        assert_eq!(retirement.stems.len(), 1);
        assert!(weak.upgrade().is_some());

        drop(retirement);

        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn test_tempo_ratio_for_sample_id_speed_only() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        mixer.set_speed(1.25);

        let ratio = mixer.tempo_ratio_for_sample_id(0);
        assert!((ratio - 1.25).abs() < 1e-6);
    }

    #[test]
    fn test_tempo_ratio_for_sample_id_bpm_lock() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        mixer.set_speed(1.0);
        mixer.set_bpm_lock(true);
        mixer.set_master_bpm(120.0);
        mixer.set_pad_bpm(0, Some(90.0));

        let ratio = mixer.tempo_ratio_for_sample_id(0);
        assert!((ratio - (120.0 / 90.0)).abs() < 1e-6);

        mixer.set_pad_bpm(0, None);
        let ratio = mixer.tempo_ratio_for_sample_id(0);
        assert!((ratio - 1.0).abs() < 1e-6);
    }

    #[test]
    fn key_lock_reduces_varispeed_pitch_shift_in_mixer_path() {
        let sample_rate_hz = 48_000.0;
        let source_hz = 440.0;
        let source = create_sine_sample(sample_rate_hz, 96_000, source_hz);

        let mut varispeed_mixer = RtMixer::new(1, sample_rate_hz);
        varispeed_mixer.load_sample(0, source.clone());
        varispeed_mixer.set_speed(2.0);
        varispeed_mixer.set_key_lock(false);
        assert!(varispeed_mixer.play_sample(0, 1.0));
        let varispeed_output = render_chunks(&mut varispeed_mixer, 48, 512);

        let mut key_lock_mixer = RtMixer::new(1, sample_rate_hz);
        key_lock_mixer.load_sample(0, source);
        key_lock_mixer.set_speed(2.0);
        key_lock_mixer.set_key_lock(true);
        assert!(key_lock_mixer.play_sample(0, 1.0));
        let key_lock_output = render_chunks(&mut key_lock_mixer, 48, 512);

        let skip = 8192;
        let varispeed_hz = estimate_frequency(&varispeed_output[skip..], sample_rate_hz);
        let key_lock_hz = estimate_frequency(&key_lock_output[skip..], sample_rate_hz);

        assert!(varispeed_hz > 800.0, "varispeed_hz={varispeed_hz}");
        assert!(
            (360.0..560.0).contains(&key_lock_hz),
            "key_lock_hz={key_lock_hz}"
        );
    }

    #[test]
    fn key_lock_ratio_change_while_active_advances_existing_voice() {
        let sample_rate_hz = 48_000.0;
        let source = create_sine_sample(sample_rate_hz, 200_000, 330.0);
        let mut mixer = RtMixer::new(1, sample_rate_hz);
        mixer.load_sample(0, source);
        mixer.set_speed(1.0);
        mixer.set_key_lock(true);
        assert!(mixer.play_sample(0, 1.0));

        let before_change_output = render_chunks(&mut mixer, 4, 512);
        let frame_before_change = active_voice_frame(&mixer, 0).unwrap();

        mixer.set_speed(2.0);
        let after_change_output = render_chunks(&mut mixer, 12, 512);
        let frame_after_change = active_voice_frame(&mixer, 0).unwrap();

        assert!(before_change_output.iter().all(|sample| sample.is_finite()));
        assert!(after_change_output.iter().all(|sample| sample.is_finite()));
        assert_eq!(mixer.voices.iter().filter(|voice| voice.active).count(), 1);
        assert!(frame_after_change > frame_before_change + 12 * 512);
        assert!(frame_after_change <= frame_before_change + 12 * 1024);
    }

    #[test]
    fn active_key_lock_toggles_do_not_retrigger_or_stop_voice() {
        let mut mixer = RtMixer::new(1, 48_000.0);
        mixer.load_sample(0, create_sine_sample(48_000.0, 96_000, 440.0));
        mixer.set_speed(2.0);
        mixer.set_key_lock(false);
        assert!(mixer.play_sample(0, 1.0));

        let varispeed_output = render_chunks(&mut mixer, 1, 512);
        let after_varispeed = active_voice_frame(&mixer, 0).unwrap();
        mixer.set_key_lock(true);
        let key_lock_output = render_chunks(&mut mixer, 1, 512);
        let after_key_lock = active_voice_frame(&mixer, 0).unwrap();
        mixer.set_key_lock(false);
        let restored_output = render_chunks(&mut mixer, 1, 512);
        let after_restore = active_voice_frame(&mixer, 0).unwrap();

        assert!(varispeed_output.iter().all(|sample| sample.is_finite()));
        assert!(key_lock_output.iter().all(|sample| sample.is_finite()));
        assert!(restored_output.iter().all(|sample| sample.is_finite()));
        assert_eq!(after_varispeed, 1024);
        assert_eq!(after_key_lock, 2048);
        assert_eq!(after_restore, 3072);
        assert_eq!(mixer.voices.iter().filter(|voice| voice.active).count(), 1);
    }

    #[test]
    fn global_key_lock_overwrites_all_pad_states() {
        let mut mixer = RtMixer::new(1, 48_000.0);

        mixer.set_pad_key_lock(3, true);
        mixer.set_pad_key_lock(4, false);
        mixer.set_key_lock(true);

        assert!(mixer.pad_key_lock_enabled.iter().all(|enabled| *enabled));

        mixer.set_key_lock(false);

        assert!(mixer.pad_key_lock_enabled.iter().all(|enabled| !*enabled));
    }

    #[test]
    fn per_pad_key_lock_update_changes_only_target_pad() {
        let mut mixer = RtMixer::new(1, 48_000.0);

        mixer.set_key_lock(true);
        mixer.set_pad_key_lock(3, false);

        assert!(mixer.pad_key_lock_enabled[2]);
        assert!(!mixer.pad_key_lock_enabled[3]);
        assert!(mixer.pad_key_lock_enabled[4]);
    }

    #[test]
    fn per_pad_key_lock_modes_render_independently() {
        let sample_rate_hz = 48_000.0;
        let source_hz = 440.0;
        let source = create_sine_sample(sample_rate_hz, 96_000, source_hz);
        let mut mixer = RtMixer::new(1, sample_rate_hz);
        mixer.load_sample(0, source.clone());
        mixer.load_sample(1, source);
        mixer.set_speed(2.0);
        mixer.set_key_lock(false);
        mixer.set_pad_key_lock(1, true);

        assert!(mixer.play_sample(0, 1.0));
        let varispeed_output = render_chunks(&mut mixer, 48, 512);
        mixer.stop_sample(0);

        assert!(mixer.play_sample(1, 1.0));
        let key_lock_output = render_chunks(&mut mixer, 48, 512);

        let skip = 8192;
        let varispeed_hz = estimate_frequency(&varispeed_output[skip..], sample_rate_hz);
        let key_lock_hz = estimate_frequency(&key_lock_output[skip..], sample_rate_hz);

        assert!(varispeed_hz > 800.0, "varispeed_hz={varispeed_hz}");
        assert!(
            (360.0..560.0).contains(&key_lock_hz),
            "key_lock_hz={key_lock_hz}"
        );
    }

    #[test]
    fn key_lock_loop_wrap_keeps_source_playhead_in_loop_region() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_frame_number_sample(30));
        mixer.set_pad_loop_region(0, 1.0, Some(1.8));
        mixer.set_speed(2.0);
        mixer.set_key_lock(true);
        assert!(mixer.play_sample(0, 1.0));

        let output = render_chunks(&mut mixer, 1, 5);

        assert!(output.iter().all(|sample| sample.is_finite()));
        assert_eq!(active_voice_frame(&mixer, 0), Some(12));
    }

    #[test]
    fn key_lock_retrigger_stop_and_unload_clear_pending_shifted_output() {
        let mut mixer = RtMixer::new(1, 48_000.0);
        let source = create_sine_sample(48_000.0, 96_000, 440.0);
        mixer.load_sample(0, source.clone());
        mixer.set_speed(2.0);
        mixer.set_key_lock(true);
        assert!(mixer.play_sample(0, 1.0));

        let primed_output = render_chunks(&mut mixer, 24, 512);
        assert!(primed_output.iter().any(|sample| sample.abs() > 1.0e-4));

        let block_size = active_voice_rubberband_block_size(&mixer, 0);
        let fallback_frames = block_size
            .saturating_sub(1)
            .clamp(1, mixer.max_realtime_render_frames());

        assert!(mixer.play_sample(0, 1.0));
        let retrigger_output = render_chunks(&mut mixer, 1, fallback_frames);
        assert!(retrigger_output.iter().all(|sample| sample.abs() < 1.0e-6));

        let primed_output = render_chunks(&mut mixer, 24, 512);
        assert!(primed_output.iter().any(|sample| sample.abs() > 1.0e-4));

        mixer.stop_sample(0);
        let stopped_output = render_chunks(&mut mixer, 1, 512);
        assert!(stopped_output.iter().all(|sample| sample.abs() < 1.0e-6));
        assert!(mixer.voices.iter().all(|voice| !voice.active));

        assert!(mixer.play_sample(0, 1.0));
        let restart_output = render_chunks(&mut mixer, 1, fallback_frames);
        assert!(restart_output.iter().all(|sample| sample.abs() < 1.0e-6));

        let primed_output = render_chunks(&mut mixer, 24, 512);
        assert!(primed_output.iter().any(|sample| sample.abs() > 1.0e-4));

        mixer.unload_sample(0);
        let unloaded_output = render_chunks(&mut mixer, 1, 512);
        assert!(unloaded_output.iter().all(|sample| sample.abs() < 1.0e-6));
        assert!(mixer.sample_bank[0].is_none());
        assert!(mixer.voices.iter().all(|voice| !voice.active));

        mixer.load_sample(0, source);
        assert!(mixer.play_sample(0, 1.0));
        let reload_output = render_chunks(&mut mixer, 1, fallback_frames);
        assert!(reload_output.iter().all(|sample| sample.abs() < 1.0e-6));
    }

    #[test]
    fn stem_selection_and_pause_resume_retain_warm_key_lock_history() {
        let mut mixer = RtMixer::new(1, 48_000.0);
        mixer.load_sample(0, create_sine_sample(48_000.0, 96_000, 440.0));
        assert!(mixer.publish_prepared_stems(
            0,
            create_test_prepared_stems(mixer.sample_bank[0].as_ref().unwrap(), 1, 48_000, 96_000)
        ));
        mixer.set_speed(2.0);
        mixer.set_key_lock(true);
        assert!(mixer.play_sample(0, 1.0));
        let warm = render_chunks(&mut mixer, 24, 512);
        assert!(warm.iter().any(|sample| sample.abs() > 0.05));

        let before_pause = active_voice_frame(&mixer, 0);
        mixer.pause_sample(0);
        assert!(
            render_chunks(&mut mixer, 2, 512)
                .iter()
                .all(|sample| *sample == 0.0)
        );
        assert_eq!(active_voice_frame(&mixer, 0), before_pause);
        mixer.resume_sample(0);
        let resumed = render_chunks(&mut mixer, 1, 512);
        assert!(resumed.iter().any(|sample| sample.abs() > 0.05));

        let before_switch = active_voice_frame(&mixer, 0).unwrap();
        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
        let switched = render_chunks(&mut mixer, 1, 512);
        assert!(switched.iter().any(|sample| sample.abs() > 0.05));
        assert_eq!(active_voice_frame(&mixer, 0), Some(before_switch + 1024));
    }

    #[test]
    fn prepared_stems_share_bpm_lock_key_lock_pitch_path_with_full_mix() {
        let sample_rate_hz = 48_000.0;
        let frames = 96_000;
        let source_hz = 330.0;

        let mut full_mix_mixer = RtMixer::new(1, sample_rate_hz);
        full_mix_mixer.load_sample(0, create_sine_sample(sample_rate_hz, frames, source_hz));
        full_mix_mixer.set_bpm_lock(true);
        full_mix_mixer.set_master_bpm(120.0);
        full_mix_mixer.set_pad_bpm(0, Some(60.0));
        full_mix_mixer.set_key_lock(true);
        assert!(full_mix_mixer.play_sample(0, 1.0));

        let mut stem_mixer = RtMixer::new(1, sample_rate_hz);
        stem_mixer.load_sample(0, create_test_sample(1, frames, 0.0));
        assert!(stem_mixer.publish_prepared_stems(
            0,
            create_sine_prepared_stems(
                stem_mixer.sample_bank[0].as_ref().unwrap(),
                sample_rate_hz as u32,
                frames,
                source_hz
            )
        ));
        assert!(stem_mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
        stem_mixer.set_bpm_lock(true);
        stem_mixer.set_master_bpm(120.0);
        stem_mixer.set_pad_bpm(0, Some(60.0));
        stem_mixer.set_key_lock(true);
        assert!(stem_mixer.play_sample(0, 1.0));

        let full_mix_output = render_chunks(&mut full_mix_mixer, 40, 512);
        let stem_output = render_chunks(&mut stem_mixer, 40, 512);
        let skip = 8192;
        let full_mix_hz = estimate_frequency(&full_mix_output[skip..], sample_rate_hz);
        let stem_hz = estimate_frequency(&stem_output[skip..], sample_rate_hz);

        assert_eq!(
            active_voice_frame(&full_mix_mixer, 0),
            active_voice_frame(&stem_mixer, 0)
        );
        assert!(
            (260.0..420.0).contains(&full_mix_hz),
            "full_mix_hz={full_mix_hz}"
        );
        assert!((260.0..420.0).contains(&stem_hz), "stem_hz={stem_hz}");
        assert!(
            (full_mix_hz - stem_hz).abs() < 60.0,
            "full_mix_hz={full_mix_hz} stem_hz={stem_hz}"
        );
    }

    #[test]
    fn multi_loop_key_lock_voices_render_finite_and_stay_bounded() {
        let mut mixer = RtMixer::new(1, 48_000.0);
        mixer.set_speed(2.0);
        mixer.set_key_lock(true);
        let active_voices = 8;

        for id in 0..active_voices {
            mixer.load_sample(
                id,
                create_sine_sample(48_000.0, 96_000, 220.0 + id as f32 * 35.0),
            );
            mixer.set_pad_loop_region(id, 0.0, Some(1.5));
            assert!(mixer.play_sample(id, 0.5));
        }

        let frames = mixer.max_realtime_render_frames() * 2 + 17;
        let output = render_chunks(&mut mixer, 1, frames);

        assert_eq!(
            mixer.voices.iter().filter(|voice| voice.active).count(),
            active_voices
        );
        assert!(output.iter().all(|sample| sample.is_finite()));
        for id in 0..active_voices {
            assert_eq!(active_voice_frame(&mixer, id), Some(2082));
        }
    }

    #[test]
    fn bpm_locked_multi_loop_should_stay_phase_stable_across_repeated_wraps() {
        const SAMPLE_RATE_HZ: f32 = 3_360.0;
        const ANCHOR_BPM: f64 = 120.0;
        const SECOND_PAD_BPM: f64 = 140.0;
        const LOOP_REPEATS: usize = 10;
        const PHASE_TOLERANCE_FRAMES: f64 = 1.0;

        let patterns: [(&str, &[usize]); 2] = [
            ("fixed-512", &[512]),
            ("variable", &[127, 385, 64, 448, 511, 1]),
        ];
        let mut failures = Vec::new();

        for speed in [1.0_f64, 1.25, 1.5, 2.0] {
            let master_bpm = ANCHOR_BPM * speed;
            let output_cycle_frames = four_bar_loop_frames(SAMPLE_RATE_HZ, master_bpm);

            for key_lock_enabled in [false, true] {
                for (pattern_name, pattern) in patterns {
                    let mut output_frame = 0_u64;
                    let mut mixer = RtMixer::new(1, SAMPLE_RATE_HZ);
                    mixer.set_bpm_lock(true);
                    mixer.set_master_bpm(master_bpm);
                    mixer.set_key_lock(key_lock_enabled);

                    for (id, bpm) in [(0, ANCHOR_BPM), (1, SECOND_PAD_BPM), (2, ANCHOR_BPM)] {
                        let loop_frames = four_bar_loop_frames(SAMPLE_RATE_HZ, bpm);
                        mixer.load_sample(id, create_test_sample(1, loop_frames, 0.25));
                        mixer.set_pad_bpm(id, Some(bpm));
                        mixer.set_pad_loop_region(
                            id,
                            0.0,
                            Some(loop_frames as f64 / SAMPLE_RATE_HZ as f64),
                        );
                        assert!(mixer.play_sample_at_output_frame(id, 1.0, output_frame));
                    }

                    render_frames_with_pattern(
                        &mut mixer,
                        &mut output_frame,
                        output_cycle_frames,
                        pattern,
                    );

                    let matched_bpm_first_pass_error = bpm_locked_phase_error_frames(
                        &mixer,
                        0,
                        ANCHOR_BPM,
                        1,
                        SECOND_PAD_BPM,
                        master_bpm,
                        SAMPLE_RATE_HZ,
                    );
                    if matched_bpm_first_pass_error > PHASE_TOLERANCE_FRAMES {
                        failures.push(format!(
                            "matched BPM drifted on first pass: speed={speed}, \
                             key_lock={key_lock_enabled}, pattern={pattern_name}, \
                             error={matched_bpm_first_pass_error:.3} frames",
                        ));
                    }

                    render_frames_with_pattern(
                        &mut mixer,
                        &mut output_frame,
                        output_cycle_frames * (LOOP_REPEATS - 1),
                        pattern,
                    );

                    let same_bpm_error = bpm_locked_phase_error_frames(
                        &mixer,
                        0,
                        ANCHOR_BPM,
                        2,
                        ANCHOR_BPM,
                        master_bpm,
                        SAMPLE_RATE_HZ,
                    );
                    let matched_bpm_error = bpm_locked_phase_error_frames(
                        &mixer,
                        0,
                        ANCHOR_BPM,
                        1,
                        SECOND_PAD_BPM,
                        master_bpm,
                        SAMPLE_RATE_HZ,
                    );

                    if same_bpm_error > PHASE_TOLERANCE_FRAMES {
                        failures.push(format!(
                            "same BPM drifted: speed={speed}, key_lock={key_lock_enabled}, \
                             pattern={pattern_name}, error={same_bpm_error:.3} frames",
                        ));
                    }
                    if matched_bpm_error > PHASE_TOLERANCE_FRAMES {
                        failures.push(format!(
                            "matched BPM drifted: speed={speed}, key_lock={key_lock_enabled}, \
                             pattern={pattern_name}, error={matched_bpm_error:.3} frames",
                        ));
                    }

                    assert!(mixer.play_sample_at_output_frame(0, 1.0, output_frame));
                    assert!(mixer.play_sample_at_output_frame(1, 1.0, output_frame));
                    let retrigger_error = bpm_locked_phase_error_frames(
                        &mixer,
                        0,
                        ANCHOR_BPM,
                        1,
                        SECOND_PAD_BPM,
                        master_bpm,
                        SAMPLE_RATE_HZ,
                    );
                    if retrigger_error > PHASE_TOLERANCE_FRAMES {
                        failures.push(format!(
                            "matched BPM retrigger did not reset phase: speed={speed}, \
                             key_lock={key_lock_enabled}, pattern={pattern_name}, \
                             error={retrigger_error:.3} frames",
                        ));
                    }
                }
            }
        }

        assert!(
            failures.is_empty(),
            "BPM-locked Multi Loop voices should remain phase-stable after repeated wraps: {}",
            failures.join("; ")
        );
    }

    #[test]
    fn prepared_stems_share_output_anchored_bpm_lock_phase_with_full_mix() {
        const SAMPLE_RATE_HZ: f32 = 3_360.0;
        const ANCHOR_BPM: f64 = 120.0;
        const STEM_PAD_BPM: f64 = 140.0;
        const PHASE_TOLERANCE_FRAMES: f64 = 1.0;

        let mut output_frame = 0_u64;
        let mut mixer = RtMixer::new(1, SAMPLE_RATE_HZ);
        mixer.set_bpm_lock(true);
        mixer.set_master_bpm(ANCHOR_BPM * 1.5);

        let full_mix_loop_frames = four_bar_loop_frames(SAMPLE_RATE_HZ, ANCHOR_BPM);
        mixer.load_sample(0, create_test_sample(1, full_mix_loop_frames, 0.2));
        mixer.set_pad_bpm(0, Some(ANCHOR_BPM));
        mixer.set_pad_loop_region(
            0,
            0.0,
            Some(full_mix_loop_frames as f64 / SAMPLE_RATE_HZ as f64),
        );

        let stem_loop_frames = four_bar_loop_frames(SAMPLE_RATE_HZ, STEM_PAD_BPM);
        mixer.load_sample(1, create_test_sample(1, stem_loop_frames, 0.0));
        assert!(mixer.publish_prepared_stems(
            1,
            create_test_prepared_stems_with_values(
                mixer.sample_bank[1].as_ref().unwrap(),
                1,
                SAMPLE_RATE_HZ as u32,
                stem_loop_frames,
                [0.1, 0.05, 0.0, 0.0],
            ),
        ));
        assert!(mixer.set_stem_mix_mode(1, StemMixMode::AllStems, 42));
        mixer.set_pad_bpm(1, Some(STEM_PAD_BPM));
        mixer.set_pad_loop_region(
            1,
            0.0,
            Some(stem_loop_frames as f64 / SAMPLE_RATE_HZ as f64),
        );

        assert!(mixer.play_sample_at_output_frame(0, 1.0, output_frame));
        assert!(mixer.play_sample_at_output_frame(1, 1.0, output_frame));

        render_frames_with_pattern(
            &mut mixer,
            &mut output_frame,
            four_bar_loop_frames(SAMPLE_RATE_HZ, ANCHOR_BPM * 1.5) * 10,
            &[127, 385, 64, 448, 511, 1],
        );

        let error = bpm_locked_phase_error_frames(
            &mixer,
            0,
            ANCHOR_BPM,
            1,
            STEM_PAD_BPM,
            ANCHOR_BPM * 1.5,
            SAMPLE_RATE_HZ,
        );
        assert!(
            error <= PHASE_TOLERANCE_FRAMES,
            "prepared-stem pad drifted from full mix by {error:.3} frames"
        );
    }

    #[test]
    fn missing_bpm_fallback_does_not_disturb_synced_multi_loop_pads() {
        const SAMPLE_RATE_HZ: f32 = 3_360.0;
        const ANCHOR_BPM: f64 = 120.0;
        const SECOND_PAD_BPM: f64 = 140.0;
        const PHASE_TOLERANCE_FRAMES: f64 = 1.0;

        let mut output_frame = 0_u64;
        let mut mixer = RtMixer::new(1, SAMPLE_RATE_HZ);
        mixer.set_speed(1.25);
        mixer.set_bpm_lock(true);
        mixer.set_master_bpm(ANCHOR_BPM * 1.5);

        for (id, bpm) in [(0, Some(ANCHOR_BPM)), (1, Some(SECOND_PAD_BPM)), (2, None)] {
            let loop_bpm = bpm.unwrap_or(ANCHOR_BPM);
            let loop_frames = four_bar_loop_frames(SAMPLE_RATE_HZ, loop_bpm);
            mixer.load_sample(id, create_test_sample(1, loop_frames, 0.2));
            mixer.set_pad_bpm(id, bpm);
            mixer.set_pad_loop_region(id, 0.0, Some(loop_frames as f64 / SAMPLE_RATE_HZ as f64));
            assert!(mixer.play_sample_at_output_frame(id, 1.0, output_frame));
        }

        render_frames_with_pattern(
            &mut mixer,
            &mut output_frame,
            four_bar_loop_frames(SAMPLE_RATE_HZ, ANCHOR_BPM * 1.5) * 10,
            &[512, 127, 385, 64],
        );

        let error = bpm_locked_phase_error_frames(
            &mixer,
            0,
            ANCHOR_BPM,
            1,
            SECOND_PAD_BPM,
            ANCHOR_BPM * 1.5,
            SAMPLE_RATE_HZ,
        );
        assert!(
            error <= PHASE_TOLERANCE_FRAMES,
            "valid BPM pads drifted after missing-BPM pad joined by {error:.3} frames"
        );
        assert!(active_voice_frame(&mixer, 2).is_some());
        assert_eq!(mixer.output_bpm_for_sample_id(2), None);
    }

    #[test]
    fn test_long_editor_markers_and_signed_origins_retain_individual_source_frames() {
        for rate in [44_100.0, 48_000.0, 96_000.0] {
            let mut mixer = RtMixer::new(1, rate);
            let start_frame = rate as usize * 3_600 + 137;
            let end_frame = start_frame + 23;
            mixer.set_pad_loop_region(
                0,
                start_frame as f64 / rate as f64,
                Some(end_frame as f64 / rate as f64),
            );
            assert_eq!(mixer.pad_loop_start_frame[0], start_frame);
            assert_eq!(mixer.pad_loop_end_frame[0], Some(end_frame));
            mixer.set_pad_timing_metadata(
                0,
                PadTimingMetadata {
                    phase_anchor_s: -(start_frame as f64 / rate as f64),
                },
            );
            assert_eq!(mixer.pad_phase_anchor_frame(0), Some(-(start_frame as f64)));
        }
    }

    #[test]
    fn test_pad_timing_metadata_stores_sample_accurate_anchor_frame() {
        let mut mixer = RtMixer::new(1, 10.0);

        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 1.2,
            },
        );

        assert_eq!(mixer.pad_phase_anchor_frame(0), Some(12.0));
    }

    #[test]
    fn test_pad_timing_metadata_invalid_values_preserve_previous_origin() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 1.2,
            },
        );

        for phase_anchor_s in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            mixer.set_pad_timing_metadata(0, PadTimingMetadata { phase_anchor_s });
            assert_eq!(mixer.pad_phase_anchor_frame(0), Some(12.0));
        }
    }

    #[test]
    fn test_pad_timing_metadata_invalid_id_is_ignored() {
        let mut mixer = RtMixer::new(1, 10.0);

        mixer.set_pad_timing_metadata(
            NUM_SAMPLES + 1,
            PadTimingMetadata {
                phase_anchor_s: 1.2,
            },
        );

        assert_eq!(mixer.pad_phase_anchor_frame(0), Some(0.0));
        assert_eq!(mixer.pad_phase_anchor_frame(NUM_SAMPLES + 1), None);
    }

    #[test]
    fn test_unload_sample_clears_pad_timing_metadata() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 100, 0.5));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 1.2,
            },
        );

        mixer.unload_sample(0);

        assert_eq!(mixer.pad_phase_anchor_frame(0), Some(0.0));
    }

    #[test]
    fn test_phase_aligned_initial_frame_uses_pad_bpm_and_anchor() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 0.3,
            },
        );

        let frame = mixer.phase_aligned_initial_sample_frame(0, 64, 2.0);

        assert_eq!(frame, 23);
    }

    #[test]
    fn test_phase_aligned_initial_frame_uses_active_loop_region() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 1.0,
            },
        );
        mixer.set_pad_loop_region(0, 1.0, Some(5.0));

        let frame = mixer.phase_aligned_initial_sample_frame(0, 64, 2.0);

        assert_eq!(frame, 30);
    }

    #[test]
    fn test_phase_aligned_initial_frame_wraps_into_loop_region() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 1.0,
            },
        );
        mixer.set_pad_loop_region(0, 1.0, Some(3.0));

        let frame = mixer.phase_aligned_initial_sample_frame(0, 64, 3.0);

        assert_eq!(frame, 20);
    }

    #[test]
    fn test_phase_aligned_initial_frame_falls_back_without_pad_bpm() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 0.3,
            },
        );
        mixer.set_pad_loop_region(0, 0.7, Some(2.0));

        let frame = mixer.phase_aligned_initial_sample_frame(0, 64, 2.0);

        assert_eq!(frame, 7);
    }

    #[test]
    fn test_phase_aligned_initial_frame_falls_back_for_invalid_anchor() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_pad_loop_region(0, 0.5, Some(2.0));
        mixer.pad_phase_anchor_frame[0] = f64::NAN;

        let frame = mixer.phase_aligned_initial_sample_frame(0, 20, 2.0);

        assert_eq!(frame, 5);
    }

    #[test]
    fn test_phase_aligned_initial_frame_falls_back_for_invalid_loop_region() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 0.3,
            },
        );
        mixer.set_pad_loop_region(0, 5.0, Some(6.0));

        let frame = mixer.phase_aligned_initial_sample_frame(0, 20, 2.0);

        assert_eq!(frame, 0);
    }

    #[test]
    fn test_play_sample_phase_aligned_starts_voice_at_phase_frame() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 0.3,
            },
        );

        assert!(mixer.play_sample_phase_aligned(0, 1.0, 2.0));

        assert_eq!(active_voice_frame(&mixer, 0), Some(23));
    }

    #[test]
    fn test_play_sample_keeps_immediate_loop_start_with_phase_metadata() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.set_pad_loop_region(0, 0.7, Some(5.0));
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 2.0,
            },
        );

        assert!(mixer.play_sample(0, 1.0));

        assert_eq!(active_voice_frame(&mixer, 0), Some(7));
    }

    #[test]
    fn test_active_pad_bar_phase_uses_current_voice_frame() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 0.5,
            },
        );
        assert!(mixer.play_sample(0, 1.0));

        let mut output = vec![0.0; 30];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        assert_eq!(active_voice_frame(&mixer, 0), Some(30));
        assert_eq!(mixer.active_pad_bar_phase_beats(0), Some(2.5));
    }

    #[test]
    fn test_active_pad_bar_phase_wraps_before_anchor() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 2.0,
            },
        );
        assert!(mixer.play_sample(0, 1.0));

        assert_eq!(mixer.active_pad_bar_phase_beats(0), Some(2.0));
    }

    #[test]
    fn test_active_pad_bar_phase_requires_playing_pad_bpm_and_valid_anchor() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 20, 0.5));

        assert_eq!(mixer.active_pad_bar_phase_beats(0), None);
        assert!(mixer.play_sample(0, 1.0));
        assert_eq!(mixer.active_pad_bar_phase_beats(0), None);

        mixer.set_pad_bpm(0, Some(60.0));
        mixer.pad_phase_anchor_frame[0] = f64::NAN;
        assert_eq!(mixer.active_pad_bar_phase_beats(0), None);
    }

    #[test]
    fn test_load_sample() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        let sample = create_test_sample(2, 100, 0.5);

        mixer.load_sample(0, sample.clone());

        // Sample should be loaded
        assert!(mixer.sample_bank[0].is_some());
    }

    #[test]
    fn test_load_sample_clears_prepared_stems() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        mixer.load_sample(0, create_test_sample(2, 100, 0.5));
        assert!(mixer.publish_prepared_stems(
            0,
            create_test_prepared_stems(mixer.sample_bank[0].as_ref().unwrap(), 2, 44_100, 100)
        ));

        mixer.load_sample(0, create_test_sample(2, 100, 0.25));

        assert!(mixer.prepared_stems[0].is_none());
    }

    #[test]
    fn test_publish_prepared_stems_accepts_stopped_loaded_pad() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        mixer.load_sample(0, create_test_sample(2, 100, 0.5));

        assert!(mixer.publish_prepared_stems(
            0,
            create_test_prepared_stems(mixer.sample_bank[0].as_ref().unwrap(), 2, 44_100, 100)
        ));

        assert_eq!(
            mixer.prepared_stems[0]
                .as_ref()
                .map(|stems| stems.source_version_hash),
            Some(42)
        );
    }

    #[test]
    fn delayed_prepared_stems_reject_equal_valued_replacement_and_retire_source_pin() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        let sample = create_test_sample(1, 100, 0.5);
        let old_source = Arc::downgrade(&sample.samples);
        mixer.load_sample(0, sample);
        let stems =
            create_test_prepared_stems(mixer.sample_bank[0].as_ref().unwrap(), 1, 44_100, 100);

        // A fresh source can have exactly the same dimensions and PCM values.
        mixer.load_sample(0, create_test_sample(1, 100, 0.5));
        let mut retirement = CollectingRetirement::default();
        assert!(!mixer.publish_prepared_stems_rt(0, stems, &mut retirement));
        assert!(mixer.prepared_stems[0].is_none());
        assert_eq!(retirement.stems.len(), 1);
        assert!(old_source.upgrade().is_some());

        drop(retirement);
        assert!(old_source.upgrade().is_none());
    }

    #[test]
    fn delayed_prepared_stems_reject_timing_intent_changed_after_enqueue() {
        use crate::audio_engine::prepared_source::PreparedSourcePermit;
        use std::sync::atomic::{AtomicU64, Ordering};

        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 100, 0.5));
        let epoch = Arc::new(AtomicU64::new(1));
        let mut stems =
            create_test_prepared_stems(mixer.sample_bank[0].as_ref().unwrap(), 1, 44_100, 100);
        stems.publication = PreparedSourcePermit::for_epoch(epoch.clone(), 1);

        // Model an intent publication after command enqueue and before callback consumption.
        epoch.store(2, Ordering::Release);
        let mut retirement = CollectingRetirement::default();
        assert!(!mixer.publish_prepared_stems_rt(0, stems, &mut retirement));
        assert!(mixer.prepared_stems[0].is_none());
        assert_eq!(retirement.stems.len(), 1);
    }

    #[test]
    fn test_publish_prepared_stems_rejects_active_pad() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 100, 0.5));
        assert!(mixer.play_sample(0, 1.0));

        assert!(!mixer.publish_prepared_stems(
            0,
            create_test_prepared_stems(mixer.sample_bank[0].as_ref().unwrap(), 1, 44_100, 100)
        ));

        assert!(mixer.prepared_stems[0].is_none());
    }

    #[test]
    fn test_publish_prepared_stems_rejects_mismatched_layout() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        mixer.load_sample(0, create_test_sample(2, 100, 0.5));

        assert!(!mixer.publish_prepared_stems(
            0,
            create_test_prepared_stems(mixer.sample_bank[0].as_ref().unwrap(), 1, 44_100, 100)
        ));
        assert!(!mixer.publish_prepared_stems(
            0,
            create_test_prepared_stems(mixer.sample_bank[0].as_ref().unwrap(), 2, 48_000, 100)
        ));
        assert!(!mixer.publish_prepared_stems(
            0,
            create_test_prepared_stems(mixer.sample_bank[0].as_ref().unwrap(), 2, 44_100, 99)
        ));

        assert!(mixer.prepared_stems[0].is_none());
    }

    #[test]
    fn test_set_stem_mix_mode_requires_matching_prepared_source() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 20, 0.9));

        assert!(!mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
        assert_eq!(mixer.stem_mix_mode[0], StemMixMode::FullMix);

        assert!(mixer.publish_prepared_stems(
            0,
            create_test_prepared_stems(mixer.sample_bank[0].as_ref().unwrap(), 1, 44_100, 20)
        ));
        assert!(!mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 7));
        assert_eq!(mixer.stem_mix_mode[0], StemMixMode::FullMix);

        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
        assert_eq!(mixer.stem_mix_mode[0], StemMixMode::AllStems);
        assert_eq!(mixer.stem_mix_source_version_hash[0], 42);
    }

    #[test]
    fn test_set_stem_mix_mode_reverts_to_full_mix_without_prepared_stems() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 20, 0.9));
        assert!(mixer.publish_prepared_stems(
            0,
            create_test_prepared_stems(mixer.sample_bank[0].as_ref().unwrap(), 1, 44_100, 20)
        ));
        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));

        assert!(mixer.set_stem_mix_mode(0, StemMixMode::FullMix, 0));

        assert_eq!(mixer.stem_mix_mode[0], StemMixMode::FullMix);
        assert_eq!(mixer.stem_mix_source_version_hash[0], 0);
    }

    #[test]
    fn test_set_stem_enabled_mask_requires_matching_prepared_source() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 20, 0.9));

        assert!(!mixer.set_stem_enabled_mask(0, STEM_MASK_VOCALS, 42));
        assert_eq!(mixer.stem_enabled_mask[0], STEM_COMPONENT_MASK);

        assert!(mixer.publish_prepared_stems(
            0,
            create_test_prepared_stems(mixer.sample_bank[0].as_ref().unwrap(), 1, 44_100, 20)
        ));
        assert!(!mixer.set_stem_enabled_mask(0, STEM_MASK_VOCALS, 7));
        assert!(!mixer.set_stem_enabled_mask(0, STEM_MASK_VOCALS | stem_index_mask(4), 42));
        assert_eq!(mixer.stem_enabled_mask[0], STEM_COMPONENT_MASK);

        assert!(mixer.set_stem_enabled_mask(0, STEM_MASK_VOCALS | STEM_MASK_DRUMS, 42));
        assert_eq!(
            mixer.stem_enabled_mask[0],
            STEM_MASK_VOCALS | STEM_MASK_DRUMS
        );
    }

    #[test]
    fn test_render_uses_full_mix_by_default_when_prepared_stems_are_available() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 20, 0.9));
        let stems = create_test_prepared_stems_with_values(
            mixer.sample_bank[0].as_ref().unwrap(),
            1,
            44_100,
            20,
            [0.1, 0.2, 0.05, 0.0],
        );
        assert!(mixer.publish_prepared_stems(0, stems));
        assert!(mixer.play_sample(0, 1.0));

        let mut output = vec![0.0; 20];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        assert!(output.iter().all(|&sample| (sample - 0.9).abs() < 1e-5));
        assert!((pad_peaks[0] - 0.9).abs() < 1e-5);
    }

    #[test]
    fn test_render_uses_prepared_stems_in_all_stems_mode() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 20, 0.9));
        let stems = create_test_prepared_stems_with_values(
            mixer.sample_bank[0].as_ref().unwrap(),
            1,
            44_100,
            20,
            [0.1, 0.2, 0.05, 0.0],
        );
        assert!(mixer.publish_prepared_stems(0, stems));
        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
        assert!(mixer.play_sample(0, 1.0));

        let mut output = vec![0.0; 20];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        assert!(output.iter().all(|&sample| (sample - 0.35).abs() < 1e-5));
        assert!((pad_peaks[0] - 0.35).abs() < 1e-5);
    }

    #[test]
    fn test_render_uses_enabled_stem_mask_in_all_stems_mode() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 20, 0.9));
        let stems = create_test_prepared_stems_with_values(
            mixer.sample_bank[0].as_ref().unwrap(),
            1,
            44_100,
            20,
            [0.1, 0.2, 0.05, 0.4],
        );
        assert!(mixer.publish_prepared_stems(0, stems));
        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
        assert!(mixer.set_stem_enabled_mask(
            0,
            STEM_MASK_DRUMS | STEM_MASK_MELODY | STEM_MASK_BASS,
            42
        ));
        assert!(mixer.play_sample(0, 1.0));

        let mut output = vec![0.0; 20];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        assert!(output.iter().all(|&sample| (sample - 0.65).abs() < 1e-5));
        assert!((pad_peaks[0] - 0.65).abs() < 1e-5);
    }

    #[test]
    fn test_all_stems_mask_does_not_add_instrumental_artifact() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 20, 0.9));
        let stems = create_test_prepared_stems_with_values(
            mixer.sample_bank[0].as_ref().unwrap(),
            1,
            44_100,
            20,
            [0.1, 0.2, 0.05, 0.4],
        );
        assert!(mixer.publish_prepared_stems(0, stems));
        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
        assert!(mixer.set_stem_enabled_mask(0, STEM_COMPONENT_MASK, 42));
        assert!(mixer.play_sample(0, 1.0));

        let mut output = vec![0.0; 20];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        assert!(output.iter().all(|&sample| (sample - 0.75).abs() < 1e-5));
        assert!((pad_peaks[0] - 0.75).abs() < 1e-5);
    }

    #[test]
    fn test_switching_to_all_stems_preserves_voice_playhead() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 20, 0.9));
        let stems = create_test_prepared_stems_with_values(
            mixer.sample_bank[0].as_ref().unwrap(),
            1,
            44_100,
            20,
            [0.1, 0.2, 0.05, 0.0],
        );
        assert!(mixer.publish_prepared_stems(0, stems));
        assert!(mixer.play_sample(0, 1.0));

        let mut output = vec![0.0; 5];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);
        assert_eq!(active_voice_frame(&mixer, 0), Some(5));
        assert!(output.iter().all(|&sample| (sample - 0.9).abs() < 1e-5));

        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
        let mut output = vec![0.0; 5];
        mixer.render(&mut output, &mut pad_peaks);

        assert_eq!(active_voice_frame(&mixer, 0), Some(10));
        assert!((output[0] - 0.9).abs() < 1e-5);
        assert!(output[4] < output[0]);
        assert!(output[4] > 0.35);
        assert!(mixer.stem_transitions[0].is_active());

        let mut output = vec![0.0; STEM_TRANSITION_RAMP_FRAMES];
        mixer.render(&mut output, &mut pad_peaks);
        assert!(!mixer.stem_transitions[0].is_active());

        let mut output = vec![0.0; 5];
        mixer.render(&mut output, &mut pad_peaks);
        assert!(output.iter().all(|&sample| (sample - 0.35).abs() < 1e-5));
    }

    #[test]
    fn test_stem_mask_change_crossfades_and_preserves_loop_relative_source_frame() {
        let mut mixer = RtMixer::new(1, 10.0);
        let full_mix = SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from(vec![0.0; 8].into_boxed_slice()),
        };
        let vocals: Vec<f32> = (0..8).map(|frame| frame as f32).collect();
        let drums: Vec<f32> = (0..8).map(|frame| 100.0 + frame as f32).collect();
        let stems = PreparedStemSet {
            complete_set_identity: std::sync::Arc::new([0; 32]),
            accepted_timing: None,
            reference_samples: full_mix.samples.clone(),
            publication: crate::audio_engine::prepared_source::PreparedSourcePermit::unrestricted(),
            source_version_hash: 42,
            sample_rate_hz: 10,
            channels: 1,
            frame_count: 8,
            available_mask: full_stem_available_mask(),
            stems: [
                SampleBuffer {
                    residency: None,
                    channels: 1,
                    samples: Arc::from(vocals.into_boxed_slice()),
                },
                create_test_sample(1, 8, 0.0),
                create_test_sample(1, 8, 0.0),
                SampleBuffer {
                    residency: None,
                    channels: 1,
                    samples: Arc::from(drums.into_boxed_slice()),
                },
            ],
        };
        mixer.load_sample(0, full_mix);
        assert!(mixer.publish_prepared_stems(0, stems));
        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
        assert!(mixer.set_stem_enabled_mask(0, STEM_MASK_VOCALS, 42));
        mixer.set_pad_loop_region(0, 0.2, Some(0.6));
        assert!(mixer.play_sample(0, 1.0));

        let mut output = vec![0.0; 2];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);
        assert_eq!(output, vec![2.0, 3.0]);
        assert_eq!(active_voice_frame(&mixer, 0), Some(4));

        assert!(mixer.set_stem_enabled_mask(0, STEM_MASK_DRUMS, 42));
        let mut output = vec![0.0; 1];
        mixer.render(&mut output, &mut pad_peaks);

        assert!((output[0] - 4.0).abs() < 1e-4);
        assert_eq!(active_voice_frame(&mixer, 0), Some(5));
        assert!(mixer.stem_transitions[0].is_active());

        let mut output = vec![0.0; STEM_TRANSITION_RAMP_FRAMES];
        mixer.render(&mut output, &mut pad_peaks);
        assert!(!mixer.stem_transitions[0].is_active());
    }

    #[test]
    fn test_inactive_stem_mode_change_does_not_leave_stale_transition() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 20, 0.9));
        let stems = create_test_prepared_stems_with_values(
            mixer.sample_bank[0].as_ref().unwrap(),
            1,
            44_100,
            20,
            [0.1, 0.2, 0.05, 0.0],
        );
        assert!(mixer.publish_prepared_stems(0, stems));

        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
        assert!(!mixer.stem_transitions[0].is_active());

        assert!(mixer.play_sample(0, 1.0));
        let mut output = vec![0.0; 5];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        assert!(output.iter().all(|&sample| (sample - 0.35).abs() < 1e-5));
        assert!(!mixer.stem_transitions[0].is_active());
    }

    #[test]
    fn test_render_falls_back_to_full_mix_for_incomplete_prepared_stems() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 20, 0.4));

        let mut stems = create_test_prepared_stems_with_values(
            mixer.sample_bank[0].as_ref().unwrap(),
            1,
            44_100,
            20,
            [0.9, 0.0, 0.0, 0.0],
        );
        stems.available_mask = 0;
        mixer.prepared_stems[0] = Some(stems);
        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
        assert!(mixer.play_sample(0, 1.0));

        let mut output = vec![0.0; 20];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        assert!(output.iter().all(|&sample| (sample - 0.4).abs() < 1e-5));
        assert!((pad_peaks[0] - 0.4).abs() < 1e-5);
    }

    #[test]
    fn render_rejects_same_shape_stems_bound_to_a_different_source() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        let previous_source = create_test_sample(1, 20, 0.4);
        let stems = create_test_prepared_stems_with_values(
            &previous_source,
            1,
            44_100,
            20,
            [0.9, 0.0, 0.0, 0.0],
        );
        mixer.load_sample(0, create_test_sample(1, 20, 0.4));
        // Exercise render's independent validation even if a stale set bypassed admission.
        mixer.prepared_stems[0] = Some(stems);
        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
        assert!(mixer.play_sample(0, 1.0));

        let mut output = vec![0.0; 20];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        assert!(output.iter().all(|&sample| (sample - 0.4).abs() < 1e-5));
        assert!((pad_peaks[0] - 0.4).abs() < 1e-5);
    }

    #[test]
    fn test_prepared_stem_render_source_uses_loop_relative_frame_positions() {
        let mut mixer = RtMixer::new(1, 10.0);
        let full_mix = SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from(vec![100.0; 6].into_boxed_slice()),
        };
        let stem_values: [[f32; 6]; STEM_BUFFER_COUNT] = [
            [1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            [10.0, 20.0, 30.0, 40.0, 50.0, 60.0],
            [0.0; 6],
            [0.0; 6],
        ];
        let stems = PreparedStemSet {
            complete_set_identity: std::sync::Arc::new([0; 32]),
            accepted_timing: None,
            reference_samples: full_mix.samples.clone(),
            publication: crate::audio_engine::prepared_source::PreparedSourcePermit::unrestricted(),
            source_version_hash: 42,
            sample_rate_hz: 10,
            channels: 1,
            frame_count: 6,
            available_mask: full_stem_available_mask(),
            stems: std::array::from_fn(|index| SampleBuffer {
                residency: None,
                channels: 1,
                samples: Arc::from(stem_values[index].to_vec().into_boxed_slice()),
            }),
        };
        mixer.load_sample(0, full_mix.clone());
        mixer.set_pad_loop_region(0, 0.2, Some(0.5));

        let prepared_stems =
            prepared_stem_set_for_render(Some(&stems), &full_mix, 1, 10.0, 6, None).unwrap();
        let region = mixer.effective_loop_region(0, 6).unwrap();
        let mixed: Vec<f32> = (0..4)
            .map(|i| {
                let frame = region.start + (i % region.len());
                render_source_sample(
                    &full_mix,
                    Some(prepared_stems),
                    STEM_COMPONENT_MASK,
                    frame,
                    1,
                    0,
                )
            })
            .collect();

        assert_eq!(mixed, vec![33.0, 44.0, 55.0, 33.0]);
    }

    #[test]
    fn test_prepared_stem_rendering_shares_bpm_lock_playhead_timing() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 100, 0.2));
        let stems = create_test_prepared_stems_with_values(
            mixer.sample_bank[0].as_ref().unwrap(),
            1,
            44_100,
            100,
            [0.1, 0.05, 0.0, 0.0],
        );
        assert!(mixer.publish_prepared_stems(0, stems,));
        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
        mixer.set_bpm_lock(true);
        mixer.set_master_bpm(120.0);
        mixer.set_pad_bpm(0, Some(60.0));
        assert!(mixer.play_sample(0, 1.0));

        let mut output = vec![0.0; 20];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        assert_eq!(active_voice_frame(&mixer, 0), Some(40));
        assert!(output.iter().any(|&sample| sample != 0.0));
    }

    #[test]
    fn test_load_sample_invalid_id() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        let sample = create_test_sample(2, 100, 0.5);

        // Try to load at invalid ID
        mixer.load_sample(NUM_SAMPLES + 100, sample.clone());

        // Should not panic, but sample should not be loaded
        assert!(mixer.sample_bank[NUM_SAMPLES - 1].is_none());
    }

    #[test]
    fn test_load_sample_wrong_channels() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        let sample = create_test_sample(1, 100, 0.5);

        mixer.load_sample(0, sample);

        // Sample should not be loaded due to channel mismatch
        assert!(mixer.sample_bank[0].is_none());
    }

    #[test]
    fn test_play_sample() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        let sample = create_test_sample(2, 100, 0.5);
        mixer.load_sample(0, sample);

        let result = mixer.play_sample(0, 0.8);

        // Should succeed
        assert!(result);
        // One voice should be active
        assert!(mixer.voices.iter().any(|v| v.active));
    }

    #[test]
    fn test_play_sample_not_loaded() {
        let mut mixer = RtMixer::new(2, 44_100.0);

        // Try to play sample that wasn't loaded
        let result = mixer.play_sample(0, 0.8);

        // Should fail
        assert!(!result);
        // No voice should be created
        assert!(mixer.voices.iter().all(|v| !v.active));
    }

    #[test]
    fn test_play_sample_returns_false_on_invalid_id() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        let sample = create_test_sample(2, 100, 0.5);
        mixer.load_sample(0, sample);

        // Try to play with invalid ID
        let result = mixer.play_sample(NUM_SAMPLES + 10, 0.8);

        // Should fail
        assert!(!result);
    }

    #[test]
    fn test_play_sample_returns_false_on_invalid_velocity() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        let sample = create_test_sample(2, 100, 0.5);
        mixer.load_sample(0, sample);

        // Try to play with invalid velocity (out of range)
        let result = mixer.play_sample(0, 1.5);

        // Should fail
        assert!(!result);
        assert!(mixer.voices.iter().all(|v| !v.active));
    }

    #[test]
    fn test_play_sample_restarts_if_already_playing() {
        let mut mixer = RtMixer::new(1, 10.0);
        let sample = create_test_sample(1, 100, 0.5);
        mixer.load_sample(0, sample);
        mixer.set_pad_loop_region(0, 0.2, None);

        // Play sample - starts at loop start frame (2)
        mixer.play_sample(0, 0.8);

        // Check voice started at expected position
        let voice = mixer.voices.iter().find(|v| v.active).unwrap();
        assert_eq!(voice.frame_pos, 2);

        // Play again - should restart
        let result = mixer.play_sample(0, 0.6);

        // Should succeed
        assert!(result);
        // Still only one voice active
        assert_eq!(mixer.voices.iter().filter(|v| v.active).count(), 1);
        // Position should be reset to loop start (2)
        let voice = mixer.voices.iter().find(|v| v.active).unwrap();
        assert_eq!(voice.frame_pos, 2);
    }

    #[test]
    fn test_stop_sample() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        let sample1 = create_test_sample(2, 100, 0.5);
        let sample2 = create_test_sample(2, 100, 0.3);
        mixer.load_sample(0, sample1);
        mixer.load_sample(1, sample2);

        mixer.play_sample(0, 0.8);
        mixer.play_sample(1, 0.6);

        // Should have 2 active voices
        assert_eq!(mixer.voices.iter().filter(|v| v.active).count(), 2);

        mixer.stop_sample(0);

        // Only sample 1 should be stopped, sample 2 should still play
        assert!(mixer.voices.iter().any(|v| v.active && v.sample_id == 1));
        assert!(mixer.voices.iter().all(|v| !v.active || v.sample_id != 0));
    }

    #[test]
    fn test_unload_sample() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        let sample = create_test_sample(2, 100, 0.5);
        mixer.load_sample(0, sample);
        mixer.play_sample(0, 0.8);

        // Should have loaded sample and active voice
        assert!(mixer.sample_bank[0].is_some());
        assert!(mixer.voices.iter().any(|v| v.active));

        mixer.unload_sample(0);

        // Sample should be unloaded and voice stopped
        assert!(mixer.sample_bank[0].is_none());
        assert!(mixer.voices.iter().all(|v| !v.active));
    }

    #[test]
    fn test_render_silence() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        let mut output = vec![0.0; 200]; // 100 frames of stereo
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];

        mixer.render(&mut output, &mut pad_peaks);

        // Output should be silence (all zeros)
        assert!(output.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn test_render_with_voice() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        let sample = create_test_sample(2, 10, 0.5);
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.load_sample(0, sample);
        mixer.play_sample(0, 1.0);

        let mut output = vec![0.0; 20]; // 10 frames of stereo

        mixer.render(&mut output, &mut pad_peaks);

        // Output should contain sample data
        assert!(output.iter().any(|&s| s != 0.0));
    }

    #[test]
    fn test_neutral_pad_isolator_preserves_mixer_output() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        let samples = vec![0.10, -0.20, 0.30, -0.40, -0.50, 0.60, 0.70, -0.80];
        let sample = SampleBuffer {
            residency: None,
            channels: 2,
            samples: Arc::from(samples.clone().into_boxed_slice()),
        };
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];

        mixer.load_sample(0, sample);
        assert!(mixer.play_sample(0, 1.0));

        let mut output = vec![0.0; samples.len()];
        mixer.render(&mut output, &mut pad_peaks);

        for (actual, expected) in output.iter().zip(samples.iter()) {
            assert!((*actual - *expected).abs() < 1e-5);
        }
    }

    #[test]
    fn test_pad_isolator_full_kill_replaces_hardwired_eq_processing() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 128, 0.5));
        mixer.set_pad_eq(0, PAD_EQ_DB_MIN, PAD_EQ_DB_MIN, PAD_EQ_DB_MIN);
        assert!(mixer.play_sample(0, 1.0));

        let mut output = vec![0.0; 128];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        assert!(output.iter().all(|sample| sample.abs() < 1e-6));
        assert!(pad_peaks[0] < 1e-6);
    }

    #[test]
    fn test_pad_isolator_boost_is_not_double_processed_by_old_eq_path() {
        let frames = 4096;
        let sample = create_sine_sample(44_100.0, frames, 1_000.0);
        let mut neutral = RtMixer::new(1, 44_100.0);
        let mut boosted = RtMixer::new(1, 44_100.0);
        let mut neutral_peaks = [0.0_f32; NUM_SAMPLES];
        let mut boosted_peaks = [0.0_f32; NUM_SAMPLES];

        neutral.load_sample(0, sample.clone());
        boosted.load_sample(0, sample);
        boosted.set_pad_eq(0, PAD_EQ_DB_MAX, PAD_EQ_DB_MAX, PAD_EQ_DB_MAX);
        assert!(neutral.play_sample(0, 1.0));
        assert!(boosted.play_sample(0, 1.0));

        let mut neutral_output = vec![0.0; frames];
        let mut boosted_output = vec![0.0; frames];
        neutral.render(&mut neutral_output, &mut neutral_peaks);
        boosted.render(&mut boosted_output, &mut boosted_peaks);

        let neutral_rms = rms(&neutral_output[1024..]);
        let boosted_rms = rms(&boosted_output[1024..]);
        let boost_ratio = boosted_rms / neutral_rms;

        assert!(boost_ratio > 1.6);
        assert!(boost_ratio < 2.2);
    }

    #[test]
    fn test_speed_changes_affect_render_output() {
        let samples: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let sample = SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from(samples.into_boxed_slice()),
        };
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];

        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, sample.clone());

        mixer.set_speed(1.0);
        mixer.play_sample(0, 1.0);
        let mut output_1x = vec![0.0; 20];
        mixer.render(&mut output_1x, &mut pad_peaks);

        for voice in &mut mixer.voices {
            voice.stop();
        }
        mixer.set_speed(2.0);
        mixer.play_sample(0, 1.0);
        let mut output_2x = vec![0.0; 20];
        mixer.render(&mut output_2x, &mut pad_peaks);

        assert!(
            output_1x
                .iter()
                .zip(&output_2x)
                .any(|(a, b)| (*a - *b).abs() > 1e-6)
        );
    }

    #[test]
    fn test_render_loop_sample() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        let sample = create_test_sample(1, 5, 0.5);
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.load_sample(0, sample);
        mixer.play_sample(0, 1.0);

        // Render more frames than the sample contains
        let mut output = vec![0.0; 20]; // 20 frames of mono

        mixer.render(&mut output, &mut pad_peaks);

        // Sample should loop and all frames should have data.
        assert!(output.iter().all(|&s| (s - 0.5).abs() < 1e-5));
    }

    #[test]
    fn test_render_respects_custom_loop_region_frames() {
        let mut mixer = RtMixer::new(1, 10.0);
        let sample = create_test_sample(1, 20, 0.5);
        mixer.load_sample(0, sample);
        mixer.set_pad_loop_region(0, 0.2, Some(0.5));
        mixer.play_sample(0, 1.0);

        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        for _ in 0..20 {
            let mut output = vec![0.0; 1];
            mixer.render(&mut output, &mut pad_peaks);

            let frame = mixer.pad_playhead_frame[0].unwrap();
            assert!((2..5).contains(&frame));
            let seconds = mixer.pad_playhead_seconds(0).unwrap();
            assert!((seconds - frame as f64 / 10.0).abs() < 1e-6);
        }
    }

    #[test]
    fn long_seek_and_playhead_conversion_preserves_frames_without_dense_pcm() {
        for rate in [44_100_u32, 48_000, 96_000] {
            let mut mixer = RtMixer::new(1, rate as f32);
            let rate_f64 = f64::from(rate);
            for duration in [600_u32, 1_800] {
                let first = (u64::from(rate) * u64::from(duration)) as usize;
                let total_frames = first + 16;
                for frame in first..total_frames {
                    let seconds = frame as f64 / rate_f64;
                    assert_eq!(
                        mixer.source_frame_from_seconds(seconds, total_frames),
                        frame
                    );
                    mixer.pad_playhead_frame[0] = Some(frame);
                    let telemetry = mixer.pad_playhead_seconds(0).unwrap();
                    assert_eq!(telemetry, seconds);
                    assert_eq!(
                        mixer.source_frame_from_seconds(telemetry, total_frames),
                        frame
                    );
                }
                assert_eq!(
                    mixer.source_frame_from_seconds(10_000.0, total_frames),
                    total_frames
                );
                assert_eq!(mixer.source_frame_from_seconds(-1.0, total_frames), 0);
                assert_eq!(mixer.source_frame_from_seconds(f64::NAN, total_frames), 0);
                assert_eq!(
                    mixer.source_frame_from_seconds((first as f64 + 0.49) / rate_f64, total_frames),
                    first,
                );
                assert_eq!(
                    mixer.source_frame_from_seconds((first as f64 + 0.51) / rate_f64, total_frames),
                    first + 1,
                );
            }
        }
    }

    #[test]
    fn test_render_clamps_frame_pos_to_loop_start_after_update() {
        let mut mixer = RtMixer::new(1, 10.0);
        let sample = create_test_sample(1, 10, 0.5);
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.load_sample(0, sample);
        mixer.play_sample(0, 1.0);

        let mut output = vec![0.0; 5];
        mixer.render(&mut output, &mut pad_peaks);

        mixer.set_pad_loop_region(0, 0.6, Some(0.8));

        let mut output = vec![0.0; 1];
        mixer.render(&mut output, &mut pad_peaks);

        let frame = mixer.pad_playhead_frame[0].unwrap();
        assert!((6..8).contains(&frame));
    }

    #[test]
    fn test_live_loop_update_preserves_source_frame_inside_new_region() {
        let mut mixer = RtMixer::new(1, 10.0);
        let sample = SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from((0..10).map(|frame| frame as f32).collect::<Vec<_>>()),
        };
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.load_sample(0, sample);
        mixer.set_pad_loop_region(0, 0.0, Some(1.0));
        assert!(mixer.play_sample(0, 1.0));

        let mut output = vec![0.0; 6];
        mixer.render(&mut output, &mut pad_peaks);
        assert_eq!(output, vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(active_voice_frame(&mixer, 0), Some(6));

        mixer.set_pad_loop_region(0, 0.4, Some(0.9));
        let mut output = vec![0.0; 1];
        mixer.render(&mut output, &mut pad_peaks);

        assert_eq!(output, vec![6.0]);
        assert_eq!(active_voice_frame(&mixer, 0), Some(7));
    }

    #[test]
    fn test_seek_before_loop_plays_into_loop_then_wraps() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_frame_number_sample(30));
        mixer.set_pad_loop_region(0, 1.0, Some(1.8));
        assert!(mixer.play_sample(0, 1.0));

        assert!(mixer.seek_sample(0, 0.5));
        assert_eq!(active_voice_frame(&mixer, 0), Some(5));

        let mut output = vec![0.0; 16];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        assert_eq!(
            output,
            vec![
                5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 10.0,
                11.0, 12.0,
            ]
        );
        assert_eq!(active_voice_frame(&mixer, 0), Some(13));
    }

    #[test]
    fn test_seek_inside_loop_uses_normal_loop_wrapping() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_frame_number_sample(30));
        mixer.set_pad_loop_region(0, 1.0, Some(1.8));
        assert!(mixer.play_sample(0, 1.0));

        assert!(mixer.seek_sample(0, 1.2));

        let mut output = vec![0.0; 10];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        assert_eq!(
            output,
            vec![12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 10.0, 11.0, 12.0, 13.0]
        );
        assert_eq!(active_voice_frame(&mixer, 0), Some(14));
    }

    #[test]
    fn test_seek_after_loop_plays_to_track_end_then_wraps_to_loop() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_frame_number_sample(30));
        mixer.set_pad_loop_region(0, 1.0, Some(1.8));
        assert!(mixer.play_sample(0, 1.0));

        assert!(mixer.seek_sample(0, 2.2));

        let mut output = vec![0.0; 12];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        assert_eq!(
            output,
            vec![
                22.0, 23.0, 24.0, 25.0, 26.0, 27.0, 28.0, 29.0, 10.0, 11.0, 12.0, 13.0
            ]
        );
        assert_eq!(active_voice_frame(&mixer, 0), Some(14));
    }

    #[test]
    fn test_seek_paused_voice_keeps_paused_state_until_resume() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_frame_number_sample(30));
        mixer.set_pad_loop_region(0, 1.0, Some(1.8));
        assert!(mixer.play_sample(0, 1.0));
        mixer.pause_sample(0);

        assert!(mixer.seek_sample(0, 2.2));

        let mut output = vec![1.0; 4];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        assert_eq!(output, vec![0.0, 0.0, 0.0, 0.0]);
        assert_eq!(active_voice_frame(&mixer, 0), Some(22));
        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| { voice.active && voice.sample_id == 0 && voice.paused })
        );

        mixer.resume_sample(0);
        let mut output = vec![0.0; 10];
        mixer.render(&mut output, &mut pad_peaks);

        assert_eq!(
            output,
            vec![22.0, 23.0, 24.0, 25.0, 26.0, 27.0, 28.0, 29.0, 10.0, 11.0]
        );
    }

    #[test]
    fn test_seek_stopped_sample_is_noop() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_frame_number_sample(30));

        assert!(!mixer.seek_sample(0, 1.2));
        assert_eq!(mixer.pad_playhead_seconds(0), None);
        assert_eq!(active_voice_frame(&mixer, 0), None);
    }

    #[test]
    fn test_live_loop_update_after_explicit_seek_keeps_existing_clamp_behavior() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_frame_number_sample(30));
        mixer.set_pad_loop_region(0, 1.0, Some(1.8));
        assert!(mixer.play_sample(0, 1.0));
        assert!(mixer.seek_sample(0, 2.2));

        mixer.set_pad_loop_region(0, 1.2, Some(1.6));

        let mut output = vec![0.0; 1];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        assert_eq!(output, vec![12.0]);
        assert_eq!(active_voice_frame(&mixer, 0), Some(13));
    }

    #[test]
    fn test_multiple_voices_mixing() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        let sample1 = create_test_sample(2, 10, 0.3);
        let sample2 = create_test_sample(2, 10, 0.2);
        mixer.load_sample(0, sample1);
        mixer.load_sample(1, sample2);

        mixer.play_sample(0, 1.0);
        mixer.play_sample(1, 1.0);

        let mut output = vec![0.0; 20]; // 10 frames of stereo

        mixer.render(&mut output, &mut pad_peaks);

        // Output should contain mixed samples (0.3 + 0.2 = 0.5 per channel).
        assert!(output.iter().all(|&s| (s - 0.5).abs() < 1e-5));
    }

    #[test]
    fn test_pad_gain_applies_to_render() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        let sample = create_test_sample(1, 5, 0.8);
        mixer.load_sample(0, sample);
        mixer.set_pad_gain(0, -6.0);
        mixer.play_sample(0, 1.0);

        let mut output = vec![0.0; 20]; // 20 frames of mono
        mixer.render(&mut output, &mut pad_peaks);

        let expected = 0.8 * gain_db_to_linear(-6.0);
        assert!(output.iter().all(|&s| (s - expected).abs() < 1e-6));
    }

    #[test]
    fn test_pad_gain_db_to_linear_reference_values() {
        assert!((gain_db_to_linear(0.0) - 1.0).abs() < 1e-6);
        assert!((gain_db_to_linear(6.0) - 1.995_262_4).abs() < 1e-6);
        assert!((gain_db_to_linear(-6.0) - 0.501_187_2).abs() < 1e-6);
    }

    #[test]
    fn test_pad_gain_boost_applies_to_render() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.load_sample(0, create_test_sample(1, 5, 0.25));
        mixer.set_pad_gain(0, 6.0);
        mixer.play_sample(0, 1.0);

        let mut output = vec![0.0; 20];
        mixer.render(&mut output, &mut pad_peaks);

        let expected = 0.25 * gain_db_to_linear(6.0);
        assert!(
            output
                .iter()
                .all(|&sample| (sample - expected).abs() < 1e-6)
        );
    }

    #[test]
    fn test_active_pad_gain_changes_are_smoothed() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 1024, 1.0));
        assert!(mixer.play_sample(0, 1.0));

        mixer.set_pad_gain(0, 12.0);
        let target = gain_db_to_linear(12.0);
        assert!((mixer.pad_gain_smoothers[0].current() - 1.0).abs() < 1e-6);
        assert!(mixer.pad_gain_smoothers[0].frames_remaining > 0);

        let first_smoothed_value = mixer.pad_gain_smoothers[0].next();
        assert!(first_smoothed_value > 1.0);
        assert!(first_smoothed_value < target);
    }

    #[test]
    fn test_voice_limit() {
        let mut mixer = RtMixer::new(1, 44_100.0);

        // Create MAX_VOICES + 5 samples
        let mut success_count = 0;
        for i in 0..(MAX_VOICES + 5) {
            let sample = create_test_sample(1, 10, 0.5);
            mixer.load_sample(i, sample);
            if mixer.play_sample(i, 1.0) {
                success_count += 1;
            }
        }

        // First MAX_VOICES should succeed
        assert_eq!(success_count, MAX_VOICES);
        // Only MAX_VOICES voices should be active
        assert_eq!(mixer.voices.iter().filter(|v| v.active).count(), MAX_VOICES);
    }

    #[test]
    fn test_pause_sample() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        let sample = create_test_sample(1, 100, 0.5);
        mixer.load_sample(0, sample);
        mixer.play_sample(0, 1.0);

        // Should have active voice
        let voice = mixer
            .voices
            .iter()
            .find(|v| v.active && v.sample_id == 0)
            .unwrap();
        assert!(!voice.paused);
        let frame_before = voice.frame_pos;

        mixer.pause_sample(0);

        let voice = mixer
            .voices
            .iter()
            .find(|v| v.active && v.sample_id == 0)
            .unwrap();
        assert!(voice.paused);
        // frame_pos should be unchanged after pause
        assert_eq!(voice.frame_pos, frame_before);
    }

    #[test]
    fn test_resume_sample() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        let sample = create_test_sample(1, 100, 0.5);
        mixer.load_sample(0, sample);
        mixer.play_sample(0, 1.0);

        // Pause first
        mixer.pause_sample(0);
        let frame_before_resume = mixer
            .voices
            .iter()
            .find(|v| v.active && v.sample_id == 0)
            .unwrap()
            .frame_pos;

        mixer.resume_sample(0);

        let voice = mixer
            .voices
            .iter()
            .find(|v| v.active && v.sample_id == 0)
            .unwrap();
        assert!(!voice.paused);
        // frame_pos should be same as before resume
        assert_eq!(voice.frame_pos, frame_before_resume);
    }

    #[test]
    fn test_pause_and_resume_affects_mixing() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        let sample = create_test_sample(1, 100, 0.5); // 100 frames
        mixer.load_sample(0, sample);
        mixer.play_sample(0, 1.0);

        let mut output = vec![0.0; 20];
        let mut peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut peaks);

        // After render, output should have non-zero values
        assert!(output.iter().any(|&x| x != 0.0));
        let frame_after_first_render = mixer
            .voices
            .iter()
            .find(|v| v.active && v.sample_id == 0)
            .unwrap()
            .frame_pos;
        assert_eq!(frame_after_first_render, 20); // advanced 20 frames

        // Pause, then render again: output should be silence (since paused)
        mixer.pause_sample(0);
        let mut output2 = vec![0.0; 20];
        mixer.render(&mut output2, &mut peaks);
        // Output should be all zeros (silence)
        assert!(output2.iter().all(|&x| x == 0.0));
        // frame_pos should not have advanced
        let frame_after_pause = mixer
            .voices
            .iter()
            .find(|v| v.active && v.sample_id == 0)
            .unwrap()
            .frame_pos;
        assert_eq!(frame_after_pause, frame_after_first_render);

        // Resume and render again: should produce output again
        mixer.resume_sample(0);
        let mut output3 = vec![0.0; 20];
        mixer.render(&mut output3, &mut peaks);
        assert!(output3.iter().any(|&x| x != 0.0));
        // frame_pos should have advanced by another 20
        let frame_after_resume = mixer
            .voices
            .iter()
            .find(|v| v.active && v.sample_id == 0)
            .unwrap()
            .frame_pos;
        assert_eq!(frame_after_resume, frame_after_pause + 20);
    }
}

#[cfg(test)]
#[path = "productive_history_tests.rs"]
mod productive_history_tests;

#[cfg(test)]
#[path = "prepared_native_mixer_tests.rs"]
mod prepared_native_mixer_tests;

#[cfg(test)]
#[path = "resident_mixer_tests.rs"]
mod resident_mixer_tests;

#[cfg(test)]
#[path = "resident_context_proof_tests.rs"]
mod resident_context_proof_tests;

#[cfg(test)]
#[path = "mixer_source_tests.rs"]
mod source_tests;
