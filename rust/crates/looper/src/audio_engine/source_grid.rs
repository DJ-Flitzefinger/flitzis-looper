//! Bounded source-grid arithmetic for prepared constant-tempo loops.
//!
//! A grid origin is a signed, virtual source position rather than a readable
//! sample index. Mapping keeps the complete master beat position until the
//! effective loop supplies its period; reducing to one bar would lose the
//! second and subsequent bars of longer loops.

use crate::audio_engine::transport::{GRID_64THS_PER_BAR, GRID_64THS_PER_BEAT};

const LOOP_LENGTH_TOLERANCE_FRAMES: f64 = 1.0;
#[cfg(test)]
const PHASE_EPSILON_BEATS: f64 = 1.0e-9;

#[derive(Debug, Clone, Copy)]
pub(crate) struct SourceGrid {
    frames_per_beat: f64,
    origin_frame: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SourceLoopMetrics {
    pub(crate) start_beat: f64,
    /// Duration of the actual integer source-frame range.
    pub(crate) length_beats: f64,
    /// Exact musical period when frame rounding preserves a compatible loop.
    ///
    /// `None` means that mapping uses the actual physical frame duration and
    /// does not claim a shared short-loop/bar cycle after repeated wraps.
    pub(crate) compatible_cycle_beats: Option<f64>,
}

impl SourceGrid {
    pub(crate) fn new(sample_rate_hz: f64, pad_bpm: f32, origin_frame: f64) -> Option<Self> {
        if !sample_rate_hz.is_finite()
            || sample_rate_hz <= 0.0
            || !pad_bpm.is_finite()
            || pad_bpm <= 0.0
            || !origin_frame.is_finite()
        {
            return None;
        }

        let frames_per_beat = sample_rate_hz * 60.0 / f64::from(pad_bpm);
        if !frames_per_beat.is_finite() || frames_per_beat <= 0.0 {
            return None;
        }

        Some(Self {
            frames_per_beat,
            origin_frame,
        })
    }

    pub(crate) fn beat_at_source(self, frame: f64) -> Option<f64> {
        let beat = (frame - self.origin_frame) / self.frames_per_beat;
        beat.is_finite().then_some(beat)
    }

    #[cfg(test)]
    pub(crate) fn bar_phase_at_source(self, frame: f64) -> Option<f64> {
        let beats_per_bar = f64::from(GRID_64THS_PER_BAR) / f64::from(GRID_64THS_PER_BEAT);
        let phase = self.beat_at_source(frame)?.rem_euclid(beats_per_bar);
        Some(
            if phase <= PHASE_EPSILON_BEATS || beats_per_bar - phase <= PHASE_EPSILON_BEATS {
                0.0
            } else {
                phase
            },
        )
    }

    pub(crate) fn loop_metrics(
        self,
        loop_start: usize,
        loop_end: usize,
    ) -> Option<SourceLoopMetrics> {
        let length_frames = loop_end
            .checked_sub(loop_start)
            .filter(|length| *length > 0)?;
        let length_beats = length_frames as f64 / self.frames_per_beat;
        if !length_beats.is_finite() || length_beats <= 0.0 {
            return None;
        }

        Some(SourceLoopMetrics {
            start_beat: self.beat_at_source(loop_start as f64)?,
            length_beats,
            compatible_cycle_beats: self.compatible_cycle_beats(length_frames),
        })
    }

    pub(crate) fn source_at_master_beat(
        self,
        master_beat: f64,
        loop_start: usize,
        loop_end: usize,
    ) -> Option<usize> {
        let metrics = self.loop_metrics(loop_start, loop_end)?;
        let relative_beat = master_beat - metrics.start_beat;
        if !relative_beat.is_finite() {
            return None;
        }

        let length_frames = loop_end - loop_start;
        let offset_frames = if let Some(cycle_beats) = metrics.compatible_cycle_beats {
            // Wrap musical time first so integer marker rounding cannot add
            // phase error on every cycle of a fractional-BPM loop.
            relative_beat.rem_euclid(cycle_beats) * self.frames_per_beat
        } else {
            // Arbitrary manual lengths retain their exact physical range.
            // They can be read safely without asserting sustained bar sync.
            let relative_frame = relative_beat * self.frames_per_beat;
            if !relative_frame.is_finite() {
                return None;
            }
            relative_frame.rem_euclid(length_frames as f64)
        };
        if !offset_frames.is_finite() {
            return None;
        }

        let rounded_offset = offset_frames.round();
        // Rounding at the exclusive end belongs to the next loop beginning.
        // This also bounds the at-most-one-frame compatible-length mismatch.
        let offset = if rounded_offset >= length_frames as f64 {
            0
        } else {
            rounded_offset as usize
        };
        Some(loop_start + offset)
    }

    fn compatible_cycle_beats(self, length_frames: usize) -> Option<f64> {
        let ticks =
            (length_frames as f64 / self.frames_per_beat * f64::from(GRID_64THS_PER_BEAT)).round();
        if !ticks.is_finite() || ticks < 1.0 || ticks >= u64::MAX as f64 {
            return None;
        }
        let ticks = ticks as u64;
        let ticks_per_bar = u64::from(GRID_64THS_PER_BAR);
        if !(ticks_per_bar.is_multiple_of(ticks) || ticks.is_multiple_of(ticks_per_bar)) {
            return None;
        }

        let cycle_beats = ticks as f64 / f64::from(GRID_64THS_PER_BEAT);
        let musical_length_frames = cycle_beats * self.frames_per_beat;
        if !musical_length_frames.is_finite()
            || (musical_length_frames - length_frames as f64).abs() > LOOP_LENGTH_TOLERANCE_FRAMES
        {
            return None;
        }
        Some(cycle_beats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_with_100_frames_per_beat(origin_frame: f64) -> SourceGrid {
        SourceGrid::new(100.0, 60.0, origin_frame).unwrap()
    }

    #[test]
    fn complete_master_beat_preserves_second_bar_of_long_loop() {
        let grid = grid_with_100_frames_per_beat(0.0);

        assert_eq!(grid.source_at_master_beat(1.0, 0, 800), Some(100));
        assert_eq!(grid.source_at_master_beat(5.0, 0, 800), Some(500));
        assert_eq!(grid.source_at_master_beat(13.0, 0, 800), Some(500));
        assert_eq!(grid.source_at_master_beat(-3.0, 0, 800), Some(500));
    }

    #[test]
    fn offbeat_loop_start_keeps_its_source_grid_phase() {
        let grid = grid_with_100_frames_per_beat(25.0);
        let metrics = grid.loop_metrics(150, 950).unwrap();

        assert_eq!(metrics.start_beat, 1.25);
        assert_eq!(metrics.length_beats, 8.0);
        assert_eq!(metrics.compatible_cycle_beats, Some(8.0));
        assert_eq!(grid.source_at_master_beat(1.25, 150, 950), Some(150));
        assert_eq!(grid.source_at_master_beat(0.0, 150, 950), Some(825));
        assert_eq!(grid.source_at_master_beat(5.25, 150, 950), Some(550));
    }

    #[test]
    fn signed_and_outside_file_origins_are_virtual_grid_positions() {
        let negative = grid_with_100_frames_per_beat(-125.0);
        assert_eq!(negative.beat_at_source(0.0), Some(1.25));
        assert_eq!(negative.bar_phase_at_source(0.0), Some(1.25));
        assert_eq!(negative.source_at_master_beat(0.0, 0, 800), Some(675));

        let beyond_file = grid_with_100_frames_per_beat(1_025.0);
        assert_eq!(beyond_file.beat_at_source(25.0), Some(-10.0));
        assert_eq!(beyond_file.bar_phase_at_source(25.0), Some(2.0));
        assert_eq!(beyond_file.source_at_master_beat(0.0, 0, 800), Some(225));
    }

    #[test]
    fn short_and_whole_bar_cycles_are_recognized() {
        let grid = grid_with_100_frames_per_beat(0.0);
        for (length, beats) in [(25, 0.25), (50, 0.5), (100, 1.0), (200, 2.0), (1_200, 12.0)] {
            assert_eq!(
                grid.loop_metrics(0, length).unwrap().compatible_cycle_beats,
                Some(beats)
            );
        }

        assert_eq!(grid.source_at_master_beat(3.5, 0, 200), Some(150));
        assert_eq!(grid.source_at_master_beat(4.0, 0, 200), Some(0));
    }

    #[test]
    fn arbitrary_physical_loops_have_explicit_compatibility_fallback() {
        let grid = grid_with_100_frames_per_beat(25.0);
        let metrics = grid.loop_metrics(100, 407).unwrap();

        assert_eq!(metrics.compatible_cycle_beats, None);
        assert_eq!(metrics.length_beats, 3.07);
        assert_eq!(grid.source_at_master_beat(4.0, 100, 407), Some(118));
        assert_eq!(grid.source_at_master_beat(0.0, 100, 407), Some(332));
        // Three-beat manual loops also do not tile the shared bar cycle.
        assert_eq!(
            grid.loop_metrics(0, 300).unwrap().compatible_cycle_beats,
            None
        );
    }

    #[test]
    fn fractional_bpm_cycles_do_not_accumulate_marker_rounding() {
        for sample_rate_hz in [44_100.0, 48_000.0] {
            let grid = SourceGrid::new(sample_rate_hz, 123.45, -217.0).unwrap();
            let loop_start = 1_007;
            let loop_end = loop_start + (grid.frames_per_beat * 8.0).round() as usize;
            let metrics = grid.loop_metrics(loop_start, loop_end).unwrap();
            assert_eq!(metrics.compatible_cycle_beats, Some(8.0));

            let phase_beat = metrics.start_beat + 0.375;
            let first_position = grid.source_at_master_beat(phase_beat, loop_start, loop_end);
            for cycle in [0_u32, 1, 10, 1_000, 1_000_000] {
                let cycle_beat = metrics.start_beat + f64::from(cycle) * 8.0;
                assert_eq!(
                    grid.source_at_master_beat(cycle_beat, loop_start, loop_end),
                    Some(loop_start)
                );
                assert_eq!(
                    grid.source_at_master_beat(
                        phase_beat + f64::from(cycle) * 8.0,
                        loop_start,
                        loop_end
                    ),
                    first_position
                );
            }
        }
    }

    #[test]
    fn compatibility_accepts_one_frame_and_rejects_larger_length_error() {
        let grid = grid_with_100_frames_per_beat(0.0);
        assert_eq!(
            grid.loop_metrics(0, 801).unwrap().compatible_cycle_beats,
            Some(8.0)
        );
        assert_eq!(
            grid.loop_metrics(0, 799).unwrap().compatible_cycle_beats,
            Some(8.0)
        );
        assert_eq!(
            grid.loop_metrics(0, 802).unwrap().compatible_cycle_beats,
            None
        );
    }

    #[test]
    fn rounded_reads_stay_inside_half_open_loop() {
        let grid = grid_with_100_frames_per_beat(0.0);
        assert_eq!(grid.source_at_master_beat(7.996, 0, 800), Some(0));
        assert_eq!(grid.source_at_master_beat(7.994, 0, 800), Some(799));
        assert_eq!(grid.source_at_master_beat(-0.004, 100, 900), Some(800));
        assert_eq!(grid.source_at_master_beat(8.0, 0, 800), Some(0));
    }

    #[test]
    fn invalid_inputs_fail_without_fabricating_a_grid_or_source_position() {
        for sample_rate in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            assert!(SourceGrid::new(sample_rate, 120.0, 0.0).is_none());
        }
        for bpm in [f32::NAN, f32::INFINITY, 0.0, -1.0] {
            assert!(SourceGrid::new(48_000.0, bpm, 0.0).is_none());
        }
        for origin in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(SourceGrid::new(48_000.0, 120.0, origin).is_none());
        }
        let grid = grid_with_100_frames_per_beat(0.0);
        for frame in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(grid.beat_at_source(frame), None);
            assert_eq!(grid.bar_phase_at_source(frame), None);
            assert_eq!(grid.source_at_master_beat(frame, 0, 800), None);
        }
        assert_eq!(grid.loop_metrics(800, 800), None);
        assert_eq!(grid.loop_metrics(801, 800), None);
        assert_eq!(grid.source_at_master_beat(1.0, 800, 800), None);
        assert!(SourceGrid::new(f64::MAX, 1.0, 0.0).is_none());
    }
}
