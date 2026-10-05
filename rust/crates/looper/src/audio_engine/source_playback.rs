//! Scalar source clock shared by live resampling and non-realtime source preparation.
//!
//! A constant-rate epoch is addressed by its active output-frame count, never by callback
//! endpoints. Rebases preserve the fractional source position. All operations are bounded and
//! allocation-free; pausing simply stops advancing this clock.

use super::constants::{SPEED_MAX, SPEED_MIN};
use super::source_reader::{
    ExplicitSeekMode, FrameRange, advance_playback_position, playhead_before_render,
};

const TEMPO_STEP: f32 = 0.05;
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
    domain: Option<(usize, FrameRange)>,
    elapsed_output_frames: u64,
    ratio: f32,
    target: f32,
    frames_until_step: usize,
}

impl SourcePlayback {
    pub(crate) fn new(frame: usize, seek_mode: ExplicitSeekMode, ratio: f32) -> Self {
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

    pub(crate) fn configure(&mut self, sample_frames: usize, region: FrameRange) {
        let domain = (sample_frames, region);
        if self.domain == Some(domain) {
            return;
        }
        self.rebase();
        self.domain = Some(domain);
        let normalized = playhead_before_render(self.origin.frame, region, self.origin.seek_mode);
        if normalized != self.origin.frame {
            self.origin.frame = normalized;
            self.origin.fraction = 0.0;
        }
    }

    pub(crate) fn clear_explicit_seek(&mut self) {
        self.rebase();
        self.origin.seek_mode = ExplicitSeekMode::Normal;
        // The next configure must normalize against the newly accepted loop.
        self.domain = None;
    }

    pub(crate) fn set_target(&mut self, target: f32) {
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
    pub(crate) fn chunk(&mut self, max_frames: usize) -> (usize, f32) {
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
                * f64::from(self.ratio);
        let whole = distance.floor() as usize;
        let (frame, seek_mode) = self.domain.map_or(
            (
                self.origin.frame.saturating_add(whole),
                self.origin.seek_mode,
            ),
            |(sample_frames, region)| {
                advance_playback_position(
                    self.origin.frame,
                    whole,
                    sample_frames,
                    region,
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

fn checked_ratio(ratio: f32) -> f32 {
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
    fn long_constant_epoch_uses_exact_count_instead_of_segment_rounding() {
        let ratio = 123.45_f32 / 97.3_f32;
        let mut playback = SourcePlayback::new(7, ExplicitSeekMode::Normal, ratio);
        playback.configure(101, FrameRange { start: 7, end: 101 });
        // Thirty minutes at 96 kHz, advanced with unrelated chunk sizes.
        let total = 96_000 * 60 * 30;
        for frames in [1, 31, 512, total - 544] {
            playback.advance(frames);
        }
        let distance = total as f64 * f64::from(ratio);
        let position = playback.position();
        assert_eq!(position.frame, 7 + distance.floor() as usize % 94);
        assert_eq!(position.fraction, distance.fract());
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
