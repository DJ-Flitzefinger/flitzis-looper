//! Audio-thread-owned transport timeline.
//!
//! The transport keeps sample-frame time in Rust so later Gen3 scheduling can
//! target absolute output frames without relying on Python callback timing.

const DEFAULT_SAMPLE_RATE_HZ: u32 = 44_100;
const DEFAULT_MASTER_PERIOD_SECONDS: f64 = 0.5;
const BEATS_PER_BAR_4_4: u32 = 4;
pub(crate) const GRID_64THS_PER_BEAT: u16 = 16;
pub(crate) const GRID_64THS_PER_BAR: u16 = GRID_64THS_PER_BEAT * BEATS_PER_BAR_4_4 as u16;
const PHASE_EPSILON: f64 = 1.0e-9;
const GRID_EPSILON_FRAMES: f64 = 1.0e-6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QuantizeGrid {
    step_64ths: u16,
}

impl QuantizeGrid {
    pub(crate) fn from_step_64ths(step_64ths: u16) -> Option<Self> {
        if step_64ths == 0 || step_64ths > GRID_64THS_PER_BAR {
            return None;
        }
        Some(Self { step_64ths })
    }

    pub(crate) fn step_64ths(&self) -> u16 {
        self.step_64ths
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TransportTimeline {
    output_frame: u64,
    sample_rate_hz: u32,
    master_period_seconds: Option<f64>,
    beats_per_bar: u32,
    downbeat_frame: u64,
    beat_anchor_frame: u64,
    beat_anchor_position: f64,
    bootstrap_reference: Option<usize>,
    bootstrap_complete: bool,
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
            master_period_seconds: Some(DEFAULT_MASTER_PERIOD_SECONDS),
            beats_per_bar: BEATS_PER_BAR_4_4,
            downbeat_frame: 0,
            beat_anchor_frame: 0,
            beat_anchor_position: 0.0,
            bootstrap_reference: None,
            bootstrap_complete: false,
        }
    }

    pub(crate) fn output_frame(&self) -> u64 {
        self.output_frame
    }

    // --- Reserved getters/setters for future transport integration (UI display, scheduling) ---
    #[allow(dead_code)]
    pub(crate) fn sample_rate_hz(&self) -> u32 {
        self.sample_rate_hz
    }

    #[allow(dead_code)]
    pub(crate) fn master_bpm(&self) -> Option<f64> {
        Some(60.0 / self.master_period_seconds?)
    }

    /// Authoritative output seconds per quarter; BPM is presentation only.
    pub(crate) fn master_period_seconds(&self) -> Option<f64> {
        self.master_period_seconds
    }

    #[allow(dead_code)]
    pub(crate) fn beats_per_bar(&self) -> u32 {
        self.beats_per_bar
    }

    #[allow(dead_code)]
    pub(crate) fn downbeat_frame(&self) -> u64 {
        self.downbeat_frame
    }

    #[allow(dead_code)]
    pub(crate) fn set_downbeat_frame(&mut self, frame: u64) {
        self.downbeat_frame = frame;
        self.beat_anchor_frame = frame;
        self.beat_anchor_position = 0.0;
    }

    #[allow(dead_code)]
    pub(crate) fn anchor_downbeat_to_bar_phase(&mut self, bar_phase_beats: f64) -> bool {
        self.anchor_downbeat_to_bar_phase_at_frame(bar_phase_beats, self.output_frame)
    }

    pub(crate) fn anchor_downbeat_to_bar_phase_at_frame(
        &mut self,
        bar_phase_beats: f64,
        output_frame: u64,
    ) -> bool {
        if !bar_phase_beats.is_finite() {
            return false;
        }
        self.anchor_beat_position_at_frame(
            normalize_phase(bar_phase_beats, self.beats_per_bar as f64),
            output_frame,
        )
    }

    pub(crate) fn anchor_beat_position_at_frame(
        &mut self,
        beat_position: f64,
        output_frame: u64,
    ) -> bool {
        if !beat_position.is_finite() {
            return false;
        }
        let Some(frames_per_beat) = self.frames_per_beat() else {
            return false;
        };
        let Some(frames_per_bar) = self.frames_per_bar() else {
            return false;
        };
        let bar_phase_beats = normalize_phase(beat_position, self.beats_per_bar as f64);

        if !frames_per_beat.is_finite()
            || frames_per_beat <= 0.0
            || !frames_per_bar.is_finite()
            || frames_per_bar <= 0.0
        {
            return false;
        }

        let mut downbeat_frame = output_frame as f64 - bar_phase_beats * frames_per_beat;

        if downbeat_frame < 0.0 {
            let bars_to_add = (-downbeat_frame / frames_per_bar).ceil();
            downbeat_frame += bars_to_add * frames_per_bar;
        }

        if !downbeat_frame.is_finite() || downbeat_frame < 0.0 || downbeat_frame >= u64::MAX as f64
        {
            return false;
        }

        self.downbeat_frame = downbeat_frame.round() as u64;
        self.beat_anchor_frame = output_frame;
        self.beat_anchor_position = beat_position;
        true
    }

    #[cfg(test)]
    pub(crate) fn set_master_bpm_and_anchor_beat_position_at_frame(
        &mut self,
        bpm: f64,
        beat_position: f64,
        output_frame: u64,
    ) -> bool {
        let Some(period_seconds) = period_from_bpm(bpm) else {
            return false;
        };
        self.set_master_period_and_anchor_beat_position_at_frame(
            period_seconds,
            beat_position,
            output_frame,
        )
    }

    pub(crate) fn set_master_period_and_anchor_beat_position_at_frame(
        &mut self,
        period_seconds: f64,
        beat_position: f64,
        output_frame: u64,
    ) -> bool {
        if !self.is_valid_period(period_seconds) {
            return false;
        }
        let previous = *self;
        self.master_period_seconds = Some(period_seconds);

        if self.anchor_beat_position_at_frame(beat_position, output_frame) {
            return true;
        }

        *self = previous;
        false
    }

    #[cfg(test)]
    pub(crate) fn set_master_bpm_preserving_beat_position_at_frame(
        &mut self,
        bpm: f64,
        output_frame: u64,
    ) -> bool {
        let Some(period_seconds) = period_from_bpm(bpm) else {
            return false;
        };
        self.set_master_period_preserving_beat_position_at_frame(period_seconds, output_frame)
    }

    pub(crate) fn set_master_period_preserving_beat_position_at_frame(
        &mut self,
        period_seconds: f64,
        output_frame: u64,
    ) -> bool {
        if !self.is_valid_period(period_seconds) {
            return false;
        }

        let Some(beat_position) = self.beat_position_at_frame(output_frame) else {
            return self.set_master_period(period_seconds);
        };
        self.set_master_period_and_anchor_beat_position_at_frame(
            period_seconds,
            beat_position,
            output_frame,
        )
    }

    pub(crate) fn request_bootstrap(&mut self, id: usize) {
        if !self.bootstrap_complete && self.bootstrap_reference.is_none() {
            self.bootstrap_reference = Some(id);
        }
    }

    pub(crate) fn bootstrap_reference(&self) -> Option<usize> {
        self.bootstrap_reference
            .filter(|_| !self.bootstrap_complete)
    }

    pub(crate) fn clear_pending_bootstrap_for_pad(&mut self, id: usize) {
        if self.bootstrap_reference == Some(id) {
            self.bootstrap_reference = None;
        }
    }

    pub(crate) fn complete_bootstrap(&mut self) {
        self.bootstrap_complete = true;
        self.bootstrap_reference = None;
    }

    #[cfg(test)]
    pub(crate) fn bootstrap_from_source_at_frame(
        &mut self,
        bpm: f64,
        source_beat: f64,
        frame: u64,
    ) -> bool {
        let Some(period_seconds) = period_from_bpm(bpm) else {
            return false;
        };
        self.bootstrap_from_source_period_at_frame(period_seconds, source_beat, frame)
    }

    pub(crate) fn bootstrap_from_source_period_at_frame(
        &mut self,
        period_seconds: f64,
        source_beat: f64,
        frame: u64,
    ) -> bool {
        if self.bootstrap_complete {
            return false;
        }
        if self.set_master_period_and_anchor_beat_position_at_frame(
            period_seconds,
            source_beat,
            frame,
        ) {
            self.complete_bootstrap();
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    pub(crate) fn set_master_bpm(&mut self, bpm: f64) -> bool {
        let Some(period_seconds) = period_from_bpm(bpm) else {
            return false;
        };
        self.set_master_period(period_seconds)
    }

    pub(crate) fn set_master_period(&mut self, period_seconds: f64) -> bool {
        if !self.is_valid_period(period_seconds) {
            return false;
        }
        self.master_period_seconds = Some(period_seconds);
        true
    }

    fn is_valid_period(&self, period_seconds: f64) -> bool {
        let frames_per_beat = f64::from(self.sample_rate_hz) * period_seconds;
        let frames_per_bar = frames_per_beat * f64::from(self.beats_per_bar);
        period_seconds.is_finite()
            && period_seconds > 0.0
            && frames_per_beat.is_finite()
            && frames_per_beat > 0.0
            && frames_per_bar.is_finite()
    }

    #[allow(dead_code)]
    pub(crate) fn clear_master_bpm(&mut self) {
        self.master_period_seconds = None;
    }

    pub(crate) fn advance_by_rendered_frames(&mut self, frames: usize) {
        self.output_frame = self.output_frame.saturating_add(frames as u64);
    }

    pub(crate) fn frames_per_beat(&self) -> Option<f64> {
        Some(f64::from(self.sample_rate_hz) * self.master_period_seconds?)
    }

    pub(crate) fn frames_per_bar(&self) -> Option<f64> {
        Some(self.frames_per_beat()? * self.beats_per_bar as f64)
    }

    // --- Reserved position queries for future UI/scheduling integration ---
    #[allow(dead_code)]
    pub(crate) fn beat_position(&self) -> Option<f64> {
        self.beat_position_at_frame(self.output_frame)
    }

    pub(crate) fn beat_position_at_frame(&self, output_frame: u64) -> Option<f64> {
        Some(
            self.beat_anchor_position
                + (output_frame as f64 - self.beat_anchor_frame as f64) / self.frames_per_beat()?,
        )
    }

    #[allow(dead_code)]
    pub(crate) fn beat_phase(&self) -> Option<f64> {
        Some(normalize_phase(self.beat_position()?, 1.0))
    }

    #[allow(dead_code)]
    pub(crate) fn bar_phase_beats(&self) -> Option<f64> {
        self.bar_phase_beats_at_frame(self.output_frame)
    }

    pub(crate) fn bar_phase_beats_at_frame(&self, output_frame: u64) -> Option<f64> {
        Some(normalize_phase(
            self.beat_position_at_frame(output_frame)?,
            self.beats_per_bar as f64,
        ))
    }

    #[allow(dead_code)]
    pub(crate) fn current_beat_index_in_bar(&self) -> Option<u32> {
        let phase = self.bar_phase_beats()?;
        Some((phase.floor() as u32).min(self.beats_per_bar.saturating_sub(1)))
    }

    pub(crate) fn next_grid_frame(&self, grid: QuantizeGrid) -> Option<u64> {
        let frames_per_grid =
            self.frames_per_beat()? * grid.step_64ths() as f64 / GRID_64THS_PER_BEAT as f64;

        if !frames_per_grid.is_finite() || frames_per_grid <= 0.0 {
            return None;
        }

        let grid_position =
            self.beat_position()? * GRID_64THS_PER_BEAT as f64 / grid.step_64ths() as f64;
        let rounded_grid = grid_position.round();
        let distance_frames = (grid_position - rounded_grid).abs() * frames_per_grid;

        let target_grid = if distance_frames <= GRID_EPSILON_FRAMES {
            rounded_grid
        } else {
            grid_position.floor() + 1.0
        };

        let target_frame =
            self.output_frame as f64 + (target_grid - grid_position) * frames_per_grid;
        Some(frame_at_or_after(target_frame, self.output_frame))
    }
}

#[cfg(test)]
fn period_from_bpm(bpm: f64) -> Option<f64> {
    if !bpm.is_finite() || bpm <= 0.0 {
        return None;
    }
    let period_seconds = 60.0 / bpm;
    (period_seconds.is_finite() && period_seconds > 0.0).then_some(period_seconds)
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

    // Floating beat arithmetic can put an exact integer boundary a tiny
    // fraction of a frame above itself. Preserve that boundary consistently.
    ((target_frame - GRID_EPSILON_FRAMES).ceil() as u64).max(current_frame)
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
        assert_eq!(
            transport.master_period_seconds(),
            Some(DEFAULT_MASTER_PERIOD_SECONDS)
        );
        assert_eq!(transport.master_bpm(), Some(120.0));
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

        for bpm in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
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
        assert_eq!(
            transport.next_grid_frame(QuantizeGrid::from_step_64ths(16).unwrap()),
            None
        );
    }

    #[test]
    fn default_masterclock_advances_without_pad_playback() {
        let mut transport = TransportTimeline::new(48_000);

        transport.advance_by_rendered_frames(512);
        transport.advance_by_rendered_frames(512);

        assert_eq!(transport.output_frame(), 1_024);
        assert_eq!(
            transport.master_period_seconds(),
            Some(DEFAULT_MASTER_PERIOD_SECONDS)
        );
        assert_eq!(transport.master_bpm(), Some(120.0));
        assert_eq!(transport.downbeat_frame(), 0);
        assert_eq!(transport.beat_position(), Some(1_024.0 / 24_000.0));
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
        transport.clear_master_bpm();

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
        assert_eq!(
            transport.next_grid_frame(QuantizeGrid::from_step_64ths(16).unwrap()),
            Some(25_000)
        );
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
    fn next_grid_frame_beat_uses_current_frame_on_grid_boundary() {
        let transport = transport_at(24_000);

        assert_eq!(
            transport.next_grid_frame(QuantizeGrid::from_step_64ths(16).unwrap()),
            Some(24_000)
        );
    }

    #[test]
    fn next_grid_frame_beat_targets_next_boundary_between_beats() {
        let transport = transport_at(24_001);

        assert_eq!(
            transport.next_grid_frame(QuantizeGrid::from_step_64ths(16).unwrap()),
            Some(48_000)
        );
    }

    #[test]
    fn next_grid_frame_bar_uses_current_frame_on_bar_boundary() {
        let transport = transport_at(96_000);

        assert_eq!(
            transport.next_grid_frame(QuantizeGrid::from_step_64ths(64).unwrap()),
            Some(96_000)
        );
    }

    #[test]
    fn next_grid_frame_bar_targets_next_boundary_between_bars() {
        let transport = transport_at(24_000);

        assert_eq!(
            transport.next_grid_frame(QuantizeGrid::from_step_64ths(64).unwrap()),
            Some(96_000)
        );
    }

    #[test]
    fn next_grid_frame_respects_downbeat_anchor() {
        let mut transport = transport_at(1_001);
        transport.set_downbeat_frame(1_000);

        assert_eq!(
            transport.next_grid_frame(QuantizeGrid::from_step_64ths(16).unwrap()),
            Some(25_000)
        );
        assert_eq!(
            transport.next_grid_frame(QuantizeGrid::from_step_64ths(64).unwrap()),
            Some(97_000)
        );
    }

    #[test]
    fn next_grid_frame_supports_loop_editor_64th_note_subdivisions() {
        let transport = transport_at(6_001);
        let sixteenth_note = QuantizeGrid::from_step_64ths(4).unwrap();
        let sixty_fourth_note = QuantizeGrid::from_step_64ths(1).unwrap();

        assert_eq!(transport.next_grid_frame(sixteenth_note), Some(12_000));
        assert_eq!(transport.next_grid_frame(sixty_fourth_note), Some(7_500));
    }

    #[test]
    fn next_grid_frame_uses_future_boundary_when_previous_is_closer() {
        let transport = transport_at(6_001);
        let sixteenth_note = QuantizeGrid::from_step_64ths(4).unwrap();

        assert_eq!(transport.next_grid_frame(sixteenth_note), Some(12_000));
    }

    #[test]
    fn master_bpm_anchor_can_use_arbitrary_output_frame() {
        let mut transport = TransportTimeline::new(48_000);

        assert!(transport.set_master_bpm_and_anchor_beat_position_at_frame(60.0, 2.5, 60_000));

        assert_eq!(transport.master_bpm(), Some(60.0));
        assert_eq!(transport.downbeat_frame(), 132_000);
        assert_eq!(transport.bar_phase_beats_at_frame(60_000), Some(2.5));
    }

    #[test]
    fn master_bpm_update_can_preserve_current_bar_phase() {
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(10);

        assert_eq!(transport.bar_phase_beats(), Some(1.0));

        assert!(
            transport
                .set_master_bpm_preserving_beat_position_at_frame(120.0, transport.output_frame())
        );

        assert_eq!(transport.master_bpm(), Some(120.0));
        assert_eq!(transport.downbeat_frame(), 5);
        assert_eq!(transport.bar_phase_beats(), Some(1.0));
    }

    #[test]
    fn invalid_quantize_grid_step_is_rejected() {
        assert_eq!(QuantizeGrid::from_step_64ths(0), None);
        assert_eq!(QuantizeGrid::from_step_64ths(GRID_64THS_PER_BAR + 1), None);
    }

    #[test]
    fn missing_master_bpm_disables_quantized_grid_targets() {
        let mut transport = TransportTimeline::new(48_000);
        transport.clear_master_bpm();

        assert_eq!(transport.beat_position(), None);
        assert_eq!(transport.bar_phase_beats_at_frame(48_000), None);
        assert_eq!(transport.bar_phase_beats(), None);
        assert_eq!(
            transport.next_grid_frame(QuantizeGrid::from_step_64ths(16).unwrap()),
            None
        );
        assert_eq!(
            transport.next_grid_frame(QuantizeGrid::from_step_64ths(64).unwrap()),
            None
        );
    }

    #[test]
    fn bpm_changes_preserve_whole_bars_and_fractional_beats() {
        for rate in [44_100, 48_000] {
            let mut transport = TransportTimeline::new(rate);
            transport.set_master_bpm(123.45);
            transport.advance_by_rendered_frames(rate as usize * 17 + 137);
            let before = transport.beat_position().unwrap();
            assert!(before > 32.0);
            let frame = transport.output_frame();
            for bpm in [87.65, 155.25, 123.45] {
                assert!(transport.set_master_bpm_preserving_beat_position_at_frame(bpm, frame));
                assert!((transport.beat_position().unwrap() - before).abs() < 1e-12);
                assert_eq!(transport.output_frame(), frame);
            }
            transport.advance_by_rendered_frames(rate as usize);
            assert!((transport.beat_position().unwrap() - before - 123.45 / 60.0).abs() < 1e-12);
        }
    }

    #[test]
    fn virtual_negative_and_fractional_origin_controls_exact_grid() {
        let mut transport = TransportTimeline::new(48_000);
        assert!(transport.anchor_beat_position_at_frame(0.125_02, 0));
        let grid = QuantizeGrid::from_step_64ths(4).unwrap();
        assert_eq!(transport.next_grid_frame(grid), Some(3_000));
        let phase = transport.beat_position().unwrap();
        assert!(!transport.anchor_beat_position_at_frame(f64::NAN, 7));
        assert_eq!(transport.beat_position(), Some(phase));
    }

    #[test]
    fn selected_reference_bootstraps_once_without_resetting_output_clock() {
        let mut transport = transport_at(96_007);
        transport.request_bootstrap(3);
        transport.request_bootstrap(9);
        assert_eq!(transport.bootstrap_reference(), Some(3));
        assert!(!transport.bootstrap_from_source_at_frame(120.0, f64::NAN, 96_007));
        assert_eq!(transport.bootstrap_reference(), Some(3));
        assert!(transport.bootstrap_from_source_at_frame(90.0, -5.25, 96_007));
        assert_eq!(transport.beat_position(), Some(-5.25));
        assert_eq!(transport.output_frame(), 96_007);
        transport.request_bootstrap(9);
        assert_eq!(transport.bootstrap_reference(), None);
        assert!(!transport.bootstrap_from_source_at_frame(120.0, 8.0, 96_007));
        assert_eq!(transport.beat_position(), Some(-5.25));
    }

    #[test]
    fn unloading_pending_reference_releases_unused_bootstrap() {
        let mut transport = transport_at(100);
        transport.request_bootstrap(3);
        transport.clear_pending_bootstrap_for_pad(2);
        assert_eq!(transport.bootstrap_reference(), Some(3));
        transport.clear_pending_bootstrap_for_pad(3);
        transport.request_bootstrap(9);
        assert_eq!(transport.bootstrap_reference(), Some(9));
    }

    #[test]
    fn accepted_period_selects_true_fractional_boundary_without_binary32_bpm_roundtrip() {
        let period_seconds = 0.500_000_001_234_567_9;
        let mut transport = TransportTimeline::new(48_000);
        assert!(transport.set_master_period(period_seconds));
        transport.advance_by_rendered_frames(24_000);
        let grid = QuantizeGrid::from_step_64ths(16).unwrap();

        assert_eq!(transport.master_period_seconds(), Some(period_seconds));
        assert_eq!(transport.frames_per_beat(), Some(48_000.0 * period_seconds));
        // The true first quarter lies strictly after frame 24000. Its first
        // available output frame is 24001, independently of callback partition.
        assert_eq!(transport.next_grid_frame(grid), Some(24_001));
        let lossy_bpm = (60.0 / period_seconds) as f32;
        assert_eq!(lossy_bpm, 120.0);
        assert!(transport.set_master_bpm(f64::from(lossy_bpm)));
        assert_eq!(transport.next_grid_frame(grid), Some(24_000));
    }

    #[test]
    fn direct_period_bootstrap_and_rate_change_preserve_exact_period_and_beat_epoch() {
        let first_period = 0.500_000_001_234_567_9;
        let next_period = 0.734_567_890_123_456_7;
        let mut transport = TransportTimeline::new(48_000);
        transport.advance_by_rendered_frames(96_007);
        transport.request_bootstrap(3);
        assert!(transport.bootstrap_from_source_period_at_frame(first_period, -5.25, 96_007));
        assert_eq!(transport.master_period_seconds(), Some(first_period));
        assert_eq!(transport.beat_position(), Some(-5.25));
        assert_eq!(transport.bootstrap_reference(), None);

        transport.advance_by_rendered_frames(137);
        let beat_before = transport.beat_position().unwrap();
        let frame = transport.output_frame();
        assert!(transport.set_master_period_preserving_beat_position_at_frame(next_period, frame));
        assert_eq!(transport.master_period_seconds(), Some(next_period));
        assert_eq!(transport.beat_position(), Some(beat_before));
        assert_eq!(transport.output_frame(), frame);
        transport.advance_by_rendered_frames(48_000);
        assert!(
            (transport.beat_position().unwrap() - beat_before - 1.0 / next_period).abs() < 1e-12
        );
        assert!(!transport.bootstrap_from_source_period_at_frame(first_period, 0.0, frame));
        assert_eq!(transport.master_period_seconds(), Some(next_period));
    }

    #[test]
    fn invalid_period_or_anchor_preserves_current_clock_and_pending_bootstrap() {
        let period_seconds = 0.500_000_001_234_567_9;
        let mut transport = TransportTimeline::new(48_000);
        assert!(
            transport.set_master_period_and_anchor_beat_position_at_frame(
                period_seconds,
                -5.25,
                96_007,
            )
        );
        transport.advance_by_rendered_frames(96_144);
        transport.request_bootstrap(3);
        let frame = transport.output_frame();
        let downbeat = transport.downbeat_frame();
        let beat = transport.beat_position();
        for invalid in [f64::NAN, f64::INFINITY, 0.0, -1.0, f64::MAX] {
            assert!(!transport.set_master_period(invalid));
            assert!(!transport.set_master_period_preserving_beat_position_at_frame(invalid, frame));
            assert!(!transport.bootstrap_from_source_period_at_frame(invalid, 1.0, frame));
            assert_eq!(transport.master_period_seconds(), Some(period_seconds));
            assert_eq!(transport.beat_position(), beat);
            assert_eq!(transport.downbeat_frame(), downbeat);
            assert_eq!(transport.output_frame(), frame);
            assert_eq!(transport.bootstrap_reference(), Some(3));
        }
        assert!(
            !transport.set_master_period_and_anchor_beat_position_at_frame(0.75, f64::NAN, frame)
        );
        assert!(!transport.bootstrap_from_source_period_at_frame(0.75, f64::NAN, frame));
        assert_eq!(transport.master_period_seconds(), Some(period_seconds));
        assert_eq!(transport.beat_position(), beat);
        assert_eq!(transport.downbeat_frame(), downbeat);
        assert_eq!(transport.bootstrap_reference(), Some(3));
    }
}
