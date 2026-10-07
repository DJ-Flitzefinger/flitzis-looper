//! Scalar source clock shared by live resampling and non-realtime source preparation.
//!
//! A constant-rate epoch is addressed by its active output-frame count, never by callback
//! endpoints. Rebases preserve the fractional source position. All operations are bounded and
//! allocation-free; pausing simply stops advancing this clock.

use super::constants::{SPEED_MAX, SPEED_MIN};
#[cfg(test)]
use super::source_reader::FrameRange;
use super::source_reader::{
    ExplicitSeekMode, SourceLoopDomain, advance_playback_position, playhead_before_render,
};

const TEMPO_STEP: f64 = 0.05;
const TEMPO_STEP_OUTPUT_FRAMES: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FractionalSourcePosition {
    pub(crate) frame: usize,
    pub(crate) fraction: f64,
    pub(crate) seek_mode: ExplicitSeekMode,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SourcePlayback {
    origin: FractionalSourcePosition,
    domain: Option<SourceLoopDomain>,
    elapsed_output_frames: u64,
    ratio: f64,
    target: f64,
    frames_until_step: usize,
}

impl SourcePlayback {
    pub(crate) fn new(frame: usize, seek_mode: ExplicitSeekMode, ratio: f64) -> Self {
        let ratio = checked_ratio(ratio);
        Self {
            origin: FractionalSourcePosition {
                frame,
                fraction: 0.0,
                seek_mode,
            },
            domain: None,
            elapsed_output_frames: 0,
            ratio,
            target: ratio,
            frames_until_step: 0,
        }
    }

    /// Copy the current source phase into a constant-rate preparation epoch.
    ///
    /// Pending live smoothing is removed only from this value. Its logical counterpart retains
    /// the accepted target and active-frame step interval.
    #[cfg(test)]
    pub(crate) fn at_constant_ratio(mut self, ratio: f64) -> Self {
        self.rebase();
        self.ratio = checked_ratio(ratio);
        self.target = self.ratio;
        self.frames_until_step = 0;
        self
    }

    pub(crate) fn tempo_ratio(&self) -> f64 {
        self.ratio
    }

    /// Compare the complete copied trajectory at a prepared native adoption boundary.
    ///
    /// Equal current source phase is insufficient: a pending rate target or a different active
    /// smoothing interval changes future feed. The copied worker cursor must reach the same
    /// canonical epoch and exact binary64 state, rather than merely a wrapped position.
    pub(crate) fn matches_exact(&self, other: &Self) -> bool {
        self.origin.frame == other.origin.frame
            && self.origin.fraction.to_bits() == other.origin.fraction.to_bits()
            && self.origin.seek_mode == other.origin.seek_mode
            && self.domain == other.domain
            && self.elapsed_output_frames == other.elapsed_output_frames
            && self.ratio.to_bits() == other.ratio.to_bits()
            && self.target.to_bits() == other.target.to_bits()
            && self.frames_until_step == other.frames_until_step
    }

    /// Check pending preparation control intent while its copied smoothing clock advances.
    pub(crate) fn matches_rate_target(&self, other: &Self) -> bool {
        self.target.to_bits() == other.target.to_bits()
    }

    #[cfg(test)]
    pub(crate) fn configure(&mut self, sample_frames: usize, region: FrameRange) {
        self.configure_domain(SourceLoopDomain::physical(sample_frames, region));
    }

    pub(crate) fn loop_period(&self) -> Option<f64> {
        self.domain.and_then(|domain| domain.musical_period)
    }

    pub(crate) fn matches_domain(&self, domain: SourceLoopDomain) -> bool {
        self.domain == Some(domain)
    }

    pub(crate) fn configure_domain(&mut self, domain: SourceLoopDomain) {
        if self.domain == Some(domain) {
            return;
        }
        let same_geometry = self.domain.is_some_and(|previous| {
            previous.sample_frames == domain.sample_frames && previous.region == domain.region
        });
        self.rebase();
        self.domain = Some(domain);
        let outside = if let Some(period) = domain.musical_period {
            let offset =
                self.origin.frame.saturating_sub(domain.region.start) as f64 + self.origin.fraction;
            self.origin.seek_mode == ExplicitSeekMode::Normal
                && (self.origin.frame < domain.region.start || offset >= period)
        } else {
            playhead_before_render(self.origin.frame, domain.region, self.origin.seek_mode)
                != self.origin.frame
        };
        // A timing-only domain change wraps the retained virtual origin naturally.
        // Actual loop/extent edits keep the established out-of-range clamp policy.
        if outside && !same_geometry {
            self.origin.frame = domain.region.start;
            self.origin.fraction = 0.0;
        }
    }

    pub(crate) fn clear_explicit_seek(&mut self) {
        self.rebase();
        let was_explicit = self.origin.seek_mode != ExplicitSeekMode::Normal;
        self.origin.seek_mode = ExplicitSeekMode::Normal;
        // Exiting intro/tail normalizes on the next configuration. An already normal
        // cursor retains its geometry so a same-marker timing refresh cannot discard
        // the virtual seam's fractional residue.
        if was_explicit {
            self.domain = None;
        }
    }

    pub(crate) fn set_target(&mut self, target: f64) {
        let target = checked_ratio(target);
        if target != self.target {
            self.target = target;
            self.frames_until_step = 0;
        }
    }

    /// A seek changes source phase, while the accepted rate target and smoothing clock continue.
    pub(crate) fn seek(&mut self, frame: usize, seek_mode: ExplicitSeekMode) {
        self.origin = FractionalSourcePosition {
            frame,
            fraction: 0.0,
            seek_mode,
        };
        self.domain = None;
        self.elapsed_output_frames = 0;
    }

    /// Begin a constant-ratio chunk, bounded by the next active-output-frame smoothing step.
    pub(crate) fn chunk(&mut self, max_frames: usize) -> (usize, f64) {
        if self.frames_until_step == 0 && self.ratio != self.target {
            self.rebase();
            let delta = (self.target - self.ratio).clamp(-TEMPO_STEP, TEMPO_STEP);
            self.ratio = if (self.target - self.ratio).abs() <= TEMPO_STEP {
                self.target
            } else {
                (self.ratio + delta).clamp(SPEED_MIN, SPEED_MAX)
            };
            self.frames_until_step = TEMPO_STEP_OUTPUT_FRAMES;
        }
        let frames = if self.ratio == self.target {
            max_frames
        } else {
            max_frames.min(self.frames_until_step)
        };
        (frames, self.ratio)
    }

    pub(crate) fn position(&self) -> FractionalSourcePosition {
        self.position_at(0)
    }

    pub(crate) fn position_at(&self, output_offset: usize) -> FractionalSourcePosition {
        let distance = self.origin.fraction
            + self
                .elapsed_output_frames
                .saturating_add(output_offset as u64) as f64
                * self.ratio;
        if let Some(domain) = self.domain
            && let Some(period) = domain.musical_period
            && period != domain.region.len() as f64
        {
            return self.musical_position_at(distance, domain, period);
        }
        let whole = distance.floor() as usize;
        let (frame, seek_mode) = self.domain.map_or(
            (
                self.origin.frame.saturating_add(whole),
                self.origin.seek_mode,
            ),
            |domain| {
                advance_playback_position(
                    self.origin.frame,
                    whole,
                    domain.sample_frames,
                    domain.region,
                    self.origin.seek_mode,
                )
            },
        );
        FractionalSourcePosition {
            frame,
            fraction: distance - whole as f64,
            seek_mode,
        }
    }

    /// Retain virtual phase in source-frame units, including the fractional seam.
    /// Physical interpolation is the reader's responsibility; rebases never project
    /// this value to a PCM tap and therefore cannot lose musical phase.
    fn musical_position_at(
        &self,
        distance: f64,
        domain: SourceLoopDomain,
        period: f64,
    ) -> FractionalSourcePosition {
        let (phase, mode) = match self.origin.seek_mode {
            ExplicitSeekMode::Normal => (
                (self.origin.frame.saturating_sub(domain.region.start) as f64 + distance)
                    .rem_euclid(period),
                ExplicitSeekMode::Normal,
            ),
            ExplicitSeekMode::BeforeLoop | ExplicitSeekMode::AfterLoop => {
                let boundary = if self.origin.seek_mode == ExplicitSeekMode::BeforeLoop {
                    domain.region.start
                } else {
                    domain.sample_frames
                };
                let prefix = boundary.saturating_sub(self.origin.frame) as f64;
                if distance < prefix {
                    let whole = distance.floor() as usize;
                    return FractionalSourcePosition {
                        frame: self.origin.frame.saturating_add(whole),
                        fraction: distance - whole as f64,
                        seek_mode: self.origin.seek_mode,
                    };
                }
                (
                    (distance - prefix).rem_euclid(period),
                    ExplicitSeekMode::Normal,
                )
            }
        };
        let whole = phase.floor() as usize;
        FractionalSourcePosition {
            frame: domain.region.start + whole,
            fraction: phase - whole as f64,
            seek_mode: mode,
        }
    }

    pub(crate) fn advance(&mut self, output_frames: usize) {
        self.elapsed_output_frames = self
            .elapsed_output_frames
            .saturating_add(output_frames as u64);
        self.frames_until_step = self.frames_until_step.saturating_sub(output_frames);
    }

    fn rebase(&mut self) {
        self.origin = self.position();
        self.elapsed_output_frames = 0;
    }
}

fn checked_ratio(ratio: f64) -> f64 {
    if ratio.is_finite() {
        ratio.clamp(SPEED_MIN, SPEED_MAX)
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn musical_domain_preserves_virtual_seam_phase_through_rebase_rate_seek_and_edits() {
        let region = FrameRange { start: 2, end: 5 };
        let domain = SourceLoopDomain::musical(6, region, 3.25).unwrap();
        let mut playback = SourcePlayback::new(2, ExplicitSeekMode::Normal, 0.5);
        playback.configure_domain(domain);
        playback.advance(6);
        assert_eq!(playback.position().frame, 5); // Virtual, exclusive physical end.
        assert_eq!(playback.position().fraction, 0.0);
        let before = playback.position();
        playback.set_target(0.55);
        assert_eq!(playback.chunk(1), (1, 0.55));
        assert_eq!(playback.position(), before);
        playback.advance(1);
        assert!((playback.position().fraction - 0.3).abs() < 1.0e-14);
        assert_eq!(playback.position().frame, 2);
        let retained = playback.position();
        playback.configure_domain(SourceLoopDomain::musical(6, region, 2.75).unwrap());
        assert_eq!(playback.position(), retained);
        playback.seek(5, ExplicitSeekMode::AfterLoop);
        playback.configure_domain(domain);
        assert_eq!(playback.position().seek_mode, ExplicitSeekMode::AfterLoop);
        playback.advance(4);
        assert_eq!(playback.position().seek_mode, ExplicitSeekMode::Normal);
        assert!((playback.position().fraction - 0.2).abs() < 1.0e-14);
        playback.clear_explicit_seek();
        playback.configure_domain(domain);
        assert!((playback.position().fraction - 0.2).abs() < 1.0e-14);
    }

    #[test]
    fn subframe_period_change_wraps_fraction_without_changing_integer_frame() {
        let region = FrameRange { start: 2, end: 3 };
        let mut playback = SourcePlayback::new(2, ExplicitSeekMode::Normal, 0.7);
        playback.configure(4, region);
        playback.advance(1);
        assert_eq!(playback.position().fraction, 0.7);
        playback.configure_domain(SourceLoopDomain::musical(4, region, 0.25).unwrap());
        assert_eq!(playback.position().frame, 2);
        assert!((playback.position().fraction - 0.2).abs() < 1.0e-14);
        playback.advance(13);
        assert_eq!(playback.position().frame, 2);
        assert!((playback.position().fraction - 0.05).abs() < 1.0e-14);
    }

    #[test]
    fn same_geometry_period_shrink_and_clear_preserve_wrapped_fractional_residue() {
        let region = FrameRange {
            start: 10,
            end: 1510,
        };
        let initial = SourceLoopDomain::musical(1600, region, 1500.25).unwrap();
        let shorter = SourceLoopDomain::musical(1600, region, 1499.75).unwrap();
        // The power-of-two divisor reaches this exact binary64 seam at a legal rate.
        let mut playback = SourcePlayback::new(10, ExplicitSeekMode::Normal, 1500.125 / 2048.0);
        playback.configure_domain(initial);
        playback.advance(2048);
        let before = playback.position();
        assert_eq!(before.frame, 1510);
        assert_eq!(before.fraction, 0.125);
        let mut cleared = playback;
        playback.clear_explicit_seek(); // Same physical marker reapplication.
        playback.configure_domain(shorter);
        assert_eq!(playback.position().frame, 10);
        assert_eq!(playback.position().fraction, 0.375);
        assert_eq!(
            shorter.project_normal_position(before),
            Some(playback.position())
        );
        cleared.clear_explicit_seek();
        cleared.configure(1600, region);
        assert_eq!(cleared.position().frame, 10);
        assert_eq!(cleared.position().fraction, 0.125);
        assert_eq!(
            SourceLoopDomain::physical(1600, region).project_normal_position(before),
            Some(cleared.position())
        );
        let changed_geometry = SourceLoopDomain::musical(
            1600,
            FrameRange {
                start: 20,
                end: 1520,
            },
            1500.25,
        )
        .unwrap();
        playback.clear_explicit_seek();
        playback.configure_domain(changed_geometry);
        assert_eq!(playback.position().frame, 20);
        assert_eq!(playback.position().fraction, 0.0);
    }

    #[test]
    fn clearing_real_intro_or_tail_seek_still_normalizes_outside_loop_to_start() {
        let region = FrameRange { start: 2, end: 5 };
        let domain = SourceLoopDomain::musical(6, region, 3.25).unwrap();
        for (frame, mode) in [
            (0, ExplicitSeekMode::BeforeLoop),
            (5, ExplicitSeekMode::AfterLoop),
        ] {
            let mut playback = SourcePlayback::new(frame, mode, 0.73);
            playback.configure_domain(domain);
            playback.advance(1);
            assert_eq!(playback.position().seek_mode, mode);
            playback.clear_explicit_seek();
            playback.configure_domain(domain);
            assert_eq!(playback.position().frame, 2);
            assert_eq!(playback.position().fraction, 0.0);
            assert_eq!(playback.position().seek_mode, ExplicitSeekMode::Normal);
        }
    }

    #[test]
    fn copied_musical_domain_requires_exact_period_and_preserves_integer_control_arithmetic() {
        let region = FrameRange { start: 7, end: 101 };
        let mut physical = SourcePlayback::new(7, ExplicitSeekMode::Normal, 0.73);
        physical.configure(101, region);
        let mut musical = SourcePlayback::new(7, ExplicitSeekMode::Normal, 0.73);
        musical.configure_domain(SourceLoopDomain::musical(101, region, 94.0).unwrap());
        for frames in [1, 31, 96, 257, 512, 1_000_001] {
            physical.advance(frames);
            musical.advance(frames);
            assert_eq!(physical.position(), musical.position());
        }
        let mut changed = musical;
        changed.configure_domain(
            SourceLoopDomain::musical(101, region, f64::from_bits(94.0_f64.to_bits() + 1)).unwrap(),
        );
        assert!(!musical.matches_exact(&changed));
        assert_eq!(musical.loop_period(), Some(94.0));
    }

    #[test]
    fn intro_and_tail_traverse_physical_extent_once_then_use_fractional_musical_period() {
        let region = FrameRange { start: 13, end: 71 };
        let domain = SourceLoopDomain::musical(100, region, 58.25).unwrap();
        for (frame, mode, prefix) in [
            (0, ExplicitSeekMode::BeforeLoop, 13.0),
            (89, ExplicitSeekMode::AfterLoop, 11.0),
        ] {
            let mut playback = SourcePlayback::new(frame, mode, 0.73);
            playback.configure_domain(domain);
            playback.advance(7);
            let earlier = playback.position();
            assert_eq!(earlier.seek_mode, mode);
            assert!((earlier.frame as f64 + earlier.fraction - frame as f64 - 5.11).abs() < 1e-12);
            playback.advance(93);
            let phase = (100.0_f64 * 0.73 - prefix).rem_euclid(58.25);
            let position = playback.position();
            assert_eq!(position.seek_mode, ExplicitSeekMode::Normal);
            assert_eq!(position.frame, 13 + phase.floor() as usize);
            assert_eq!(position.fraction, phase.fract());
        }
    }

    #[test]
    fn musical_rate_epochs_and_copied_clock_are_partition_invariant_on_both_sides_of_physical_end()
    {
        let region = FrameRange { start: 7, end: 101 };
        for period in [93.75, 94.25] {
            let domain = SourceLoopDomain::musical(110, region, period).unwrap();
            let run = |partition: &[usize]| {
                let mut playback =
                    SourcePlayback::new(7, ExplicitSeekMode::Normal, 0.9345678901234568);
                playback.configure_domain(domain);
                playback.advance(17);
                playback.set_target(1.1234567890123457);
                let mut trace = Vec::new();
                let mut rendered = 0;
                let mut segment = 0;
                while rendered < 8_321 {
                    let requested = partition[segment % partition.len()].min(8_321 - rendered);
                    let mut remaining = requested;
                    while remaining > 0 {
                        let (frames, ratio) = playback.chunk(remaining);
                        trace.extend(
                            (0..frames).map(|offset| (ratio, playback.position_at(offset))),
                        );
                        playback.advance(frames);
                        remaining -= frames;
                    }
                    rendered += requested;
                    segment += 1;
                }
                (trace, playback)
            };
            let (reference, checkpoint) = run(&[1]);
            let mut independent_distance: f64 = 17.0 * 0.9345678901234568;
            for (ratio, position) in &reference {
                let phase = independent_distance.rem_euclid(period);
                let actual = position.frame as f64 + position.fraction - 7.0;
                assert!((actual - phase).abs() < 1.0e-8);
                independent_distance += ratio;
            }
            for partition in [&[512][..], &[31, 257, 1, 96, 777][..]] {
                let (actual, copied) = run(partition);
                assert_eq!(actual, reference);
                assert!(checkpoint.matches_exact(&copied));
            }
        }
    }

    #[test]
    fn prepared_adoption_requires_equal_future_rate_target_and_smoothing_interval() {
        let region = FrameRange { start: 7, end: 101 };
        let mut current = SourcePlayback::new(7, ExplicitSeekMode::Normal, 1.0);
        current.configure(101, region);
        let mut changed_target = current;
        changed_target.set_target(1.0 + 2.0_f64.powi(-24));
        assert_eq!(current.position(), changed_target.position());
        assert_eq!(current.tempo_ratio(), changed_target.tempo_ratio());
        assert!(!current.matches_exact(&changed_target));
        assert!(!current.matches_rate_target(&changed_target));
        assert_ne!(current.chunk(1).1, changed_target.chunk(1).1);

        let ramp = |frames| {
            let mut playback = SourcePlayback::new(7, ExplicitSeekMode::Normal, 1.0);
            playback.configure(101, region);
            playback.set_target(1.2);
            playback.chunk(frames);
            playback.advance(frames);
            playback.seek(47, ExplicitSeekMode::Normal);
            playback.configure(101, region);
            playback
        };
        let mut later_step = ramp(100);
        let mut earlier_step = ramp(101);
        assert_eq!(later_step.position(), earlier_step.position());
        assert_eq!(later_step.tempo_ratio(), earlier_step.tempo_ratio());
        assert!(!later_step.matches_exact(&earlier_step));
        assert!(later_step.matches_rate_target(&earlier_step));
        assert_eq!(later_step.chunk(512).0, 412);
        assert_eq!(earlier_step.chunk(512).0, 411);
    }

    #[test]
    fn copied_prepared_trajectory_matches_exactly_across_canonical_rate_steps_and_partitions() {
        let mut initial = SourcePlayback::new(95, ExplicitSeekMode::AfterLoop, 0.73);
        initial.configure(100, FrameRange { start: 13, end: 71 });
        initial.advance(2);
        initial.set_target(1.371_234_567_890_123);
        let advance = |mut playback: SourcePlayback, partition: &[usize]| {
            let mut elapsed = 0;
            let mut segment = 0;
            while elapsed < 8_321 {
                let requested = partition[segment % partition.len()].min(8_321 - elapsed);
                let mut remaining = requested;
                while remaining > 0 {
                    let (frames, _) = playback.chunk(remaining);
                    playback.advance(frames);
                    remaining -= frames;
                }
                elapsed += requested;
                segment += 1;
            }
            playback
        };
        let reference = advance(initial, &[1]);
        for partition in [&[512][..], &[31, 257, 1, 96, 777][..]] {
            let prepared = advance(initial, partition);
            assert!(reference.matches_exact(&prepared));
            assert_eq!(reference.position(), prepared.position());
        }
    }

    #[test]
    fn long_constant_epoch_uses_exact_count_instead_of_segment_rounding() {
        let ratio = 123.45_f64 / 97.3_f64;
        let mut playback = SourcePlayback::new(7, ExplicitSeekMode::Normal, ratio);
        playback.configure(101, FrameRange { start: 7, end: 101 });
        // Thirty minutes at 96 kHz, advanced with unrelated chunk sizes.
        let total = 96_000 * 60 * 30;
        for frames in [1, 31, 512, total - 544] {
            playback.advance(frames);
        }
        let distance = total as f64 * ratio;
        let position = playback.position();
        assert_eq!(position.frame, 7 + distance.floor() as usize % 94);
        assert_eq!(position.fraction, distance.fract());
        let narrowed_distance = total as f64 * f64::from(ratio as f32);
        assert_ne!(distance.floor(), narrowed_distance.floor());
    }

    #[test]
    fn binary64_target_change_below_one_binary32_step_retains_fractional_source_epoch() {
        // Both rates round to 1.0 in binary32, but this target must change the live source rate.
        let target = 1.0 + 2.0_f64.powi(-24);
        assert_eq!(target as f32, 1.0);
        let mut playback = SourcePlayback::new(7, ExplicitSeekMode::Normal, 1.0);
        playback.configure(101, FrameRange { start: 7, end: 101 });
        playback.advance(17);
        let before = playback.position();
        playback.set_target(target);
        assert_eq!(playback.chunk(8_388_608), (8_388_608, target));
        assert_eq!(playback.position(), before);
        playback.advance(8_388_608);
        let position = playback.position();
        assert_eq!(position.frame, 7 + (17 + 8_388_608) % 94);
        assert_eq!(position.fraction, 0.5);

        let next_target = target + 2.0_f64.powi(-25);
        playback.set_target(next_target);
        assert_eq!(playback.chunk(1), (1, next_target));
        assert_eq!(playback.position(), position);
        playback.advance(1);
        assert_eq!(playback.position().fraction, (0.5 + next_target).fract());
    }

    #[test]
    fn binary64_ramp_and_position_are_identical_under_unrelated_partitions() {
        let start = 0.9345678901234568;
        let target = 1.1234567890123457;
        let total = 2309;
        let region = FrameRange { start: 7, end: 101 };
        let run = |partition: &[usize]| {
            let mut playback = SourcePlayback::new(7, ExplicitSeekMode::Normal, start);
            playback.configure(101, region);
            playback.advance(17);
            playback.set_target(target);
            let mut trace = Vec::with_capacity(total);
            let mut rendered = 0;
            let mut segment = 0;
            while rendered < total {
                let requested = partition[segment % partition.len()].min(total - rendered);
                let mut remaining = requested;
                while remaining > 0 {
                    let (frames, ratio) = playback.chunk(remaining);
                    for offset in 0..frames {
                        trace.push((ratio, playback.position_at(offset)));
                    }
                    playback.advance(frames);
                    remaining -= frames;
                }
                rendered += requested;
                segment += 1;
            }
            (trace, playback.position())
        };
        let (reference, final_position) = run(&[1]);
        for partition in [&[512][..], &[1024][..], &[31, 257, 1, 96, 777][..]] {
            let (trace, position) = run(partition);
            assert_eq!(trace, reference);
            assert_eq!(position, final_position);
        }
        let ratios = [
            start + 0.05,
            start + 0.05 + 0.05,
            start + 0.05 + 0.05 + 0.05,
            target,
        ];
        let mut distance = 17.0 * start;
        for (frame, (ratio, position)) in reference.iter().enumerate() {
            let stage = (frame / 512).min(3);
            assert_eq!(*ratio, ratios[stage]);
            // Independent scalar integral: every accepted stage spans active output frames.
            assert_eq!(position.frame, 7 + distance.floor() as usize % 94);
            assert!((position.fraction - distance.fract()).abs() < 1.0e-9);
            distance += ratios[stage];
        }
        assert_eq!(final_position.frame, 7 + distance.floor() as usize % 94);
        assert!((final_position.fraction - distance.fract()).abs() < 1.0e-9);
    }

    #[test]
    fn copied_preparation_clock_and_live_continuation_have_identical_positions() {
        let mut live = SourcePlayback::new(0, ExplicitSeekMode::BeforeLoop, 0.73);
        live.configure(100, FrameRange { start: 13, end: 71 });
        live.advance(17);
        let mut prepared = live;
        for frames in [1, 31, 96, 257, 512] {
            for offset in 0..frames {
                assert_eq!(live.position_at(offset), prepared.position());
                prepared.advance(1);
            }
            live.advance(frames);
            assert_eq!(live.position(), prepared.position());
        }
    }

    #[test]
    fn exact_preparation_copy_retains_phase_and_does_not_consume_live_smoothing() {
        let mut logical = SourcePlayback::new(95, ExplicitSeekMode::AfterLoop, 0.73);
        logical.configure(100, FrameRange { start: 13, end: 71 });
        logical.advance(2);
        logical.set_target(1.25);
        assert_eq!(logical.chunk(3).0, 3);
        logical.advance(3);
        let position = logical.position();
        let remaining_step_frames = logical.frames_until_step;
        let exact_ratio = 1.37_f64;
        let mut feed = logical.at_constant_ratio(exact_ratio);

        assert_eq!(feed.position(), position);
        assert_eq!(feed.tempo_ratio(), exact_ratio);
        assert_eq!(feed.chunk(777), (777, exact_ratio));
        feed.advance(7);
        let distance = position.fraction + 7.0 * exact_ratio;
        let expected_frame = 13 + (position.frame + distance.floor() as usize - 100) % 58;
        assert_eq!(feed.position().frame, expected_frame);
        assert_eq!(feed.position().fraction, distance.fract());
        assert_eq!(feed.position().seek_mode, ExplicitSeekMode::Normal);

        assert_eq!(logical.position(), position);
        assert_eq!(logical.target, 1.25);
        assert_eq!(logical.chunk(777).0, remaining_step_frames);
        assert_ne!(logical.tempo_ratio(), exact_ratio);
    }

    #[test]
    fn seek_retains_the_remaining_active_frames_before_a_rate_step() {
        let mut playback = SourcePlayback::new(0, ExplicitSeekMode::Normal, 1.0);
        playback.configure(
            1000,
            FrameRange {
                start: 10,
                end: 900,
            },
        );
        playback.set_target(1.5);
        assert_eq!(playback.chunk(100), (100, 1.05));
        playback.advance(100);
        playback.seek(47, ExplicitSeekMode::Normal);
        playback.configure(
            1000,
            FrameRange {
                start: 10,
                end: 900,
            },
        );
        playback.set_target(1.5);
        assert_eq!(playback.position().frame, 47);
        assert_eq!(playback.position().fraction, 0.0);
        assert_eq!(playback.chunk(512), (412, 1.05));
        playback.advance(412);
        let (frames, ratio) = playback.chunk(1);
        assert_eq!(frames, 1);
        assert!((ratio - 1.1).abs() < 1e-6);
    }
}
