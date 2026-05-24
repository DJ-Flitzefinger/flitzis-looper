//! Real-time audio mixer implementation.
//!
//! This module provides the [`RtMixer`] struct which handles real-time mixing
//! of multiple audio voices with sample loading, playback, and mixing capabilities.
//!
//! The mixer manages a collection of [`VoiceSlot`](crate::audio_engine::voice_slot::VoiceSlot) instances
//! and operates on [`SampleBuffer`](crate::messages::SampleBuffer) data loaded via
//! [`decode_audio_file_to_sample_buffer`](crate::audio_engine::sample_loader::decode_audio_file_to_sample_buffer).

use crate::audio_engine::constants::{
    MAX_VOICES, NUM_SAMPLES, PAD_EQ_DB_MAX, PAD_EQ_DB_MIN, PAD_GAIN_MAX, PAD_GAIN_MIN, SPEED_MAX,
    SPEED_MIN, VOLUME_MAX, VOLUME_MIN,
};
use crate::audio_engine::eq3::{Eq3Coeffs, coeffs_for_eq3};
use crate::audio_engine::stretch_processor::DEFAULT_BLOCK_SAMPLES;
use crate::audio_engine::voice_slot::VoiceSlot;
use crate::messages::{
    PadTimingMetadata, PreparedStemSet, STEM_BUFFER_COUNT, STEM_COMPONENT_MASK, SampleBuffer,
    StemMixMode,
};
use cpal::Sample;

const BEATS_PER_BAR_4_4: f64 = 4.0;
const BAR_PHASE_EPSILON: f64 = 1.0e-9;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TransportClockSource {
    pub(crate) bpm: f32,
    pub(crate) bar_phase_beats: f64,
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

fn transpose_semitones_for_tempo_ratio(tempo_ratio: f32) -> f32 {
    if !tempo_ratio.is_finite() || tempo_ratio <= 0.0 {
        return 0.0;
    }

    -12.0 * tempo_ratio.log2()
}

fn phase_aligned_initial_frame(
    sample_rate_hz: f32,
    pad_bpm: Option<f32>,
    phase_anchor_frame: Option<usize>,
    target_bar_phase_beats: f64,
    catch_up_sample_frames: f64,
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

    let catch_up_sample_frames = if catch_up_sample_frames.is_finite() {
        catch_up_sample_frames.max(0.0)
    } else {
        0.0
    };
    let desired_frame =
        anchor_frame as f64 + target_bar_phase_beats * frames_per_beat + catch_up_sample_frames;
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

    /// Current master BPM when BPM lock is enabled.
    master_bpm: Option<f32>,

    /// Effective pad BPM metadata (manual override or analysis).
    pad_bpm: [Option<f32>; NUM_SAMPLES],

    /// Per-pad musical phase anchor derived from bounded beatgrid/downbeat metadata.
    pad_phase_anchor_frame: [usize; NUM_SAMPLES],

    /// Per-pad gain scalar (linear, 0.0..=1.0).
    pad_gain: [f32; NUM_SAMPLES],

    /// Per-pad EQ coefficients (low/mid/high).
    pad_eq: [Eq3Coeffs; NUM_SAMPLES],

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
            master_bpm: None,
            pad_bpm: std::array::from_fn(|_| None),
            pad_phase_anchor_frame: std::array::from_fn(|_| 0),
            pad_gain: std::array::from_fn(|_| 1.0),
            pad_eq: std::array::from_fn(|_| coeffs_for_eq3(sample_rate_hz, 0.0, 0.0, 0.0)),
            pad_loop_start_frame: std::array::from_fn(|_| 0),
            pad_loop_end_frame: std::array::from_fn(|_| None),
            pad_playhead_frame: std::array::from_fn(|_| None),
            sample_bank: std::array::from_fn(|_| None),
            prepared_stems: Box::new(std::array::from_fn(|_| None)),
            stem_mix_mode: std::array::from_fn(|_| StemMixMode::FullMix),
            stem_mix_source_version_hash: std::array::from_fn(|_| 0),
            stem_enabled_mask: std::array::from_fn(|_| STEM_COMPONENT_MASK),
            voices: std::array::from_fn(|_| VoiceSlot::new(channels)),
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
    pub fn load_sample(&mut self, id: usize, sample: SampleBuffer) {
        if id >= NUM_SAMPLES {
            return;
        }

        if sample.channels != self.channels {
            return;
        }

        self.sample_bank[id] = Some(sample);
        self.prepared_stems[id] = None;
        self.stem_enabled_mask[id] = STEM_COMPONENT_MASK;
    }

    pub(crate) fn publish_prepared_stems(&mut self, id: usize, stems: PreparedStemSet) -> bool {
        if !self.can_accept_prepared_stems(id, &stems) {
            return false;
        }

        self.prepared_stems[id] = Some(stems);
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

        match mode {
            StemMixMode::FullMix => {
                self.stem_mix_mode[id] = StemMixMode::FullMix;
                self.stem_mix_source_version_hash[id] = 0;
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

        self.stem_enabled_mask[id] = enabled_stem_mask;
        true
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
    pub fn play_sample(&mut self, id: usize, velocity: f32) -> bool {
        self.play_sample_with_phase(id, velocity, None)
    }

    #[allow(dead_code)]
    pub(crate) fn play_sample_phase_aligned(
        &mut self,
        id: usize,
        velocity: f32,
        target_bar_phase_beats: f64,
    ) -> bool {
        self.play_sample_with_phase(id, velocity, Some((target_bar_phase_beats, 0)))
    }

    pub(crate) fn play_sample_phase_aligned_with_catch_up(
        &mut self,
        id: usize,
        velocity: f32,
        target_bar_phase_beats: f64,
        catch_up_output_frames: u64,
    ) -> bool {
        self.play_sample_with_phase(
            id,
            velocity,
            Some((target_bar_phase_beats, catch_up_output_frames)),
        )
    }

    fn play_sample_with_phase(
        &mut self,
        id: usize,
        velocity: f32,
        target_bar_phase_beats: Option<(f64, u64)>,
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
            .map(|(phase, catch_up_frames)| {
                self.phase_aligned_initial_sample_frame_with_catch_up(
                    id,
                    sample_frames,
                    phase,
                    catch_up_frames,
                )
            })
            .unwrap_or_else(|| self.effective_loop_start_frame(id, sample_frames));

        // Sample is already playing? -> reset play position
        for voice_slot in &mut self.voices {
            if voice_slot.active && voice_slot.sample_id == id {
                voice_slot.restart(initial_frame_pos, velocity, tempo_ratio);
                return true;
            }
        }

        // Start new voice slot
        for voice_slot in &mut self.voices {
            if !voice_slot.active {
                voice_slot.start(id, sample.clone(), initial_frame_pos, velocity, tempo_ratio);
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
        self.phase_aligned_initial_sample_frame_with_catch_up(
            id,
            sample_frames,
            target_bar_phase_beats,
            0,
        )
    }

    pub(crate) fn phase_aligned_initial_sample_frame_with_catch_up(
        &self,
        id: usize,
        sample_frames: usize,
        target_bar_phase_beats: f64,
        catch_up_output_frames: u64,
    ) -> usize {
        if id >= NUM_SAMPLES {
            return 0;
        }

        let fallback_frame = self.effective_loop_start_frame(id, sample_frames);
        let phase_anchor_frame =
            Some(self.pad_phase_anchor_frame[id]).filter(|frame| *frame < sample_frames);
        let catch_up_sample_frames =
            catch_up_output_frames as f64 * self.tempo_ratio_for_sample_id(id) as f64;

        phase_aligned_initial_frame(
            self.sample_rate_hz,
            self.pad_bpm[id],
            phase_anchor_frame,
            target_bar_phase_beats,
            catch_up_sample_frames,
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

    pub fn set_pad_gain(&mut self, id: usize, gain: f32) {
        if id >= NUM_SAMPLES {
            return;
        }

        if !gain.is_finite() || !(PAD_GAIN_MIN..=PAD_GAIN_MAX).contains(&gain) {
            return;
        }

        self.pad_gain[id] = gain;
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

        self.pad_eq[id] = coeffs_for_eq3(self.sample_rate_hz, low_db, mid_db, high_db);
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

    pub(crate) fn active_transport_clock_source(&self) -> Option<TransportClockSource> {
        let voice = self
            .voices
            .iter()
            .find(|voice| voice.active && !voice.paused)?;
        let sample = voice.sample.as_ref()?;
        if self.channels == 0 {
            return None;
        }
        let sample_frames = sample.samples.len() / self.channels;
        let bpm = self.output_bpm_for_sample_id(voice.sample_id)?;
        let bar_phase_beats =
            self.pad_bar_phase_beats_at_frame(voice.sample_id, sample_frames, voice.frame_pos)?;

        Some(TransportClockSource {
            bpm,
            bar_phase_beats,
        })
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

    pub(crate) fn has_active_voice(&self) -> bool {
        self.voices
            .iter()
            .any(|voice| voice.active && !voice.paused)
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
    pub fn stop_sample(&mut self, id: usize) {
        if id >= NUM_SAMPLES {
            return;
        }

        for voice_slot in &mut self.voices {
            if voice_slot.is_playing_sample(id) {
                voice_slot.stop();
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
    pub fn unload_sample(&mut self, id: usize) {
        if id >= NUM_SAMPLES {
            return;
        }

        self.stop_sample(id);
        self.sample_bank[id] = None;
        self.prepared_stems[id] = None;
        self.stem_enabled_mask[id] = STEM_COMPONENT_MASK;
        self.pad_phase_anchor_frame[id] = 0;
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
    pub fn render(&mut self, output: &mut [f32], pad_peaks: &mut [f32; NUM_SAMPLES]) {
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

        let speed = self.speed;
        let bpm_lock_enabled = self.bpm_lock_enabled;
        let key_lock_enabled = self.key_lock_enabled;
        let master_bpm = self.master_bpm;
        let pad_bpm = &self.pad_bpm;
        let pad_gain = &self.pad_gain;
        let pad_eq = &self.pad_eq;
        let prepared_stem_slots = &self.prepared_stems;
        let stem_mix_mode = &self.stem_mix_mode;
        let stem_mix_source_version_hash = &self.stem_mix_source_version_hash;
        let stem_enabled_mask = &self.stem_enabled_mask;

        for voice in &mut self.voices {
            if !voice.active {
                continue;
            }

            let is_paused = voice.paused;

            let Some(sample) = voice.sample.clone() else {
                voice.stop();
                continue;
            };

            if !is_paused {
                let sample_frames = sample.samples.len() / self.channels;
                if sample_frames == 0 {
                    voice.stop();
                    continue;
                }
                let prepared_stem_set = if stem_mix_mode[voice.sample_id] == StemMixMode::AllStems {
                    prepared_stem_set_for_render(
                        prepared_stem_slots[voice.sample_id].as_ref(),
                        &sample,
                        self.channels,
                        self.sample_rate_hz,
                        sample_frames,
                    )
                    .filter(|stems| {
                        stems.source_version_hash == stem_mix_source_version_hash[voice.sample_id]
                    })
                } else {
                    None
                };

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

                let tempo_ratio = voice.smooth_tempo_ratio(target_tempo_ratio);
                let transpose_semitones = if key_lock_enabled {
                    transpose_semitones_for_tempo_ratio(tempo_ratio)
                } else {
                    0.0
                };

                let mut input_frames = ((frames as f32) * tempo_ratio).round() as usize;
                input_frames = input_frames.clamp(1, DEFAULT_BLOCK_SAMPLES);

                let mut loop_start = self.pad_loop_start_frame[voice.sample_id].min(sample_frames);
                let mut loop_end =
                    self.pad_loop_end_frame[voice.sample_id].unwrap_or(sample_frames);
                loop_end = loop_end.min(sample_frames);
                if loop_end <= loop_start {
                    loop_start = 0;
                    loop_end = sample_frames;
                }
                let loop_len = loop_end - loop_start;
                if loop_len == 0 {
                    voice.stop();
                    continue;
                }

                if voice.frame_pos < loop_start || voice.frame_pos >= loop_end {
                    voice.frame_pos = loop_start;
                }
                let base = voice.frame_pos - loop_start;

                let input_buffers = voice.stretch.input_buffers_mut(input_frames);
                for (channel, buf) in input_buffers.iter_mut().enumerate().take(self.channels) {
                    for (i, sample_ref) in buf.iter_mut().enumerate().take(input_frames) {
                        let frame = loop_start + ((base + i) % loop_len);
                        *sample_ref = render_source_sample(
                            &sample,
                            prepared_stem_set,
                            stem_enabled_mask[voice.sample_id],
                            frame,
                            self.channels,
                            channel,
                        );
                    }
                }

                voice.stretch.set_transpose_semitones(transpose_semitones);
                voice.stretch.process(input_frames, frames);

                let eq = pad_eq[voice.sample_id];
                let pad_gain = pad_gain[voice.sample_id];

                let output_buffers = voice.stretch.output_buffers();
                for frame in 0..frames {
                    let out_base = frame * self.channels;
                    for (channel, buffer) in output_buffers.iter().enumerate().take(self.channels) {
                        let sample = buffer[frame];
                        let mut sample = sample;
                        if let Some(state) = voice.eq_state.get_mut(channel) {
                            sample = eq.process(state, sample);
                        }
                        let mixed = sample * voice.volume * self.volume * pad_gain;
                        output[out_base + channel] += mixed;

                        let peak = mixed.abs();
                        if peak > pad_peaks[voice.sample_id] {
                            pad_peaks[voice.sample_id] = peak;
                        }
                    }
                }

                voice.frame_pos = loop_start + ((base + input_frames) % loop_len);
            } else {
                let sample_frames = sample.samples.len() / self.channels;
                let loop_start = self.pad_loop_start_frame[voice.sample_id].min(sample_frames);

                let mut loop_end =
                    self.pad_loop_end_frame[voice.sample_id].unwrap_or(sample_frames);
                loop_end = loop_end.min(sample_frames);
                if loop_end <= loop_start {
                    // Invalid loop; but voice is paused; skip.
                } else if voice.frame_pos < loop_start || voice.frame_pos >= loop_end {
                    voice.frame_pos = loop_start;
                }
            }
            self.pad_playhead_frame[voice.sample_id] = Some(voice.frame_pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::audio_engine::eq3::Eq3State;
    use crate::messages::{STEM_MASK_BASS, STEM_MASK_DRUMS, STEM_MASK_MELODY, STEM_MASK_VOCALS};

    use super::*;

    fn create_test_sample(channels: usize, frames: usize, value: f32) -> SampleBuffer {
        let samples = vec![value; channels * frames];
        SampleBuffer {
            channels,
            samples: Arc::from(samples.into_boxed_slice()),
        }
    }

    fn active_voice_frame(mixer: &RtMixer, id: usize) -> Option<usize> {
        mixer
            .voices
            .iter()
            .find(|voice| voice.active && voice.sample_id == id)
            .map(|voice| voice.frame_pos)
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
    fn test_transpose_semitones_for_tempo_ratio() {
        assert!((transpose_semitones_for_tempo_ratio(1.0) - 0.0).abs() < 1e-6);
        assert!((transpose_semitones_for_tempo_ratio(2.0) - (-12.0)).abs() < 1e-6);
        assert!((transpose_semitones_for_tempo_ratio(0.5) - 12.0).abs() < 1e-6);
    }

    #[test]
    fn test_eq3_coeffs_finite() {
        let eq = coeffs_for_eq3(44_100.0, 6.0, -3.0, 0.0);

        for coeffs in eq.low_lp.iter().chain(eq.high_hp.iter()) {
            for v in [coeffs.b0, coeffs.b1, coeffs.b2, coeffs.a1, coeffs.a2] {
                assert!(v.is_finite());
            }
        }

        for g in [eq.low_gain, eq.mid_gain, eq.high_gain] {
            assert!(g.is_finite());
            assert!(g >= 0.0);
        }
    }

    #[test]
    fn test_eq3_processing_stable_on_impulse() {
        let eq = coeffs_for_eq3(44_100.0, 0.0, 6.0, 0.0);
        let mut state = Eq3State::default();
        let mut max_abs: f32 = 0.0;
        for i in 0..512 {
            let x = if i == 0 { 1.0 } else { 0.0 };
            let y = eq.process(&mut state, x);
            assert!(y.is_finite());
            max_abs = max_abs.max(y.abs());
        }
        assert!(max_abs > 0.0);
    }

    #[test]
    fn test_eq3_unity_reconstruction() {
        let eq = coeffs_for_eq3(44_100.0, 0.0, 0.0, 0.0);
        let mut state = Eq3State::default();

        for i in 0..1024 {
            let x = (i as f32 * 0.01).sin() * 0.75;
            let y = eq.process(&mut state, x);
            assert!(y.is_finite());
            assert!((y - x).abs() < 1e-5);
        }
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
        assert!(output.iter().all(|&sample| (sample - 0.35).abs() < 1e-5));
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
        mixer.set_pad_gain(0, 0.25);
        mixer.play_sample(0, 1.0);

        let mut output = vec![0.0; 20]; // 20 frames of mono
        mixer.render(&mut output, &mut pad_peaks);

        let expected = 0.8 * 0.25;
        assert!(output.iter().all(|&s| (s - expected).abs() < 1e-6));
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
