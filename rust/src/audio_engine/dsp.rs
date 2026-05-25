//! Internal realtime-safe DSP/FX foundation helpers.
//!
//! The first foundation slice is intentionally neutral: it provides typed bounded parameter
//! identity, Rust-owned smoothing state, and a per-pad chain host without adding a visible effect.

#![allow(dead_code)]

const DEFAULT_SAMPLE_RATE_HZ: f32 = 44_100.0;
const DEFAULT_MAX_BLOCK_FRAMES: usize = 1;
const DEFAULT_CHANNELS: usize = 1;
const DEFAULT_NORMALIZED_VALUE: f32 = 0.5;
const DEFAULT_SMOOTHING_STEP: f32 = 0.01;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NeutralDspNode;

impl NeutralDspNode {
    fn process_sample(self, sample: f32) -> f32 {
        sample
    }

    fn reset(&mut self) {}
}

#[derive(Debug, Clone)]
pub(crate) struct PerPadDspChain {
    pad_id: u16,
    sample_rate_hz: f32,
    max_block_frames: usize,
    channels: usize,
    parameters: [SmoothedNormalizedValue; DSP_PARAMETER_SLOTS],
    neutral_node: NeutralDspNode,
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
            neutral_node: NeutralDspNode,
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
        self.channels = channels.max(DEFAULT_CHANNELS);
        self.reset();
    }

    pub(crate) fn set_parameter(&mut self, id: DspParameterId, normalized_target: f32) -> bool {
        if !id.matches_pad_chain(self.pad_id) {
            return false;
        }

        self.parameters[id.parameter_slot.index()].set_target(normalized_target)
    }

    pub(crate) fn begin_frame(&mut self) {
        for parameter in &mut self.parameters {
            parameter.advance();
        }
    }

    pub(crate) fn process_sample(&mut self, sample: f32) -> f32 {
        self.neutral_node.process_sample(sample)
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
            for sample in &mut buffer[frame_start..frame_start + channels] {
                *sample = self.process_sample(*sample);
            }
        }

        true
    }

    pub(crate) fn reset(&mut self) {
        for parameter in &mut self.parameters {
            parameter.reset_to_target();
        }
        self.neutral_node.reset();
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
    fn neutral_chain_passes_interleaved_block_through() {
        let mut chain = PerPadDspChain::new(0, 48_000.0, 8, 2);
        let mut buffer = vec![0.0, 0.5, -0.25, 1.0, -1.0, 0.25];
        let expected = buffer.clone();

        assert!(chain.process_interleaved_block(&mut buffer, 2));

        assert_eq!(buffer, expected);
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
    fn neutral_chain_rejects_mismatched_or_oversized_blocks_without_mutating_audio() {
        let mut chain = PerPadDspChain::new(0, 48_000.0, 2, 2);
        let mut buffer = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6];
        let expected = buffer.clone();

        assert!(!chain.process_interleaved_block(&mut buffer, 1));
        assert_eq!(buffer, expected);
        assert!(!chain.process_interleaved_block(&mut buffer, 2));
        assert_eq!(buffer, expected);
    }
}
