use crate::audio_engine::buffer_retirement::AudioBufferRetirement;
use crate::audio_engine::constant_timing::AcceptedTimingProjection;
use crate::audio_engine::key_lock_preparation::KeyLockPreparationLane;
use crate::audio_engine::source_grid::SourceGrid;
use crate::audio_engine::source_playback::SourcePlayback;
pub(crate) use crate::audio_engine::source_reader::ExplicitSeekMode;
use crate::audio_engine::source_reader::{
    FrameRange, SourceLoopDomain, StemRenderSelection, StemTransition,
};
use crate::audio_engine::stretch_processor::StretchProcessor;
use crate::messages::{PreparedStemSet, SampleBuffer};

/// The effective source selection follows an old PCM pin across bank replacement.
/// The set uses existing immutable Arcs; capture does not allocate on the callback.
#[derive(Clone, Debug)]
pub(crate) struct FrozenStemView {
    pub(crate) set: Option<PreparedStemSet>,
    pub(crate) selection: StemRenderSelection,
    pub(crate) transition: StemTransition,
}

pub(crate) struct VoiceStartConfig {
    pub(crate) sample_id: usize,
    pub(crate) sample: SampleBuffer,
    pub(crate) initial_frame_pos: usize,
    pub(crate) volume: f32,
    pub(crate) initial_tempo_ratio: f64,
    pub(crate) start_output_frame: Option<u64>,
    pub(crate) source_timing: VoiceSourceTiming,
}

/// Effective timing belongs to the voice's pinned source, even after a bank replacement.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct VoiceSourceTiming {
    pub(crate) accepted: Option<AcceptedTimingProjection>,
    pub(crate) legacy_period_seconds: Option<f64>,
    pub(crate) legacy_origin_frame: f64,
}

impl VoiceSourceTiming {
    /// Only the effective accepted owner can admit musical repetition of physical PCM.
    /// Legacy/manual/tap projection remains useful for display and entry, not this period.
    pub(crate) fn loop_domain(self, sample_frames: usize, region: FrameRange) -> SourceLoopDomain {
        let musical_period = self.accepted.and_then(|timing| {
            SourceGrid::from_period(
                timing.period_seconds * f64::from(timing.sample_rate_hz),
                timing.origin_seconds * f64::from(timing.sample_rate_hz),
            )?
            .compatible_loop_period(region.start, region.end)
        });
        musical_period
            .and_then(|period| SourceLoopDomain::musical(sample_frames, region, period))
            .unwrap_or_else(|| SourceLoopDomain::physical(sample_frames, region))
    }

    pub(crate) fn period_seconds(self) -> Option<f64> {
        self.accepted
            .map(|timing| timing.period_seconds)
            .or(self.legacy_period_seconds)
    }

    pub(crate) fn grid(self, sample_rate_hz: f64) -> Option<SourceGrid> {
        if let Some(timing) = self.accepted {
            SourceGrid::from_period(
                timing.period_seconds * f64::from(timing.sample_rate_hz),
                timing.origin_seconds * f64::from(timing.sample_rate_hz),
            )
        } else {
            SourceGrid::from_period(
                self.legacy_period_seconds? * sample_rate_hz,
                self.legacy_origin_frame,
            )
        }
    }
}

pub struct VoiceSlot {
    pub active: bool,
    pub sample_id: usize,
    pub sample: Option<SampleBuffer>,
    pub frame_pos: usize,
    pub volume: f32,
    pub(crate) source_playback: SourcePlayback,
    pub(crate) source_timing: VoiceSourceTiming,
    /// Frozen only when this voice keeps an old source after bank replacement.
    pub(crate) source_loop_region: Option<FrameRange>,
    pub(crate) frozen_stems: Option<FrozenStemView>,
    pub(crate) generation: u64,
    pub stretch: StretchProcessor,
    pub paused: bool,
    pub(crate) explicit_seek_mode: ExplicitSeekMode,
}

impl VoiceSlot {
    pub(crate) fn with_preparation_lane(
        channels: usize,
        preparation: KeyLockPreparationLane,
    ) -> Self {
        Self {
            active: false,
            sample_id: 0,
            sample: None,
            frame_pos: 0,
            volume: 0.0,
            source_playback: SourcePlayback::new(0, ExplicitSeekMode::Normal, 1.0),
            source_timing: VoiceSourceTiming::default(),
            source_loop_region: None,
            frozen_stems: None,
            generation: 0,
            stretch: StretchProcessor::with_preparation_lane(channels, preparation),
            paused: false,
            explicit_seek_mode: ExplicitSeekMode::Normal,
        }
    }

    pub(crate) fn start_rt(
        &mut self,
        config: VoiceStartConfig,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        // The source-address history is valid only while this voice retains its PCM pin.
        self.stretch.reset();
        if let Some(old_sample) = self.sample.take() {
            retirement.retire_sample(old_sample);
        }
        self.retire_frozen_stems(retirement);

        self.start_inner(config);
    }

    fn start_inner(&mut self, config: VoiceStartConfig) {
        self.generation = self.generation.saturating_add(1);
        let VoiceStartConfig {
            sample_id,
            sample,
            initial_frame_pos,
            volume,
            initial_tempo_ratio,
            start_output_frame: _,
            source_timing,
        } = config;
        self.active = true;
        self.sample_id = sample_id;
        self.sample = Some(sample);
        self.source_timing = source_timing;
        self.source_loop_region = None;
        self.frame_pos = initial_frame_pos;
        self.volume = volume;
        self.source_playback = SourcePlayback::new(
            initial_frame_pos,
            ExplicitSeekMode::Normal,
            initial_tempo_ratio,
        );
        self.paused = false;
        self.explicit_seek_mode = ExplicitSeekMode::Normal;
    }

    #[cfg(test)]
    pub(crate) fn stop(&mut self) {
        self.stretch.reset();
        self.sample = None;
        self.frozen_stems = None;
        self.stop_inner();
    }

    pub(crate) fn stop_rt(&mut self, retirement: &mut impl AudioBufferRetirement) {
        self.stretch.reset();
        if let Some(sample) = self.sample.take() {
            retirement.retire_sample(sample);
        }
        self.retire_frozen_stems(retirement);

        self.stop_inner();
    }

    fn stop_inner(&mut self) {
        self.generation = self.generation.saturating_add(1);
        self.active = false;
        self.frame_pos = 0;
        self.volume = 0.0;
        self.source_playback = SourcePlayback::new(0, ExplicitSeekMode::Normal, 1.0);
        self.source_timing = VoiceSourceTiming::default();
        self.source_loop_region = None;
        self.paused = false;
        self.explicit_seek_mode = ExplicitSeekMode::Normal;
    }

    pub(crate) fn retire_frozen_stems(&mut self, retirement: &mut impl AudioBufferRetirement) {
        if let Some(view) = self.frozen_stems.take()
            && let Some(set) = view.set
        {
            retirement.retire_prepared_stems(set);
        }
    }

    pub(crate) fn restart(
        &mut self,
        initial_frame_pos: usize,
        volume: f32,
        initial_tempo_ratio: f64,
        _start_output_frame: Option<u64>,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        self.retire_frozen_stems(retirement);
        self.source_loop_region = None;
        self.generation = self.generation.saturating_add(1);
        self.frame_pos = initial_frame_pos;
        self.volume = volume;
        self.source_playback = SourcePlayback::new(
            initial_frame_pos,
            ExplicitSeekMode::Normal,
            initial_tempo_ratio,
        );
        self.paused = false;
        self.explicit_seek_mode = ExplicitSeekMode::Normal;
        self.stretch.reset();
    }

    pub(crate) fn seek(
        &mut self,
        frame_pos: usize,
        mode: ExplicitSeekMode,
        _output_frame: Option<u64>,
    ) {
        self.generation = self.generation.saturating_add(1);
        self.frame_pos = frame_pos;
        self.explicit_seek_mode = mode;
        // Seek is an explicit discontinuity: retain rate, discard only the old source phase.
        self.source_playback.seek(frame_pos, mode);
        self.stretch.reset();
    }

    pub(crate) fn clear_explicit_seek(&mut self) {
        self.stretch.invalidate_prepared();
        self.explicit_seek_mode = ExplicitSeekMode::Normal;
        self.source_playback.clear_explicit_seek();
    }

    pub fn is_playing_sample(&self, sample_id: usize) -> bool {
        self.active && self.sample_id == sample_id
    }

    /// Pause playback: set the paused flag. Does not change frame_pos.
    pub fn pause(&mut self) {
        self.stretch.invalidate_prepared();
        self.paused = true;
    }

    /// Resume playback: clear the paused flag.
    pub fn resume(&mut self) {
        self.stretch.invalidate_prepared();
        self.paused = false;
    }
}

#[cfg(test)]
mod loop_domain_tests {
    use super::*;

    #[test]
    fn only_effective_accepted_timing_admits_compatible_musical_duration() {
        let region = FrameRange {
            start: 19,
            end: 1519,
        };
        let accepted = AcceptedTimingProjection {
            revision: [7; 32],
            period_seconds: 1500.25 * 16.0 / 48_000.0,
            origin_seconds: -0.123,
            sample_rate_hz: 48_000,
            publication_epoch: 4,
        };
        let legacy = VoiceSourceTiming {
            legacy_period_seconds: Some(accepted.period_seconds),
            legacy_origin_frame: accepted.origin_seconds * 48_000.0,
            ..VoiceSourceTiming::default()
        };
        assert_eq!(legacy.loop_domain(2000, region).musical_period, None);
        let current = VoiceSourceTiming {
            accepted: Some(accepted),
            ..legacy
        };
        assert_eq!(
            current.loop_domain(2000, region).musical_period,
            Some(1500.25)
        );
        assert_eq!(
            current
                .loop_domain(
                    2000,
                    FrameRange {
                        end: 1517,
                        ..region
                    }
                )
                .musical_period,
            None
        );
        let another_origin = VoiceSourceTiming {
            accepted: Some(AcceptedTimingProjection {
                origin_seconds: 0.371,
                ..accepted
            }),
            ..current
        };
        assert_eq!(
            current.loop_domain(2000, region),
            another_origin.loop_domain(2000, region)
        );
    }
}
