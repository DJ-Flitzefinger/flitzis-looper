//! Audio-thread-owned transport timeline.
//!
//! The transport keeps sample-frame time in Rust so later Gen3 scheduling can
//! target absolute output frames without relying on Python callback timing.

#![allow(dead_code)]

const DEFAULT_SAMPLE_RATE_HZ: u32 = 44_100;
const BEATS_PER_BAR_4_4: u32 = 4;
const PHASE_EPSILON: f64 = 1.0e-9;
const GRID_EPSILON_FRAMES: f64 = 1.0e-6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuantizeGrid {
    Beat,
    Bar,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TransportTimeline {
    output_frame: u64,
    sample_rate_hz: u32,
    master_bpm: Option<f32>,
    beats_per_bar: u32,
    downbeat_frame: u64,
}

impl TransportTimeline {
    pub(crate) fn new(sample_rate_hz: u32) -> Self {
        let sample_rate_hz = if sample_rate_hz == 0 {
            DEFAULT_SAMPLE_RATE_HZ
        } else {
            sample_rate_hz
        };

        Self {
            output_frame: 0,
            sample_rate_hz,
            master_bpm: None,
            beats_per_bar: BEATS_PER_BAR_4_4,
            downbeat_frame: 0,
        }
    }

    pub(crate) fn output_frame(&self) -> u64 {
        self.output_frame
    }

    pub(crate) fn sample_rate_hz(&self) -> u32 {
        self.sample_rate_hz
    }

    pub(crate) fn master_bpm(&self) -> Option<f32> {
        self.master_bpm
    }

    pub(crate) fn beats_per_bar(&self) -> u32 {
        self.beats_per_bar
    }

    pub(crate) fn downbeat_frame(&self) -> u64 {
        self.downbeat_frame
    }

    pub(crate) fn set_downbeat_frame(&mut self, frame: u64) {
        self.downbeat_frame = frame;
    }

    pub(crate) fn anchor_downbeat_to_bar_phase(&mut self, bar_phase_beats: f64) -> bool {
        let Some(frames_per_beat) = self.frames_per_beat() else {
            return false;
        };
        let Some(frames_per_bar) = self.frames_per_bar() else {
            return false;
        };
        let bar_phase_beats = normalize_phase(bar_phase_beats, self.beats_per_bar as f64);

        if !frames_per_beat.is_finite()
            || frames_per_beat <= 0.0
            || !frames_per_bar.is_finite()
            || frames_per_bar <= 0.0
        {
            return false;
        }

        let mut downbeat_frame = self.output_frame as f64 - bar_phase_beats * frames_per_beat;

        if downbeat_frame < 0.0 {
            let bars_to_add = (-downbeat_frame / frames_per_bar).ceil();
            downbeat_frame += bars_to_add * frames_per_bar;
        }

        if !downbeat_frame.is_finite() || downbeat_frame < 0.0 || downbeat_frame >= u64::MAX as f64
        {
            return false;
        }

        self.downbeat_frame = downbeat_frame.round() as u64;
        true
    }

    pub(crate) fn set_master_bpm(&mut self, bpm: f32) -> bool {
        if !is_valid_bpm(bpm) {
            return false;
        }

        self.master_bpm = Some(bpm);
        true
    }

    pub(crate) fn clear_master_bpm(&mut self) {
        self.master_bpm = None;
    }

    pub(crate) fn advance_by_rendered_frames(&mut self, frames: usize) {
        self.output_frame = self.output_frame.saturating_add(frames as u64);
    }

    pub(crate) fn frames_per_beat(&self) -> Option<f64> {
        let bpm = self.master_bpm?;
        if !is_valid_bpm(bpm) {
            return None;
        }

        Some(self.sample_rate_hz as f64 * 60.0 / bpm as f64)
    }

    pub(crate) fn frames_per_bar(&self) -> Option<f64> {
        Some(self.frames_per_beat()? * self.beats_per_bar as f64)
    }

    pub(crate) fn beat_position(&self) -> Option<f64> {
        self.beat_position_at_frame(self.output_frame)
    }

    pub(crate) fn beat_position_at_frame(&self, output_frame: u64) -> Option<f64> {
        Some(self.relative_frames_from_downbeat_at_frame(output_frame) / self.frames_per_beat()?)
    }

    pub(crate) fn beat_phase(&self) -> Option<f64> {
        Some(normalize_phase(self.beat_position()?, 1.0))
    }

    pub(crate) fn bar_phase_beats(&self) -> Option<f64> {
        self.bar_phase_beats_at_frame(self.output_frame)
    }

    pub(crate) fn bar_phase_beats_at_frame(&self, output_frame: u64) -> Option<f64> {
        Some(normalize_phase(
            self.beat_position_at_frame(output_frame)?,
            self.beats_per_bar as f64,
        ))
    }

    pub(crate) fn current_beat_index_in_bar(&self) -> Option<u32> {
        let phase = self.bar_phase_beats()?;
        Some((phase.floor() as u32).min(self.beats_per_bar.saturating_sub(1)))
    }

    pub(crate) fn next_beat_frame(&self) -> Option<u64> {
        self.next_grid_frame(QuantizeGrid::Beat)
    }

    pub(crate) fn next_bar_frame(&self) -> Option<u64> {
        self.next_grid_frame(QuantizeGrid::Bar)
    }

    pub(crate) fn next_grid_frame(&self, grid: QuantizeGrid) -> Option<u64> {
        let frames_per_grid = match grid {
            QuantizeGrid::Beat => self.frames_per_beat()?,
            QuantizeGrid::Bar => self.frames_per_bar()?,
        };

        if !frames_per_grid.is_finite() || frames_per_grid <= 0.0 {
            return None;
        }

        let relative_frames = self.relative_frames_from_downbeat();
        let grid_position = relative_frames / frames_per_grid;
        let nearest_grid = grid_position.round();
        let distance_frames = (grid_position - nearest_grid).abs() * frames_per_grid;

        let target_grid = if distance_frames <= GRID_EPSILON_FRAMES {
            nearest_grid
        } else {
            grid_position.floor() + 1.0
        };

        let target_frame = self.downbeat_frame as f64 + target_grid * frames_per_grid;
        Some(frame_at_or_after(target_frame, self.output_frame))
    }

    fn relative_frames_from_downbeat(&self) -> f64 {
        self.relative_frames_from_downbeat_at_frame(self.output_frame)
    }

    fn relative_frames_from_downbeat_at_frame(&self, output_frame: u64) -> f64 {
        output_frame as f64 - self.downbeat_frame as f64
    }
}

fn is_valid_bpm(bpm: f32) -> bool {
    bpm.is_finite() && bpm > 0.0
}

fn normalize_phase(value: f64, modulo: f64) -> f64 {
    let phase = value.rem_euclid(modulo);
    if phase <= PHASE_EPSILON || (modulo - phase) <= PHASE_EPSILON {
        0.0
    } else {
        phase
    }
}

fn frame_at_or_after(target_frame: f64, current_frame: u64) -> u64 {
    if !target_frame.is_finite() {
        return current_frame;
    }

    if target_frame <= current_frame as f64 + GRID_EPSILON_FRAMES {
        return current_frame;
    }

    if target_frame >= u64::MAX as f64 {
        return u64::MAX;
    }

    (target_frame.ceil() as u64).max(current_frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transport_at(frame: u64) -> TransportTimeline {
        let mut transport = TransportTimeline::new(48_000);
        assert!(transport.set_master_bpm(120.0));
        transport.output_frame = frame;
        transport
    }

    #[test]
    fn timeline_starts_with_sample_rate_and_zero_clock() {
        let transport = TransportTimeline::new(48_000);

        assert_eq!(transport.output_frame(), 0);
        assert_eq!(transport.sample_rate_hz(), 48_000);
        assert_eq!(transport.master_bpm(), None);
        assert_eq!(transport.beats_per_bar(), 4);
        assert_eq!(transport.downbeat_frame(), 0);
    }

    #[test]
    fn zero_sample_rate_uses_default_fallback() {
        let transport = TransportTimeline::new(0);

        assert_eq!(transport.sample_rate_hz(), DEFAULT_SAMPLE_RATE_HZ);
    }

    #[test]
    fn timeline_advances_by_rendered_output_frames() {
        let mut transport = TransportTimeline::new(48_000);

        transport.advance_by_rendered_frames(512);
        transport.advance_by_rendered_frames(128);

        assert_eq!(transport.output_frame(), 640);
    }

    #[test]
    fn timeline_saturates_instead_of_wrapping_at_u64_max() {
        let mut transport = TransportTimeline::new(48_000);
        transport.output_frame = u64::MAX - 1;

        transport.advance_by_rendered_frames(8);

        assert_eq!(transport.output_frame(), u64::MAX);
    }

    #[test]
    fn valid_master_bpm_is_stored() {
        let mut transport = TransportTimeline::new(48_000);

        assert!(transport.set_master_bpm(124.5));

        assert_eq!(transport.master_bpm(), Some(124.5));
    }

    #[test]
    fn invalid_master_bpm_is_ignored_without_corrupting_previous_value() {
        let mut transport = TransportTimeline::new(48_000);
        assert!(transport.set_master_bpm(120.0));

        for bpm in [f32::NAN, f32::INFINITY, 0.0, -1.0] {
            assert!(!transport.set_master_bpm(bpm));
            assert_eq!(transport.master_bpm(), Some(120.0));
        }
    }

    #[test]
    fn clearing_master_bpm_disables_musical_timing() {
        let mut transport = transport_at(24_000);

        transport.clear_master_bpm();

        assert_eq!(transport.master_bpm(), None);
        assert_eq!(transport.frames_per_beat(), None);
        assert_eq!(transport.next_beat_frame(), None);
    }

    #[test]
    fn bpm_converts_to_beat_and_bar_frame_lengths() {
        let transport = transport_at(0);

        assert_eq!(transport.frames_per_beat(), Some(24_000.0));
        assert_eq!(transport.frames_per_bar(), Some(96_000.0));
    }

    #[test]
    fn beat_and_bar_phase_are_derived_from_output_frame() {
        let transport = transport_at(24_000);

        assert_eq!(transport.beat_position(), Some(1.0));
        assert_eq!(transport.beat_phase(), Some(0.0));
        assert_eq!(transport.bar_phase_beats(), Some(1.0));
        assert_eq!(transport.current_beat_index_in_bar(), Some(1));
    }

    #[test]
    fn bar_phase_can_be_derived_for_arbitrary_target_frame() {
        let transport = transport_at(123);

        assert_eq!(transport.bar_phase_beats_at_frame(48_000), Some(2.0));
        assert_eq!(transport.output_frame(), 123);
    }

    #[test]
    fn target_frame_phase_respects_downbeat_anchor() {
        let mut transport = transport_at(0);
        transport.set_downbeat_frame(1_000);

        assert_eq!(transport.bar_phase_beats_at_frame(49_000), Some(2.0));
    }

    #[test]
    fn target_frame_phase_wraps_before_downbeat_anchor() {
        let mut transport = transport_at(0);
        transport.set_downbeat_frame(96_000);

        assert_eq!(transport.bar_phase_beats_at_frame(72_000), Some(3.0));
    }

    #[test]
    fn downbeat_anchor_can_be_set_from_current_bar_phase() {
        let mut transport = transport_at(60_000);

        assert!(transport.anchor_downbeat_to_bar_phase(2.5));

        assert_eq!(transport.downbeat_frame(), 0);
        assert_eq!(transport.bar_phase_beats(), Some(2.5));
    }

    #[test]
    fn downbeat_anchor_wraps_forward_when_equivalent_anchor_is_before_zero() {
        let mut transport = transport_at(12_000);

        assert!(transport.anchor_downbeat_to_bar_phase(1.0));

        assert_eq!(transport.downbeat_frame(), 84_000);
        assert_eq!(transport.bar_phase_beats(), Some(1.0));
    }

    #[test]
    fn downbeat_anchor_requires_master_bpm() {
        let mut transport = TransportTimeline::new(48_000);

        assert!(!transport.anchor_downbeat_to_bar_phase(1.0));
        assert_eq!(transport.downbeat_frame(), 0);
    }

    #[test]
    fn fractional_beat_phase_is_reported() {
        let transport = transport_at(36_000);

        assert_eq!(transport.beat_position(), Some(1.5));
        assert_eq!(transport.beat_phase(), Some(0.5));
        assert_eq!(transport.bar_phase_beats(), Some(1.5));
        assert_eq!(transport.current_beat_index_in_bar(), Some(1));
    }

    #[test]
    fn downbeat_anchor_offsets_phase() {
        let mut transport = transport_at(25_000);
        transport.set_downbeat_frame(1_000);

        assert_eq!(transport.beat_position(), Some(1.0));
        assert_eq!(transport.bar_phase_beats(), Some(1.0));
        assert_eq!(transport.next_beat_frame(), Some(25_000));
    }

    #[test]
    fn phase_wraps_before_downbeat_anchor() {
        let mut transport = transport_at(1_000);
        transport.set_downbeat_frame(25_000);

        assert_eq!(transport.beat_position(), Some(-1.0));
        assert_eq!(transport.beat_phase(), Some(0.0));
        assert_eq!(transport.bar_phase_beats(), Some(3.0));
        assert_eq!(transport.current_beat_index_in_bar(), Some(3));
    }

    #[test]
    fn next_beat_frame_uses_current_frame_on_grid_boundary() {
        let transport = transport_at(24_000);

        assert_eq!(transport.next_beat_frame(), Some(24_000));
    }

    #[test]
    fn next_beat_frame_targets_next_boundary_between_beats() {
        let transport = transport_at(24_001);

        assert_eq!(transport.next_beat_frame(), Some(48_000));
    }

    #[test]
    fn next_bar_frame_uses_current_frame_on_bar_boundary() {
        let transport = transport_at(96_000);

        assert_eq!(transport.next_bar_frame(), Some(96_000));
    }

    #[test]
    fn next_bar_frame_targets_next_boundary_between_bars() {
        let transport = transport_at(24_000);

        assert_eq!(transport.next_bar_frame(), Some(96_000));
    }

    #[test]
    fn next_grid_frame_respects_downbeat_anchor() {
        let mut transport = transport_at(1_001);
        transport.set_downbeat_frame(1_000);

        assert_eq!(transport.next_grid_frame(QuantizeGrid::Beat), Some(25_000));
        assert_eq!(transport.next_grid_frame(QuantizeGrid::Bar), Some(97_000));
    }

    #[test]
    fn missing_master_bpm_disables_quantized_grid_targets() {
        let transport = TransportTimeline::new(48_000);

        assert_eq!(transport.beat_position(), None);
        assert_eq!(transport.bar_phase_beats_at_frame(48_000), None);
        assert_eq!(transport.bar_phase_beats(), None);
        assert_eq!(transport.next_beat_frame(), None);
        assert_eq!(transport.next_bar_frame(), None);
    }
}
