//! Internal realtime-safe DSP/FX foundation helpers.
//!
//! The per-pad chain hosts the first visible DSP node: a bounded 3-band DJ isolator. Existing
//! Python-facing EQ calls remain compatible, while live audio state is owned by typed normalized
//! DSP parameters and Rust-side smoothing.

#![allow(dead_code)]

use std::f32::consts::PI;

const DEFAULT_SAMPLE_RATE_HZ: f32 = 44_100.0;
const DEFAULT_MAX_BLOCK_FRAMES: usize = 1;
const DEFAULT_CHANNELS: usize = 1;
const DEFAULT_NORMALIZED_VALUE: f32 = 0.5;
const DEFAULT_SMOOTHING_STEP: f32 = 0.01;
const DSP_MAX_CHANNELS: usize = 8;
const ISOLATOR_LOW_CROSSOVER_HZ: f32 = 250.0;
const ISOLATOR_HIGH_CROSSOVER_HZ: f32 = 4_000.0;
const ISOLATOR_BOOST_DB_MAX: f32 = 6.0;
const BUTTERWORTH_Q: f32 = 0.70710677;

pub(crate) const DSP_PARAMETER_SLOTS: usize = 4;
pub(crate) const NORMALIZED_PARAMETER_MIN: f32 = 0.0;
pub(crate) const NORMALIZED_PARAMETER_MAX: f32 = 1.0;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DspScopeKind {
    Pad = 0,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DspNodeSlot {
    Slot0 = 0,
}

#[repr(u8)]
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DspParameterSlot {
    Slot0 = 0,
    Slot1 = 1,
    Slot2 = 2,
    Slot3 = 3,
}

impl DspParameterSlot {
    const fn index(self) -> usize {
        match self {
            DspParameterSlot::Slot0 => 0,
            DspParameterSlot::Slot1 => 1,
            DspParameterSlot::Slot2 => 2,
            DspParameterSlot::Slot3 => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DspParameterId {
    scope_kind: DspScopeKind,
    scope_index: u16,
    node_slot: DspNodeSlot,
    parameter_slot: DspParameterSlot,
}

impl DspParameterId {
    pub(crate) fn per_pad(
        pad_id: usize,
        node_slot: DspNodeSlot,
        parameter_slot: DspParameterSlot,
    ) -> Option<Self> {
        Some(Self {
            scope_kind: DspScopeKind::Pad,
            scope_index: u16::try_from(pad_id).ok()?,
            node_slot,
            parameter_slot,
        })
    }

    fn matches_pad_chain(self, pad_id: u16) -> bool {
        self.scope_kind == DspScopeKind::Pad
            && self.scope_index == pad_id
            && self.node_slot == DspNodeSlot::Slot0
            && self.parameter_slot.index() < DSP_PARAMETER_SLOTS
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SmoothedNormalizedValue {
    current: f32,
    target: f32,
    max_step: f32,
}

impl SmoothedNormalizedValue {
    pub(crate) fn new(initial: f32, max_step: f32) -> Self {
        let initial = sanitize_normalized(initial, DEFAULT_NORMALIZED_VALUE);
        Self {
            current: initial,
            target: initial,
            max_step: sanitize_smoothing_step(max_step),
        }
    }

    pub(crate) fn set_target(&mut self, target: f32) -> bool {
        if !target.is_finite() {
            return false;
        }

        self.target = target.clamp(NORMALIZED_PARAMETER_MIN, NORMALIZED_PARAMETER_MAX);
        true
    }

    pub(crate) fn advance(&mut self) -> f32 {
        if !self.current.is_finite() {
            self.current = DEFAULT_NORMALIZED_VALUE;
        }
        if !self.target.is_finite() {
            self.target = self.current;
        }

        let delta = (self.target - self.current).clamp(-self.max_step, self.max_step);
        self.current =
            (self.current + delta).clamp(NORMALIZED_PARAMETER_MIN, NORMALIZED_PARAMETER_MAX);
        self.current
    }

    pub(crate) fn reset_to_target(&mut self) {
        self.current = self.target;
    }

    #[cfg(test)]
    pub(crate) fn current(&self) -> f32 {
        self.current
    }

    #[cfg(test)]
    pub(crate) fn target(&self) -> f32 {
        self.target
    }
}

impl Default for SmoothedNormalizedValue {
    fn default() -> Self {
        Self::new(DEFAULT_NORMALIZED_VALUE, DEFAULT_SMOOTHING_STEP)
    }
}

#[derive(Debug, Clone, Copy)]
struct BiquadCoeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl BiquadCoeffs {
    const fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct BiquadState {
    z1: f32,
    z2: f32,
}

impl BiquadState {
    fn process(&mut self, coeffs: BiquadCoeffs, x: f32) -> f32 {
        let y = coeffs.b0 * x + self.z1;
        self.z1 = coeffs.b1 * x - coeffs.a1 * y + self.z2;
        self.z2 = coeffs.b2 * x - coeffs.a2 * y;
        y
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct IsolatorChannelState {
    low_lp: [BiquadState; 2],
    high_hp: [BiquadState; 2],
}

#[derive(Debug, Clone)]
struct DjIsolatorNode {
    low_lp: [BiquadCoeffs; 2],
    high_hp: [BiquadCoeffs; 2],
    states: [IsolatorChannelState; DSP_MAX_CHANNELS],
    low_gain: f32,
    mid_gain: f32,
    high_gain: f32,
}

impl DjIsolatorNode {
    fn new(sample_rate_hz: f32) -> Self {
        let mut node = Self {
            low_lp: [BiquadCoeffs::identity(); 2],
            high_hp: [BiquadCoeffs::identity(); 2],
            states: [IsolatorChannelState::default(); DSP_MAX_CHANNELS],
            low_gain: 1.0,
            mid_gain: 1.0,
            high_gain: 1.0,
        };
        node.prepare(sample_rate_hz);
        node
    }

    fn prepare(&mut self, sample_rate_hz: f32) {
        let sample_rate_hz = sanitize_sample_rate(sample_rate_hz);
        let low_lp = biquad_low_pass_butterworth(sample_rate_hz, ISOLATOR_LOW_CROSSOVER_HZ);
        let high_hp = biquad_high_pass_butterworth(sample_rate_hz, ISOLATOR_HIGH_CROSSOVER_HZ);

        self.low_lp = [low_lp; 2];
        self.high_hp = [high_hp; 2];
        self.reset();
    }

    fn set_normalized_targets(&mut self, low: f32, mid: f32, high: f32) {
        self.low_gain = normalized_isolator_gain(low);
        self.mid_gain = normalized_isolator_gain(mid);
        self.high_gain = normalized_isolator_gain(high);
    }

    fn process_sample(&mut self, channel: usize, x: f32) -> f32 {
        if channel >= DSP_MAX_CHANNELS || !x.is_finite() {
            return if x.is_finite() { x } else { 0.0 };
        }

        let state = &mut self.states[channel];

        let mut low = x;
        for (coeffs, stage) in self.low_lp.iter().zip(state.low_lp.iter_mut()) {
            low = stage.process(*coeffs, low);
        }

        let mut high = x;
        for (coeffs, stage) in self.high_hp.iter().zip(state.high_hp.iter_mut()) {
            high = stage.process(*coeffs, high);
        }

        let mid = x - low - high;
        let y = low * self.low_gain + mid * self.mid_gain + high * self.high_gain;

        if y.is_finite() { y } else { 0.0 }
    }

    fn reset(&mut self) {
        self.states = [IsolatorChannelState::default(); DSP_MAX_CHANNELS];
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PerPadDspChain {
    pad_id: u16,
    sample_rate_hz: f32,
    max_block_frames: usize,
    channels: usize,
    parameters: [SmoothedNormalizedValue; DSP_PARAMETER_SLOTS],
    isolator_node: DjIsolatorNode,
}

impl PerPadDspChain {
    pub(crate) fn new(
        pad_id: usize,
        sample_rate_hz: f32,
        max_block_frames: usize,
        channels: usize,
    ) -> Self {
        let mut chain = Self {
            pad_id: u16::try_from(pad_id).unwrap_or(u16::MAX),
            sample_rate_hz: DEFAULT_SAMPLE_RATE_HZ,
            max_block_frames: DEFAULT_MAX_BLOCK_FRAMES,
            channels: DEFAULT_CHANNELS,
            parameters: [SmoothedNormalizedValue::default(); DSP_PARAMETER_SLOTS],
            isolator_node: DjIsolatorNode::new(DEFAULT_SAMPLE_RATE_HZ),
        };
        chain.prepare(sample_rate_hz, max_block_frames, channels);
        chain
    }

    pub(crate) fn prepare(
        &mut self,
        sample_rate_hz: f32,
        max_block_frames: usize,
        channels: usize,
    ) {
        self.sample_rate_hz = sanitize_sample_rate(sample_rate_hz);
        self.max_block_frames = max_block_frames.max(DEFAULT_MAX_BLOCK_FRAMES);
        self.channels = channels.clamp(DEFAULT_CHANNELS, DSP_MAX_CHANNELS);
        self.isolator_node.prepare(self.sample_rate_hz);
        self.reset();
    }

    pub(crate) fn set_parameter(&mut self, id: DspParameterId, normalized_target: f32) -> bool {
        if !id.matches_pad_chain(self.pad_id) {
            return false;
        }

        self.parameters[id.parameter_slot.index()].set_target(normalized_target)
    }

    pub(crate) fn begin_frame(&mut self) {
        let low = self.parameters[DspParameterSlot::Slot0.index()].advance();
        let mid = self.parameters[DspParameterSlot::Slot1.index()].advance();
        let high = self.parameters[DspParameterSlot::Slot2.index()].advance();
        self.isolator_node.set_normalized_targets(low, mid, high);
    }

    pub(crate) fn process_sample(&mut self, channel: usize, sample: f32) -> f32 {
        if channel >= self.channels {
            return sample;
        }

        self.isolator_node.process_sample(channel, sample)
    }

    pub(crate) fn process_interleaved_block(
        &mut self,
        buffer: &mut [f32],
        channels: usize,
    ) -> bool {
        if channels == 0 || channels != self.channels {
            return false;
        }
        let frames = buffer.len() / channels;
        if frames * channels != buffer.len() || frames > self.max_block_frames {
            return false;
        }

        for frame in 0..frames {
            self.begin_frame();
            let frame_start = frame * channels;
            for (channel, sample) in buffer[frame_start..frame_start + channels]
                .iter_mut()
                .enumerate()
            {
                *sample = self.process_sample(channel, *sample);
            }
        }

        true
    }

    pub(crate) fn reset(&mut self) {
        for parameter in &mut self.parameters {
            parameter.reset_to_target();
        }
        self.begin_frame();
        self.isolator_node.reset();
    }

    #[cfg(test)]
    pub(crate) fn parameter(&self, slot: DspParameterSlot) -> SmoothedNormalizedValue {
        self.parameters[slot.index()]
    }

    #[cfg(test)]
    pub(crate) fn prepared_state(&self) -> (f32, usize, usize) {
        (self.sample_rate_hz, self.max_block_frames, self.channels)
    }
}

fn normalized_isolator_gain(normalized: f32) -> f32 {
    let normalized = sanitize_normalized(normalized, DEFAULT_NORMALIZED_VALUE);
    if normalized <= NORMALIZED_PARAMETER_MIN {
        return 0.0;
    }
    if normalized <= DEFAULT_NORMALIZED_VALUE {
        return normalized / DEFAULT_NORMALIZED_VALUE;
    }

    let boost = (normalized - DEFAULT_NORMALIZED_VALUE) / DEFAULT_NORMALIZED_VALUE;
    let boost_db = boost * ISOLATOR_BOOST_DB_MAX;
    10.0_f32.powf(boost_db / 20.0)
}

fn biquad_low_pass_butterworth(fs_hz: f32, freq_hz: f32) -> BiquadCoeffs {
    let freq_hz = clamp_freq_hz(fs_hz, freq_hz);
    let w0 = 2.0 * PI * freq_hz / fs_hz;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / (2.0 * BUTTERWORTH_Q);

    let b0 = (1.0 - cos_w0) * 0.5;
    let b1 = 1.0 - cos_w0;
    let b2 = (1.0 - cos_w0) * 0.5;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha;

    normalize_biquad(b0, b1, b2, a0, a1, a2)
}

fn biquad_high_pass_butterworth(fs_hz: f32, freq_hz: f32) -> BiquadCoeffs {
    let freq_hz = clamp_freq_hz(fs_hz, freq_hz);
    let w0 = 2.0 * PI * freq_hz / fs_hz;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / (2.0 * BUTTERWORTH_Q);

    let b0 = (1.0 + cos_w0) * 0.5;
    let b1 = -(1.0 + cos_w0);
    let b2 = (1.0 + cos_w0) * 0.5;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha;

    normalize_biquad(b0, b1, b2, a0, a1, a2)
}

fn clamp_freq_hz(fs_hz: f32, freq_hz: f32) -> f32 {
    if !fs_hz.is_finite() || fs_hz <= 0.0 {
        return freq_hz.max(1.0);
    }

    let nyquist = fs_hz * 0.5;
    let max_hz = (nyquist * 0.9).max(1.0);
    freq_hz.clamp(1.0, max_hz)
}

fn normalize_biquad(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> BiquadCoeffs {
    if !a0.is_finite() || a0.abs() < 1e-12 {
        return BiquadCoeffs::identity();
    }

    let inv_a0 = 1.0 / a0;
    let coeffs = BiquadCoeffs {
        b0: b0 * inv_a0,
        b1: b1 * inv_a0,
        b2: b2 * inv_a0,
        a1: a1 * inv_a0,
        a2: a2 * inv_a0,
    };

    if [coeffs.b0, coeffs.b1, coeffs.b2, coeffs.a1, coeffs.a2]
        .iter()
        .all(|v| v.is_finite())
    {
        coeffs
    } else {
        BiquadCoeffs::identity()
    }
}

fn sanitize_normalized(value: f32, default: f32) -> f32 {
    if value.is_finite() {
        value.clamp(NORMALIZED_PARAMETER_MIN, NORMALIZED_PARAMETER_MAX)
    } else {
        default.clamp(NORMALIZED_PARAMETER_MIN, NORMALIZED_PARAMETER_MAX)
    }
}

fn sanitize_smoothing_step(step: f32) -> f32 {
    if step.is_finite() {
        step.clamp(0.0, NORMALIZED_PARAMETER_MAX)
    } else {
        DEFAULT_SMOOTHING_STEP
    }
}

fn sanitize_sample_rate(sample_rate_hz: f32) -> f32 {
    if sample_rate_hz.is_finite() && sample_rate_hz > 0.0 {
        sample_rate_hz
    } else {
        DEFAULT_SAMPLE_RATE_HZ
    }
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use super::*;

    fn set_and_snap_parameter(chain: &mut PerPadDspChain, slot: DspParameterSlot, normalized: f32) {
        let id = DspParameterId::per_pad(0, DspNodeSlot::Slot0, slot).unwrap();
        assert!(chain.set_parameter(id, normalized));
        chain.reset();
    }

    fn sine_rms_after_processing(frequency_hz: f32, chain: &mut PerPadDspChain) -> f32 {
        let sample_rate_hz = 48_000.0;
        let frames = 32768;
        let mut sum = 0.0_f32;
        let mut count = 0_usize;

        for frame in 0..frames {
            let x = (frame as f32 * frequency_hz * std::f32::consts::TAU / sample_rate_hz).sin();
            chain.begin_frame();
            let y = chain.process_sample(0, x);
            if frame >= 8192 {
                sum += y * y;
                count += 1;
            }
        }

        (sum / count as f32).sqrt()
    }

    fn sine_rms_ratio_with_targets(frequency_hz: f32, low: f32, mid: f32, high: f32) -> f32 {
        let mut neutral = PerPadDspChain::new(0, 48_000.0, 8192, 1);
        let neutral_rms = sine_rms_after_processing(frequency_hz, &mut neutral);

        let mut processed = PerPadDspChain::new(0, 48_000.0, 8192, 1);
        set_and_snap_parameter(&mut processed, DspParameterSlot::Slot0, low);
        set_and_snap_parameter(&mut processed, DspParameterSlot::Slot1, mid);
        set_and_snap_parameter(&mut processed, DspParameterSlot::Slot2, high);
        let processed_rms = sine_rms_after_processing(frequency_hz, &mut processed);

        processed_rms / neutral_rms
    }

    #[test]
    fn dsp_parameter_id_is_fixed_size_and_rejects_oversized_pad_index() {
        let id = DspParameterId::per_pad(3, DspNodeSlot::Slot0, DspParameterSlot::Slot2).unwrap();

        assert!(size_of::<DspParameterId>() <= 8);
        assert!(id.matches_pad_chain(3));
        assert!(!id.matches_pad_chain(4));
        assert!(
            DspParameterId::per_pad(
                usize::from(u16::MAX) + 1,
                DspNodeSlot::Slot0,
                DspParameterSlot::Slot0
            )
            .is_none()
        );
    }

    #[test]
    fn smoothed_normalized_value_clamps_finite_targets_and_rejects_nonfinite() {
        let mut value = SmoothedNormalizedValue::new(0.25, 0.10);

        assert!(value.set_target(2.0));
        assert_eq!(value.target(), 1.0);
        assert!(!value.set_target(f32::NAN));
        assert_eq!(value.target(), 1.0);

        assert!((value.advance() - 0.35).abs() < 1e-6);
        assert!((value.advance() - 0.45).abs() < 1e-6);
    }

    #[test]
    fn smoothed_normalized_value_progresses_toward_target_with_bounded_steps() {
        let mut value = SmoothedNormalizedValue::new(0.0, 0.10);
        assert!(value.set_target(0.25));

        assert!((value.advance() - 0.10).abs() < 1e-6);
        assert!((value.advance() - 0.20).abs() < 1e-6);
        assert!((value.advance() - 0.25).abs() < 1e-6);

        assert!(value.set_target(0.0));
        assert!((value.advance() - 0.15).abs() < 1e-6);
    }

    #[test]
    fn isolator_chain_is_transparent_at_neutral() {
        let mut chain = PerPadDspChain::new(0, 48_000.0, 8, 2);
        let mut buffer = vec![0.0, 0.5, -0.25, 1.0, -1.0, 0.25, 0.40, -0.35];
        let expected = buffer.clone();

        assert!(chain.process_interleaved_block(&mut buffer, 2));

        for (actual, expected) in buffer.iter().zip(expected.iter()) {
            assert!((*actual - *expected).abs() < 1e-5);
        }
    }

    #[test]
    fn per_pad_chain_prepares_resets_and_rejects_wrong_parameter_identity() {
        let mut chain = PerPadDspChain::new(2, f32::NAN, 0, 0);
        assert_eq!(chain.prepared_state(), (DEFAULT_SAMPLE_RATE_HZ, 1, 1));

        chain.prepare(96_000.0, 64, 2);
        assert_eq!(chain.prepared_state(), (96_000.0, 64, 2));

        let wrong_pad =
            DspParameterId::per_pad(3, DspNodeSlot::Slot0, DspParameterSlot::Slot1).unwrap();
        assert!(!chain.set_parameter(wrong_pad, 0.8));

        let id = DspParameterId::per_pad(2, DspNodeSlot::Slot0, DspParameterSlot::Slot1).unwrap();
        assert!(chain.set_parameter(id, 0.8));
        chain.begin_frame();
        assert!(chain.parameter(DspParameterSlot::Slot1).current() > DEFAULT_NORMALIZED_VALUE);

        chain.reset();
        assert_eq!(chain.parameter(DspParameterSlot::Slot1).current(), 0.8);
    }

    #[test]
    fn chain_rejects_mismatched_or_oversized_blocks_without_mutating_audio() {
        let mut chain = PerPadDspChain::new(0, 48_000.0, 2, 2);
        let mut buffer = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6];
        let expected = buffer.clone();

        assert!(!chain.process_interleaved_block(&mut buffer, 1));
        assert_eq!(buffer, expected);
        assert!(!chain.process_interleaved_block(&mut buffer, 2));
        assert_eq!(buffer, expected);
    }

    #[test]
    fn low_band_full_kill_reduces_low_content_but_preserves_high_content() {
        let mut neutral_low = PerPadDspChain::new(0, 48_000.0, 8192, 1);
        let mut killed_low = PerPadDspChain::new(0, 48_000.0, 8192, 1);
        set_and_snap_parameter(&mut killed_low, DspParameterSlot::Slot0, 0.0);

        let neutral_low_rms = sine_rms_after_processing(20.0, &mut neutral_low);
        let killed_low_rms = sine_rms_after_processing(20.0, &mut killed_low);

        let mut neutral_high = PerPadDspChain::new(0, 48_000.0, 8192, 1);
        let mut killed_low_for_high = PerPadDspChain::new(0, 48_000.0, 8192, 1);
        set_and_snap_parameter(&mut killed_low_for_high, DspParameterSlot::Slot0, 0.0);

        let neutral_high_rms = sine_rms_after_processing(8_000.0, &mut neutral_high);
        let killed_low_high_rms = sine_rms_after_processing(8_000.0, &mut killed_low_for_high);

        assert!(killed_low_rms < neutral_low_rms * 0.25);
        assert!(killed_low_high_rms > neutral_high_rms * 0.75);
    }

    #[test]
    fn mid_band_boost_is_bounded_to_six_db() {
        let mut neutral = PerPadDspChain::new(0, 48_000.0, 8192, 1);
        let mut boosted = PerPadDspChain::new(0, 48_000.0, 8192, 1);
        set_and_snap_parameter(&mut boosted, DspParameterSlot::Slot1, 1.0);

        let neutral_rms = sine_rms_after_processing(1_000.0, &mut neutral);
        let boosted_rms = sine_rms_after_processing(1_000.0, &mut boosted);

        assert!(boosted_rms > neutral_rms);
        assert!(boosted_rms < neutral_rms * 2.05);
    }

    #[test]
    fn representative_tone_audition_records_current_isolator_tuning_gap() {
        let all_band_boost = sine_rms_ratio_with_targets(1_000.0, 1.0, 1.0, 1.0);
        assert!((all_band_boost - 10.0_f32.powf(ISOLATOR_BOOST_DB_MAX / 20.0)).abs() < 0.02);

        let mid_kill = sine_rms_ratio_with_targets(1_000.0, 0.5, 0.0, 0.5);
        assert!(mid_kill < 0.05);

        let low_kill = sine_rms_ratio_with_targets(60.0, 0.0, 0.5, 0.5);
        let high_kill = sine_rms_ratio_with_targets(8_000.0, 0.5, 0.5, 0.0);

        // This review-slice characterization should be replaced by suppression thresholds in
        // the focused low/high kill tuning follow-up.
        assert!(
            low_kill > 0.50,
            "review should keep the low-kill tuning follow-up active until this drops"
        );
        assert!(
            high_kill > 1.00,
            "review should keep the high-kill tuning follow-up active until this drops"
        );
    }

    #[test]
    fn rapid_target_changes_are_smoothed_and_output_stays_finite() {
        let mut chain = PerPadDspChain::new(0, 48_000.0, 512, 1);
        let low_id =
            DspParameterId::per_pad(0, DspNodeSlot::Slot0, DspParameterSlot::Slot0).unwrap();

        assert!(chain.set_parameter(low_id, 0.0));
        chain.begin_frame();
        assert!((chain.parameter(DspParameterSlot::Slot0).current() - 0.49).abs() < 1e-6);

        for frame in 0..512 {
            let target = if frame % 2 == 0 { 1.0 } else { 0.0 };
            assert!(chain.set_parameter(low_id, target));
            chain.begin_frame();
            let x = if frame == 0 { 1.0 } else { 0.0 };
            let y = chain.process_sample(0, x);
            assert!(y.is_finite());
        }
    }
}
