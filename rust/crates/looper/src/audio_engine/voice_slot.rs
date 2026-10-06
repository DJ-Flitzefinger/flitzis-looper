use crate::audio_engine::buffer_retirement::AudioBufferRetirement;
use crate::audio_engine::constant_timing::AcceptedTimingProjection;
use crate::audio_engine::key_lock_preparation::KeyLockPreparationLane;
use crate::audio_engine::source_grid::SourceGrid;
use crate::audio_engine::source_playback::SourcePlayback;
pub(crate) use crate::audio_engine::source_reader::ExplicitSeekMode;
use crate::audio_engine::stretch_processor::StretchProcessor;
use crate::messages::SampleBuffer;

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

        self.start_inner(config);
    }

    fn start_inner(&mut self, config: VoiceStartConfig) {
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
        self.stop_inner();
    }

    pub(crate) fn stop_rt(&mut self, retirement: &mut impl AudioBufferRetirement) {
        self.stretch.reset();
        if let Some(sample) = self.sample.take() {
            retirement.retire_sample(sample);
        }

        self.stop_inner();
    }

    fn stop_inner(&mut self) {
        self.active = false;
        self.frame_pos = 0;
        self.volume = 0.0;
        self.source_playback = SourcePlayback::new(0, ExplicitSeekMode::Normal, 1.0);
        self.source_timing = VoiceSourceTiming::default();
        self.paused = false;
        self.explicit_seek_mode = ExplicitSeekMode::Normal;
    }

    pub fn restart(
        &mut self,
        initial_frame_pos: usize,
        volume: f32,
        initial_tempo_ratio: f64,
        _start_output_frame: Option<u64>,
    ) {
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
