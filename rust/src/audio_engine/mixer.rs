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
use crate::audio_engine::constants::{
    MAX_VOICES, NUM_SAMPLES, PAD_EQ_DB_MAX, PAD_EQ_DB_MIN, PAD_GAIN_DB_DEFAULT, PAD_GAIN_DB_MAX,
    PAD_GAIN_DB_MIN, PAD_GAIN_SMOOTH_MS, SPEED_MAX, SPEED_MIN, VOLUME_MAX, VOLUME_MIN,
};
use crate::audio_engine::dsp::{DspNodeSlot, DspParameterId, DspParameterSlot, PerPadDspChain};
use crate::audio_engine::stretch_processor::DEFAULT_BLOCK_SAMPLES;
use crate::audio_engine::voice_slot::{ExplicitSeekMode, VoiceSlot};
use crate::messages::{
    KeyLockQuality, KeyLockSettings, PadTimingMetadata, PreparedStemSet, STEM_BUFFER_COUNT,
    STEM_COMPONENT_MASK, SampleBuffer, StemMixMode,
};
use cpal::Sample;

const BEATS_PER_BAR_4_4: f64 = 4.0;
const BAR_PHASE_EPSILON: f64 = 1.0e-9;
const STEM_TRANSITION_RAMP_FRAMES: usize = 128;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameRange {
    start: usize,
    end: usize,
}

impl FrameRange {
    fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }
}

fn explicit_seek_mode_for_frame(
    frame: usize,
    loop_region: FrameRange,
    sample_frames: usize,
) -> ExplicitSeekMode {
    if frame < loop_region.start {
        ExplicitSeekMode::BeforeLoop
    } else if frame >= loop_region.end && loop_region.end < sample_frames {
        ExplicitSeekMode::AfterLoop
    } else {
        ExplicitSeekMode::Normal
    }
}

fn source_frame_for_playback(
    frame_pos: usize,
    offset: usize,
    sample_frames: usize,
    loop_region: FrameRange,
    seek_mode: ExplicitSeekMode,
) -> usize {
    let loop_len = loop_region.len();
    debug_assert!(loop_len > 0);

    match seek_mode {
        ExplicitSeekMode::Normal => {
            let base = frame_pos.saturating_sub(loop_region.start);
            loop_region.start + ((base + offset) % loop_len)
        }
        ExplicitSeekMode::BeforeLoop => {
            let frame = frame_pos.saturating_add(offset);
            if frame < loop_region.start {
                frame
            } else {
                loop_region.start + ((frame - loop_region.start) % loop_len)
            }
        }
        ExplicitSeekMode::AfterLoop => {
            let frame = frame_pos.saturating_add(offset);
            if frame < sample_frames {
                frame
            } else {
                loop_region.start + ((frame - sample_frames) % loop_len)
            }
        }
    }
}

fn advance_playback_position(
    frame_pos: usize,
    input_frames: usize,
    sample_frames: usize,
    loop_region: FrameRange,
    seek_mode: ExplicitSeekMode,
) -> (usize, ExplicitSeekMode) {
    let loop_len = loop_region.len();
    debug_assert!(loop_len > 0);

    match seek_mode {
        ExplicitSeekMode::Normal => {
            let base = frame_pos.saturating_sub(loop_region.start);
            (
                loop_region.start + ((base + input_frames) % loop_len),
                ExplicitSeekMode::Normal,
            )
        }
        ExplicitSeekMode::BeforeLoop => {
            let frame = frame_pos.saturating_add(input_frames);
            if frame < loop_region.start {
                (frame, ExplicitSeekMode::BeforeLoop)
            } else {
                (
                    loop_region.start + ((frame - loop_region.start) % loop_len),
                    ExplicitSeekMode::Normal,
                )
            }
        }
        ExplicitSeekMode::AfterLoop => {
            let frame = frame_pos.saturating_add(input_frames);
            if frame < sample_frames {
                (frame, ExplicitSeekMode::AfterLoop)
            } else {
                (
                    loop_region.start + ((frame - sample_frames) % loop_len),
                    ExplicitSeekMode::Normal,
                )
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StemRenderSelection {
    mode: StemMixMode,
    source_version_hash: u64,
    enabled_mask: u8,
}

impl StemRenderSelection {
    fn full_mix() -> Self {
        Self {
            mode: StemMixMode::FullMix,
            source_version_hash: 0,
            enabled_mask: STEM_COMPONENT_MASK,
        }
    }

    fn from_state(
        mode: StemMixMode,
        source_version_hash: u64,
        enabled_mask: u8,
    ) -> StemRenderSelection {
        match mode {
            StemMixMode::FullMix => StemRenderSelection::full_mix(),
            StemMixMode::AllStems => StemRenderSelection {
                mode,
                source_version_hash,
                enabled_mask: enabled_mask & STEM_COMPONENT_MASK,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StemTransition {
    from: StemRenderSelection,
    elapsed_frames: usize,
    total_frames: usize,
}

impl StemTransition {
    fn inactive() -> Self {
        Self {
            from: StemRenderSelection::full_mix(),
            elapsed_frames: 0,
            total_frames: 0,
        }
    }

    fn start(from: StemRenderSelection, total_frames: usize) -> Self {
        if total_frames == 0 {
            return Self::inactive();
        }

        Self {
            from,
            elapsed_frames: 0,
            total_frames,
        }
    }

    fn is_active(self) -> bool {
        self.total_frames > 0 && self.elapsed_frames < self.total_frames
    }

    fn gains_at(self, frame_offset: usize) -> (f32, f32) {
        if !self.is_active() {
            return (0.0, 1.0);
        }

        let elapsed = self
            .elapsed_frames
            .saturating_add(frame_offset)
            .min(self.total_frames);
        let to_gain = elapsed as f32 / self.total_frames as f32;
        (1.0 - to_gain, to_gain)
    }

    fn advance(&mut self, frames: usize) {
        if !self.is_active() {
            return;
        }

        self.elapsed_frames = self.elapsed_frames.saturating_add(frames);
        if self.elapsed_frames >= self.total_frames {
            self.clear();
        }
    }

    fn clear(&mut self) {
        *self = Self::inactive();
    }
}

impl Default for StemTransition {
    fn default() -> Self {
        Self::inactive()
    }
}

fn phase_aligned_initial_frame(
    sample_rate_hz: f32,
    pad_bpm: Option<f32>,
    phase_anchor_frame: Option<usize>,
    target_bar_phase_beats: f64,
    loop_region: Option<FrameRange>,
    fallback_frame: usize,
) -> usize {
    let Some(region) = loop_region.filter(|region| region.end > region.start) else {
        return fallback_frame;
    };
    let Some(anchor_frame) = phase_anchor_frame else {
        return fallback_frame;
    };
    let Some(pad_bpm) = pad_bpm.filter(|bpm| bpm.is_finite() && *bpm > 0.0) else {
        return fallback_frame;
    };
    let Some(target_bar_phase_beats) = normalize_bar_phase_beats(target_bar_phase_beats) else {
        return fallback_frame;
    };
    if !sample_rate_hz.is_finite() || sample_rate_hz <= 0.0 {
        return fallback_frame;
    }

    let frames_per_beat = sample_rate_hz as f64 * 60.0 / pad_bpm as f64;
    if !frames_per_beat.is_finite() || frames_per_beat <= 0.0 {
        return fallback_frame;
    }

    let desired_frame = anchor_frame as f64 + target_bar_phase_beats * frames_per_beat;
    wrap_frame_into_region(desired_frame, region).unwrap_or(fallback_frame)
}

fn normalize_bar_phase_beats(phase: f64) -> Option<f64> {
    if !phase.is_finite() {
        return None;
    }

    let phase = phase.rem_euclid(BEATS_PER_BAR_4_4);
    if phase <= BAR_PHASE_EPSILON || (BEATS_PER_BAR_4_4 - phase) <= BAR_PHASE_EPSILON {
        Some(0.0)
    } else {
        Some(phase)
    }
}

fn wrap_frame_into_region(frame: f64, region: FrameRange) -> Option<usize> {
    if !frame.is_finite() || region.end <= region.start {
        return None;
    }

    let len = region.len() as f64;
    if !len.is_finite() || len <= 0.0 {
        return None;
    }

    let relative = (frame - region.start as f64).rem_euclid(len);
    let rounded = relative.round();
    let offset = if rounded >= len { 0 } else { rounded as usize };

    Some(region.start + offset)
}

fn full_stem_available_mask() -> u8 {
    ((1_u16 << STEM_BUFFER_COUNT) - 1) as u8
}

fn stem_index_mask(index: usize) -> u8 {
    if index >= u8::BITS as usize {
        return 0;
    }

    1_u8 << index
}

fn prepared_stem_set_matches_sample(
    stems: &PreparedStemSet,
    sample: &SampleBuffer,
    channels: usize,
    sample_rate_hz: f32,
    sample_frames: usize,
) -> bool {
    if channels == 0 || sample_frames == 0 || sample.samples.len() != sample_frames * channels {
        return false;
    }

    let rounded_sample_rate_hz = sample_rate_hz.round();
    if !rounded_sample_rate_hz.is_finite()
        || rounded_sample_rate_hz <= 0.0
        || stems.sample_rate_hz != rounded_sample_rate_hz as u32
    {
        return false;
    }

    if stems.channels != channels
        || stems.frame_count != sample_frames
        || stems.available_mask != full_stem_available_mask()
        || stems.source_version_hash == 0
    {
        return false;
    }

    stems
        .stems
        .iter()
        .all(|stem| stem.channels == channels && stem.samples.len() == sample.samples.len())
}

fn prepared_stem_set_for_render<'a>(
    stems: Option<&'a PreparedStemSet>,
    sample: &SampleBuffer,
    channels: usize,
    sample_rate_hz: f32,
    sample_frames: usize,
) -> Option<&'a PreparedStemSet> {
    stems.filter(|stems| {
        prepared_stem_set_matches_sample(stems, sample, channels, sample_rate_hz, sample_frames)
    })
}

fn render_source_sample(
    sample: &SampleBuffer,
    stems: Option<&PreparedStemSet>,
    enabled_stem_mask: u8,
    frame: usize,
    channels: usize,
    channel: usize,
) -> f32 {
    let index = frame * channels + channel;
    if let Some(stems) = stems {
        let enabled_stem_mask = enabled_stem_mask & STEM_COMPONENT_MASK;
        stems
            .stems
            .iter()
            .enumerate()
            .filter(|(stem_index, _)| enabled_stem_mask & stem_index_mask(*stem_index) != 0)
            .map(|(_, stem)| stem.samples[index])
            .sum()
    } else {
        sample.samples[index]
    }
}

fn render_source_selection_sample(
    sample: &SampleBuffer,
    stems: Option<&PreparedStemSet>,
    selection: StemRenderSelection,
    frame: usize,
    channels: usize,
    channel: usize,
) -> f32 {
    match selection.mode {
        StemMixMode::FullMix => sample.samples[frame * channels + channel],
        StemMixMode::AllStems => {
            let matching_stems = stems.filter(|stems| {
                selection.source_version_hash != 0
                    && stems.source_version_hash == selection.source_version_hash
            });
            render_source_sample(
                sample,
                matching_stems,
                selection.enabled_mask,
                frame,
                channels,
                channel,
            )
        }
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
    speed: f32,

    /// Enable BPM lock (tempo matching).
    bpm_lock_enabled: bool,

    /// Enable key lock (preserve pitch when tempo changes).
    key_lock_enabled: bool,

    /// Global bounded Key Lock DSP parameters.
    key_lock_settings: KeyLockSettings,

    /// Current master BPM when BPM lock is enabled.
    master_bpm: Option<f32>,

    /// Effective pad BPM metadata (manual override or analysis).
    pad_bpm: [Option<f32>; NUM_SAMPLES],

    /// Per-pad musical phase anchor derived from bounded beatgrid/downbeat metadata.
    pad_phase_anchor_frame: [usize; NUM_SAMPLES],

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

    /// Active voices with MAX_VOICES slots.
    pub voices: [VoiceSlot; MAX_VOICES],
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
    pub fn new(channels: usize, sample_rate_hz: f32) -> Self {
        let sample_rate_hz = if sample_rate_hz.is_finite() && sample_rate_hz > 0.0 {
            sample_rate_hz
        } else {
            44_100.0
        };

        Self {
            channels,
            sample_rate_hz,
            volume: VOLUME_MAX,
            speed: 1.0,
            bpm_lock_enabled: false,
            key_lock_enabled: false,
            key_lock_settings: KeyLockSettings::default(),
            master_bpm: None,
            pad_bpm: std::array::from_fn(|_| None),
            pad_phase_anchor_frame: std::array::from_fn(|_| 0),
            pad_gain_db: std::array::from_fn(|_| PAD_GAIN_DB_DEFAULT),
            pad_gain_smoothers: std::array::from_fn(|_| SmoothedGain::default()),
            pad_dsp_chains: (0..NUM_SAMPLES)
                .map(|id| PerPadDspChain::new(id, sample_rate_hz, DEFAULT_BLOCK_SAMPLES, channels))
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
            voices: std::array::from_fn(|_| VoiceSlot::with_sample_rate(channels, sample_rate_hz)),
        }
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

        if sample.channels != self.channels {
            retirement.retire_sample(sample);
            return false;
        }

        if let Some(old_sample) = self.sample_bank[id].take() {
            retirement.retire_sample(old_sample);
        }
        if let Some(old_stems) = self.prepared_stems[id].take() {
            retirement.retire_prepared_stems(old_stems);
        }

        self.sample_bank[id] = Some(sample);
        self.stem_enabled_mask[id] = STEM_COMPONENT_MASK;
        self.stem_transitions[id].clear();
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
        if !self.can_accept_prepared_stems(id, &stems) {
            retirement.retire_prepared_stems(stems);
            return false;
        }

        if let Some(old_stems) = self.prepared_stems[id].take() {
            retirement.retire_prepared_stems(old_stems);
        }

        self.prepared_stems[id] = Some(stems);
        self.stem_transitions[id].clear();
        true
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

        if self.sample_is_active(id) {
            self.reset_voice_stretch_for_sample(id);
            self.stem_transitions[id] =
                StemTransition::start(previous, STEM_TRANSITION_RAMP_FRAMES);
        } else {
            self.stem_transitions[id].clear();
        }
    }

    fn reset_voice_stretch_for_sample(&mut self, id: usize) {
        for voice in &mut self.voices {
            if voice.active && voice.sample_id == id {
                voice.stretch.reset();
            }
        }
    }

    fn can_accept_prepared_stems(&self, id: usize, stems: &PreparedStemSet) -> bool {
        if id >= NUM_SAMPLES || self.channels == 0 || self.sample_is_active(id) {
            return false;
        }

        let Some(sample) = self.sample_bank[id].as_ref() else {
            return false;
        };

        let sample_frames = sample.samples.len() / self.channels;
        prepared_stem_set_matches_sample(
            stems,
            sample,
            self.channels,
            self.sample_rate_hz,
            sample_frames,
        )
    }

    fn sample_is_active(&self, id: usize) -> bool {
        self.voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == id)
    }

    pub(crate) fn can_play_sample(&self, id: usize, velocity: f32) -> bool {
        id < NUM_SAMPLES
            && velocity.is_finite()
            && (VOLUME_MIN..=VOLUME_MAX).contains(&velocity)
            && self.sample_bank[id].is_some()
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

    pub(crate) fn play_sample_rt(
        &mut self,
        id: usize,
        velocity: f32,
        retirement: &mut impl AudioBufferRetirement,
    ) -> bool {
        self.play_sample_with_phase_rt(id, velocity, None, retirement)
    }

    #[cfg(test)]
    pub(crate) fn play_sample_phase_aligned(
        &mut self,
        id: usize,
        velocity: f32,
        target_bar_phase_beats: f64,
    ) -> bool {
        let mut retirement = ImmediateAudioBufferRetirement;
        self.play_sample_with_phase_rt(id, velocity, Some(target_bar_phase_beats), &mut retirement)
    }

    fn play_sample_with_phase_rt(
        &mut self,
        id: usize,
        velocity: f32,
        target_bar_phase_beats: Option<f64>,
        retirement: &mut impl AudioBufferRetirement,
    ) -> bool {
        if !self.can_play_sample(id, velocity) {
            return false;
        }

        let Some(sample) = self.sample_bank[id].as_ref() else {
            return false;
        };
        let sample = sample.clone();

        let tempo_ratio = self.tempo_ratio_for_sample_id(id);

        let sample_frames = sample.samples.len() / self.channels;
        let initial_frame_pos = target_bar_phase_beats
            .map(|phase| self.phase_aligned_initial_sample_frame(id, sample_frames, phase))
            .unwrap_or_else(|| self.effective_loop_start_frame(id, sample_frames));

        // Sample is already playing? -> reset play position
        for voice_slot in &mut self.voices {
            if voice_slot.active && voice_slot.sample_id == id {
                self.stem_transitions[id].clear();
                voice_slot.restart(initial_frame_pos, velocity, tempo_ratio);
                return true;
            }
        }

        // Start new voice slot
        for voice_slot in &mut self.voices {
            if !voice_slot.active {
                self.stem_transitions[id].clear();
                self.pad_dsp_chains[id].reset();
                voice_slot.start_rt(
                    id,
                    sample.clone(),
                    initial_frame_pos,
                    velocity,
                    tempo_ratio,
                    retirement,
                );
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
    pub fn set_speed(&mut self, speed: f32) {
        if !speed.is_finite() || !(SPEED_MIN..=SPEED_MAX).contains(&speed) {
            return;
        }

        self.speed = speed;
    }

    pub fn set_bpm_lock(&mut self, enabled: bool) {
        self.bpm_lock_enabled = enabled;
        if !enabled {
            self.master_bpm = None;
        }
    }

    pub fn set_key_lock(&mut self, enabled: bool) {
        self.key_lock_enabled = enabled;
    }

    pub fn set_key_lock_quality(&mut self, quality: KeyLockQuality) {
        self.key_lock_settings = KeyLockSettings::from_quality(quality).sanitized();
    }

    pub fn set_key_lock_settings(&mut self, settings: KeyLockSettings) {
        self.key_lock_settings = settings.sanitized();
    }

    pub fn set_master_bpm(&mut self, bpm: f32) {
        if !bpm.is_finite() || bpm <= 0.0 {
            return;
        }

        self.master_bpm = Some(bpm);
    }

    pub fn set_pad_bpm(&mut self, id: usize, bpm: Option<f32>) {
        if id >= NUM_SAMPLES {
            return;
        }

        let bpm = bpm.and_then(|value| {
            if !value.is_finite() || value <= 0.0 {
                None
            } else {
                Some(value)
            }
        });

        self.pad_bpm[id] = bpm;
    }

    pub fn set_pad_timing_metadata(&mut self, id: usize, metadata: PadTimingMetadata) {
        if id >= NUM_SAMPLES {
            return;
        }

        self.pad_phase_anchor_frame[id] =
            self.timing_anchor_frame_from_seconds(metadata.phase_anchor_s);
    }

    #[allow(dead_code)]
    pub(crate) fn phase_aligned_initial_sample_frame(
        &self,
        id: usize,
        sample_frames: usize,
        target_bar_phase_beats: f64,
    ) -> usize {
        if id >= NUM_SAMPLES {
            return 0;
        }

        let fallback_frame = self.effective_loop_start_frame(id, sample_frames);
        let phase_anchor_frame =
            Some(self.pad_phase_anchor_frame[id]).filter(|frame| *frame < sample_frames);

        phase_aligned_initial_frame(
            self.sample_rate_hz,
            self.pad_bpm[id],
            phase_anchor_frame,
            target_bar_phase_beats,
            self.phase_alignment_loop_region(id, sample_frames),
            fallback_frame,
        )
    }

    #[cfg(test)]
    fn pad_phase_anchor_frame(&self, id: usize) -> Option<usize> {
        if id >= NUM_SAMPLES {
            return None;
        }

        Some(self.pad_phase_anchor_frame[id])
    }

    fn timing_anchor_frame_from_seconds(&self, phase_anchor_s: f32) -> usize {
        if !phase_anchor_s.is_finite() || phase_anchor_s < 0.0 {
            return 0;
        }

        let frame = phase_anchor_s as f64 * self.sample_rate_hz as f64;
        if !frame.is_finite() || frame < 0.0 {
            return 0;
        }

        let max = usize::MAX as f64;
        if frame >= max {
            return usize::MAX;
        }

        frame.round() as usize
    }

    fn source_frame_from_seconds(&self, position_s: f32, sample_frames: usize) -> usize {
        if !position_s.is_finite() || position_s < 0.0 {
            return 0;
        }

        let frame = position_s as f64 * self.sample_rate_hz as f64;
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

        let start = self.pad_loop_start_frame[id].min(sample_frames);
        let end = self.pad_loop_end_frame[id]
            .unwrap_or(sample_frames)
            .min(sample_frames);

        if end <= start {
            Some(FrameRange {
                start: 0,
                end: sample_frames,
            })
        } else {
            Some(FrameRange { start, end })
        }
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

    pub fn set_pad_loop_region(&mut self, id: usize, start_s: f32, end_s: Option<f32>) {
        if id >= NUM_SAMPLES {
            return;
        }

        if !start_s.is_finite() || start_s < 0.0 {
            return;
        }

        let start_frame = (start_s * self.sample_rate_hz).round();
        let start_frame = if start_frame.is_finite() && start_frame >= 0.0 {
            start_frame as usize
        } else {
            0
        };

        let end_frame = end_s.and_then(|end_s| {
            if !end_s.is_finite() || end_s < 0.0 {
                return None;
            }
            let end_frame = (end_s * self.sample_rate_hz).round();
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

        self.pad_loop_start_frame[id] = start_frame;
        self.pad_loop_end_frame[id] = end_frame;

        for voice_slot in &mut self.voices {
            if voice_slot.is_playing_sample(id) {
                voice_slot.clear_explicit_seek();
            }
        }
    }

    pub fn seek_sample(&mut self, id: usize, position_s: f32) -> bool {
        if id >= NUM_SAMPLES || !position_s.is_finite() || position_s < 0.0 || self.channels == 0 {
            return false;
        }

        let Some(sample) = self.sample_bank[id].as_ref() else {
            return false;
        };
        let sample_frames = sample.samples.len() / self.channels;
        if sample_frames == 0 {
            return false;
        }

        let Some(loop_region) = self.effective_loop_region(id, sample_frames) else {
            return false;
        };
        let target_frame = self.source_frame_from_seconds(position_s, sample_frames);
        let seek_mode = explicit_seek_mode_for_frame(target_frame, loop_region, sample_frames);

        let mut did_seek = false;
        for voice_slot in &mut self.voices {
            if voice_slot.is_playing_sample(id) {
                voice_slot.seek(target_frame, seek_mode);
                did_seek = true;
            }
        }

        if did_seek {
            self.pad_playhead_frame[id] = Some(target_frame);
        }

        did_seek
    }

    pub fn pad_playhead_seconds(&self, id: usize) -> Option<f32> {
        if id >= NUM_SAMPLES {
            return None;
        }
        let frame = self.pad_playhead_frame[id]?;
        Some(frame as f32 / self.sample_rate_hz)
    }

    pub(crate) fn active_pad_bar_phase_beats(&self, id: usize) -> Option<f64> {
        if id >= NUM_SAMPLES || self.channels == 0 {
            return None;
        }

        let voice = self
            .voices
            .iter()
            .find(|voice| voice.active && !voice.paused && voice.sample_id == id)?;
        let sample = voice.sample.as_ref()?;
        let sample_frames = sample.samples.len() / self.channels;

        self.pad_bar_phase_beats_at_frame(id, sample_frames, voice.frame_pos)
    }

    pub(crate) fn output_bpm_for_sample_id(&self, id: usize) -> Option<f32> {
        if id >= NUM_SAMPLES {
            return None;
        }

        let pad_bpm = self.pad_bpm[id].filter(|bpm| bpm.is_finite() && *bpm > 0.0)?;
        let bpm = pad_bpm * self.tempo_ratio_for_sample_id(id);
        if bpm.is_finite() && bpm > 0.0 {
            Some(bpm)
        } else {
            None
        }
    }

    fn pad_bar_phase_beats_at_frame(
        &self,
        id: usize,
        sample_frames: usize,
        frame_pos: usize,
    ) -> Option<f64> {
        if id >= NUM_SAMPLES || sample_frames == 0 || frame_pos >= sample_frames {
            return None;
        }

        let pad_bpm = self.pad_bpm[id].filter(|bpm| bpm.is_finite() && *bpm > 0.0)?;
        if !self.sample_rate_hz.is_finite() || self.sample_rate_hz <= 0.0 {
            return None;
        }

        let anchor_frame = self.pad_phase_anchor_frame[id];
        if anchor_frame >= sample_frames {
            return None;
        }

        let frames_per_beat = self.sample_rate_hz as f64 * 60.0 / pad_bpm as f64;
        if !frames_per_beat.is_finite() || frames_per_beat <= 0.0 {
            return None;
        }

        let pad_phase_beats = (frame_pos as f64 - anchor_frame as f64) / frames_per_beat;
        normalize_bar_phase_beats(pad_phase_beats)
    }

    fn tempo_ratio_for_sample_id(&self, sample_id: usize) -> f32 {
        let mut ratio = self.speed;

        if self.bpm_lock_enabled
            && let (Some(master_bpm), Some(pad_bpm)) = (self.master_bpm, self.pad_bpm[sample_id])
        {
            ratio = master_bpm / pad_bpm;
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
    pub fn pause_sample(&mut self, id: usize) {
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
    pub fn resume_sample(&mut self, id: usize) {
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
        self.pad_phase_anchor_frame[id] = 0;
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

    pub(crate) fn render_rt(
        &mut self,
        output: &mut [f32],
        pad_peaks: &mut [f32; NUM_SAMPLES],
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

                self.render_rt_chunk(&mut output[start..end], &mut chunk_peaks, retirement);
                for (peak, chunk_peak) in pad_peaks.iter_mut().zip(chunk_peaks.iter()) {
                    *peak = (*peak).max(*chunk_peak);
                }

                rendered_frames += chunk_frames;
            }

            return;
        }

        self.render_rt_chunk(output, pad_peaks, retirement);
    }

    fn render_rt_chunk(
        &mut self,
        output: &mut [f32],
        pad_peaks: &mut [f32; NUM_SAMPLES],
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
        let speed = self.speed;
        let volume = self.volume;
        let bpm_lock_enabled = self.bpm_lock_enabled;
        let key_lock_enabled = self.key_lock_enabled;
        let key_lock_settings = self.key_lock_settings;
        let master_bpm = self.master_bpm;
        let pad_bpm = &self.pad_bpm;
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

        for voice in &mut self.voices {
            if !voice.active {
                continue;
            }

            let is_paused = voice.paused;

            let Some(sample) = voice.sample.clone() else {
                voice.stop_rt(retirement);
                continue;
            };

            if !is_paused {
                let sample_frames = sample.samples.len() / channels;
                if sample_frames == 0 {
                    voice.stop_rt(retirement);
                    continue;
                }
                let prepared_stem_set = prepared_stem_set_for_render(
                    prepared_stem_slots[voice.sample_id].as_ref(),
                    &sample,
                    channels,
                    sample_rate_hz,
                    sample_frames,
                );
                let current_selection = StemRenderSelection::from_state(
                    stem_mix_mode[voice.sample_id],
                    stem_mix_source_version_hash[voice.sample_id],
                    stem_enabled_mask[voice.sample_id],
                );
                let stem_transition = stem_transitions[voice.sample_id];

                let mut target_tempo_ratio = speed;
                if bpm_lock_enabled
                    && let (Some(master_bpm), Some(pad_bpm)) =
                        (master_bpm, pad_bpm[voice.sample_id])
                {
                    target_tempo_ratio = master_bpm / pad_bpm;
                }

                if !target_tempo_ratio.is_finite() {
                    target_tempo_ratio = 1.0;
                }
                target_tempo_ratio = target_tempo_ratio.clamp(SPEED_MIN, SPEED_MAX);

                let tempo_ratio = voice.smooth_tempo_ratio(target_tempo_ratio, key_lock_settings);

                let mut input_frames = ((frames as f32) * tempo_ratio).round() as usize;
                input_frames = input_frames.clamp(1, DEFAULT_BLOCK_SAMPLES);

                let mut loop_start = pad_loop_start_frame[voice.sample_id].min(sample_frames);
                let mut loop_end = pad_loop_end_frame[voice.sample_id].unwrap_or(sample_frames);
                loop_end = loop_end.min(sample_frames);
                if loop_end <= loop_start {
                    loop_start = 0;
                    loop_end = sample_frames;
                }
                let loop_len = loop_end - loop_start;
                if loop_len == 0 {
                    voice.stop_rt(retirement);
                    continue;
                }

                let loop_region = FrameRange {
                    start: loop_start,
                    end: loop_end,
                };
                let mut seek_mode = voice.explicit_seek_mode;
                if seek_mode == ExplicitSeekMode::Normal
                    && (voice.frame_pos < loop_start || voice.frame_pos >= loop_end)
                {
                    voice.frame_pos = loop_start;
                }
                if voice.frame_pos > sample_frames {
                    voice.frame_pos = sample_frames;
                }
                if seek_mode == ExplicitSeekMode::BeforeLoop && voice.frame_pos >= loop_start {
                    seek_mode = ExplicitSeekMode::Normal;
                    voice.explicit_seek_mode = ExplicitSeekMode::Normal;
                }
                if seek_mode == ExplicitSeekMode::AfterLoop && voice.frame_pos < loop_end {
                    seek_mode = ExplicitSeekMode::Normal;
                    voice.explicit_seek_mode = ExplicitSeekMode::Normal;
                }
                let source_frame_pos = voice.frame_pos;

                let input_buffers = voice.stretch.input_buffers_mut(input_frames);
                for (channel, buf) in input_buffers.iter_mut().enumerate().take(channels) {
                    for (i, sample_ref) in buf.iter_mut().enumerate().take(input_frames) {
                        let frame = source_frame_for_playback(
                            source_frame_pos,
                            i,
                            sample_frames,
                            loop_region,
                            seek_mode,
                        );
                        *sample_ref = if stem_transition.is_active() {
                            let from_sample = render_source_selection_sample(
                                &sample,
                                prepared_stem_set,
                                stem_transition.from,
                                frame,
                                channels,
                                channel,
                            );
                            let to_sample = render_source_selection_sample(
                                &sample,
                                prepared_stem_set,
                                current_selection,
                                frame,
                                channels,
                                channel,
                            );
                            let (from_gain, to_gain) = stem_transition.gains_at(i);
                            from_sample * from_gain + to_sample * to_gain
                        } else {
                            render_source_selection_sample(
                                &sample,
                                prepared_stem_set,
                                current_selection,
                                frame,
                                channels,
                                channel,
                            )
                        };
                    }
                }
                stem_transitions[voice.sample_id].advance(input_frames);

                voice.stretch.process(
                    input_frames,
                    frames,
                    tempo_ratio,
                    key_lock_enabled,
                    key_lock_settings,
                );

                let pad_dsp_chain = &mut pad_dsp_chains[voice.sample_id];
                let pad_gain_smoother = &mut pad_gain_smoothers[voice.sample_id];

                let output_buffers = voice.stretch.output_buffers();
                for frame in 0..frames {
                    let out_base = frame * channels;
                    let trim_gain = pad_gain_smoother.next();
                    pad_dsp_chain.begin_frame();
                    for (channel, buffer) in output_buffers.iter().enumerate().take(channels) {
                        let sample = buffer[frame] * trim_gain;
                        let sample = pad_dsp_chain.process_sample(channel, sample);
                        let contribution = sample * voice.volume;
                        let mixed = contribution * volume;
                        output[out_base + channel] += mixed;

                        let peak = contribution.abs();
                        if peak > pad_peaks[voice.sample_id] {
                            pad_peaks[voice.sample_id] = peak;
                        }
                    }
                }

                let (next_frame_pos, next_seek_mode) = advance_playback_position(
                    source_frame_pos,
                    input_frames,
                    sample_frames,
                    loop_region,
                    seek_mode,
                );
                voice.frame_pos = next_frame_pos;
                voice.explicit_seek_mode = next_seek_mode;
            } else {
                let sample_frames = sample.samples.len() / channels;
                let loop_start = pad_loop_start_frame[voice.sample_id].min(sample_frames);

                let mut loop_end = pad_loop_end_frame[voice.sample_id].unwrap_or(sample_frames);
                loop_end = loop_end.min(sample_frames);
                if loop_end <= loop_start {
                    // Invalid loop; but voice is paused; skip.
                } else if voice.explicit_seek_mode == ExplicitSeekMode::Normal
                    && (voice.frame_pos < loop_start || voice.frame_pos >= loop_end)
                {
                    voice.frame_pos = loop_start;
                } else if voice.frame_pos > sample_frames {
                    voice.frame_pos = sample_frames;
                }
            }
            pad_playhead_frame[voice.sample_id] = Some(voice.frame_pos);
        }
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
            channels: 1,
            samples: Arc::from(samples.into_boxed_slice()),
        }
    }

    fn create_sine_prepared_stems(
        sample_rate_hz: u32,
        frames: usize,
        frequency_hz: f32,
    ) -> PreparedStemSet {
        let source = create_sine_sample(sample_rate_hz as f32, frames, frequency_hz);
        let silence = create_test_sample(1, frames, 0.0);

        PreparedStemSet {
            source_version_hash: 42,
            sample_rate_hz,
            channels: 1,
            frame_count: frames,
            available_mask: full_stem_available_mask(),
            stems: [
                source,
                silence.clone(),
                silence.clone(),
                silence.clone(),
                silence,
            ],
        }
    }

    fn create_frame_number_sample(frames: usize) -> SampleBuffer {
        SampleBuffer {
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

    fn active_voice_frame(mixer: &RtMixer, id: usize) -> Option<usize> {
        mixer
            .voices
            .iter()
            .find(|voice| voice.active && voice.sample_id == id)
            .map(|voice| voice.frame_pos)
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
        fn retire_sample(&mut self, sample: SampleBuffer) {
            self.samples.push(sample);
        }

        fn retire_prepared_stems(&mut self, stems: PreparedStemSet) {
            self.stems.push(stems);
        }

        fn available_retirement_slots(&mut self) -> usize {
            usize::MAX
        }
    }

    fn create_test_prepared_stems(
        channels: usize,
        sample_rate_hz: u32,
        frames: usize,
    ) -> PreparedStemSet {
        let buffer = create_test_sample(channels, frames, 0.25);
        PreparedStemSet {
            source_version_hash: 42,
            sample_rate_hz,
            channels,
            frame_count: frames,
            available_mask: full_stem_available_mask(),
            stems: std::array::from_fn(|_| buffer.clone()),
        }
    }

    fn create_test_prepared_stems_with_values(
        channels: usize,
        sample_rate_hz: u32,
        frames: usize,
        values: [f32; STEM_BUFFER_COUNT],
    ) -> PreparedStemSet {
        PreparedStemSet {
            source_version_hash: 42,
            sample_rate_hz,
            channels,
            frame_count: frames,
            available_mask: full_stem_available_mask(),
            stems: values.map(|value| create_test_sample(channels, frames, value)),
        }
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
            source_version_hash: 42,
            sample_rate_hz: 44_100,
            channels: 1,
            frame_count: 32,
            available_mask: full_stem_available_mask(),
            stems: std::array::from_fn(|_| SampleBuffer {
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
    fn key_lock_settings_default_to_high_values_and_can_change() {
        let mut mixer = RtMixer::new(2, 48_000.0);

        assert_eq!(mixer.key_lock_settings, KeyLockSettings::default());

        mixer.set_key_lock_quality(KeyLockQuality::VeryHigh);

        assert_eq!(
            mixer.key_lock_settings,
            KeyLockSettings::from_quality(KeyLockQuality::VeryHigh).sanitized()
        );

        let custom = KeyLockSettings {
            delay_min_samples: 128.0,
            delay_range_samples: 1024.0,
            head_count: 4,
            interpolation: crate::messages::KeyLockInterpolation::Linear,
            window: crate::messages::KeyLockWindow::Triangle,
            smoothing_step: 0.04,
            output_gain: 1.2,
        };
        mixer.set_key_lock_settings(custom);

        assert_eq!(mixer.key_lock_settings, custom);
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
            create_sine_prepared_stems(sample_rate_hz as u32, frames, source_hz)
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
    fn test_pad_timing_metadata_stores_sample_accurate_anchor_frame() {
        let mut mixer = RtMixer::new(1, 10.0);

        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 1.2,
            },
        );

        assert_eq!(mixer.pad_phase_anchor_frame(0), Some(12));
    }

    #[test]
    fn test_pad_timing_metadata_invalid_values_fall_back_to_zero() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 1.2,
            },
        );

        for phase_anchor_s in [f32::NAN, f32::INFINITY, -1.0] {
            mixer.set_pad_timing_metadata(0, PadTimingMetadata { phase_anchor_s });
            assert_eq!(mixer.pad_phase_anchor_frame(0), Some(0));
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

        assert_eq!(mixer.pad_phase_anchor_frame(0), Some(0));
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

        assert_eq!(mixer.pad_phase_anchor_frame(0), Some(0));
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
        mixer.pad_phase_anchor_frame[0] = 50;

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
        mixer.pad_phase_anchor_frame[0] = 20;
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
        assert!(mixer.publish_prepared_stems(0, create_test_prepared_stems(2, 44_100, 100)));

        mixer.load_sample(0, create_test_sample(2, 100, 0.25));

        assert!(mixer.prepared_stems[0].is_none());
    }

    #[test]
    fn test_publish_prepared_stems_accepts_stopped_loaded_pad() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        mixer.load_sample(0, create_test_sample(2, 100, 0.5));

        assert!(mixer.publish_prepared_stems(0, create_test_prepared_stems(2, 44_100, 100)));

        assert_eq!(
            mixer.prepared_stems[0]
                .as_ref()
                .map(|stems| stems.source_version_hash),
            Some(42)
        );
    }

    #[test]
    fn test_publish_prepared_stems_rejects_active_pad() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 100, 0.5));
        assert!(mixer.play_sample(0, 1.0));

        assert!(!mixer.publish_prepared_stems(0, create_test_prepared_stems(1, 44_100, 100)));

        assert!(mixer.prepared_stems[0].is_none());
    }

    #[test]
    fn test_publish_prepared_stems_rejects_mismatched_layout() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        mixer.load_sample(0, create_test_sample(2, 100, 0.5));

        assert!(!mixer.publish_prepared_stems(0, create_test_prepared_stems(1, 44_100, 100)));
        assert!(!mixer.publish_prepared_stems(0, create_test_prepared_stems(2, 48_000, 100)));
        assert!(!mixer.publish_prepared_stems(0, create_test_prepared_stems(2, 44_100, 99)));

        assert!(mixer.prepared_stems[0].is_none());
    }

    #[test]
    fn test_set_stem_mix_mode_requires_matching_prepared_source() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 20, 0.9));

        assert!(!mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
        assert_eq!(mixer.stem_mix_mode[0], StemMixMode::FullMix);

        assert!(mixer.publish_prepared_stems(0, create_test_prepared_stems(1, 44_100, 20)));
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
        assert!(mixer.publish_prepared_stems(0, create_test_prepared_stems(1, 44_100, 20)));
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

        assert!(mixer.publish_prepared_stems(0, create_test_prepared_stems(1, 44_100, 20)));
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
        let stems =
            create_test_prepared_stems_with_values(1, 44_100, 20, [0.1, 0.2, 0.05, 0.0, 0.15]);
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
        let stems =
            create_test_prepared_stems_with_values(1, 44_100, 20, [0.1, 0.2, 0.05, 0.0, 0.15]);
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
        let stems =
            create_test_prepared_stems_with_values(1, 44_100, 20, [0.1, 0.2, 0.05, 0.4, 0.15]);
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
        let stems =
            create_test_prepared_stems_with_values(1, 44_100, 20, [0.1, 0.2, 0.05, 0.4, 0.8]);
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
        let stems =
            create_test_prepared_stems_with_values(1, 44_100, 20, [0.1, 0.2, 0.05, 0.0, 0.15]);
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
            channels: 1,
            samples: Arc::from(vec![0.0; 8].into_boxed_slice()),
        };
        let vocals: Vec<f32> = (0..8).map(|frame| frame as f32).collect();
        let drums: Vec<f32> = (0..8).map(|frame| 100.0 + frame as f32).collect();
        let stems = PreparedStemSet {
            source_version_hash: 42,
            sample_rate_hz: 10,
            channels: 1,
            frame_count: 8,
            available_mask: full_stem_available_mask(),
            stems: [
                SampleBuffer {
                    channels: 1,
                    samples: Arc::from(vocals.into_boxed_slice()),
                },
                create_test_sample(1, 8, 0.0),
                create_test_sample(1, 8, 0.0),
                SampleBuffer {
                    channels: 1,
                    samples: Arc::from(drums.into_boxed_slice()),
                },
                create_test_sample(1, 8, 0.0),
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
        let stems =
            create_test_prepared_stems_with_values(1, 44_100, 20, [0.1, 0.2, 0.05, 0.0, 0.15]);
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

        let mut stems =
            create_test_prepared_stems_with_values(1, 44_100, 20, [0.9, 0.0, 0.0, 0.0, 0.0]);
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
    fn test_prepared_stem_render_source_uses_loop_relative_frame_positions() {
        let mut mixer = RtMixer::new(1, 10.0);
        let full_mix = SampleBuffer {
            channels: 1,
            samples: Arc::from(vec![100.0; 6].into_boxed_slice()),
        };
        let stem_values: [[f32; 6]; STEM_BUFFER_COUNT] = [
            [1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            [10.0, 20.0, 30.0, 40.0, 50.0, 60.0],
            [0.0; 6],
            [0.0; 6],
            [0.0; 6],
        ];
        let stems = PreparedStemSet {
            source_version_hash: 42,
            sample_rate_hz: 10,
            channels: 1,
            frame_count: 6,
            available_mask: full_stem_available_mask(),
            stems: std::array::from_fn(|index| SampleBuffer {
                channels: 1,
                samples: Arc::from(stem_values[index].to_vec().into_boxed_slice()),
            }),
        };
        mixer.load_sample(0, full_mix.clone());
        mixer.set_pad_loop_region(0, 0.2, Some(0.5));

        let prepared_stems =
            prepared_stem_set_for_render(Some(&stems), &full_mix, 1, 10.0, 6).unwrap();
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
        let stems =
            create_test_prepared_stems_with_values(1, 44_100, 100, [0.1, 0.05, 0.0, 0.0, 0.05]);
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
        assert_eq!(
            mixer.pad_dsp_chains[0].prepared_state(),
            (44_100.0, DEFAULT_BLOCK_SAMPLES, 2)
        );
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
            assert!((seconds - frame as f32 / 10.0).abs() < 1e-6);
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
