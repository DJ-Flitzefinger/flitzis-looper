use crate::audio_engine::buffer_retirement::AudioBufferRetirement;
use crate::audio_engine::constants::{SPEED_MAX, SPEED_MIN};
use crate::audio_engine::stretch_processor::StretchProcessor;
use crate::messages::KeyLockSettings;
use crate::messages::SampleBuffer;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExplicitSeekMode {
    Normal,
    BeforeLoop,
    AfterLoop,
}

pub struct VoiceSlot {
    pub active: bool,
    pub sample_id: usize,
    pub sample: Option<SampleBuffer>,
    pub frame_pos: usize,
    pub volume: f32,
    tempo_ratio_smoothed: f32,
    pub stretch: StretchProcessor,
    pub paused: bool,
    pub(crate) explicit_seek_mode: ExplicitSeekMode,
}

impl VoiceSlot {
    pub fn with_sample_rate(channels: usize, sample_rate_hz: f32) -> Self {
        Self {
            active: false,
            sample_id: 0,
            sample: None,
            frame_pos: 0,
            volume: 0.0,
            tempo_ratio_smoothed: 1.0,
            stretch: StretchProcessor::with_sample_rate(channels, sample_rate_hz),
            paused: false,
            explicit_seek_mode: ExplicitSeekMode::Normal,
        }
    }

    pub(crate) fn start_rt(
        &mut self,
        sample_id: usize,
        sample: SampleBuffer,
        initial_frame_pos: usize,
        volume: f32,
        initial_tempo_ratio: f32,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        if let Some(old_sample) = self.sample.take() {
            retirement.retire_sample(old_sample);
        }

        self.start_inner(
            sample_id,
            sample,
            initial_frame_pos,
            volume,
            initial_tempo_ratio,
        );
    }

    fn start_inner(
        &mut self,
        sample_id: usize,
        sample: SampleBuffer,
        initial_frame_pos: usize,
        volume: f32,
        initial_tempo_ratio: f32,
    ) {
        self.active = true;
        self.sample_id = sample_id;
        self.sample = Some(sample);
        self.frame_pos = initial_frame_pos;
        self.volume = volume;
        self.tempo_ratio_smoothed = initial_tempo_ratio;
        self.paused = false;
        self.explicit_seek_mode = ExplicitSeekMode::Normal;
        self.stretch.reset();
    }

    #[cfg(test)]
    pub(crate) fn stop(&mut self) {
        self.sample = None;
        self.stop_inner();
    }

    pub(crate) fn stop_rt(&mut self, retirement: &mut impl AudioBufferRetirement) {
        if let Some(sample) = self.sample.take() {
            retirement.retire_sample(sample);
        }

        self.stop_inner();
    }

    fn stop_inner(&mut self) {
        self.active = false;
        self.frame_pos = 0;
        self.volume = 0.0;
        self.tempo_ratio_smoothed = 1.0;
        self.paused = false;
        self.explicit_seek_mode = ExplicitSeekMode::Normal;
        self.stretch.reset();
    }

    pub fn restart(&mut self, initial_frame_pos: usize, volume: f32, initial_tempo_ratio: f32) {
        self.frame_pos = initial_frame_pos;
        self.volume = volume;
        self.tempo_ratio_smoothed = initial_tempo_ratio;
        self.paused = false;
        self.explicit_seek_mode = ExplicitSeekMode::Normal;
        self.stretch.reset();
    }

    pub(crate) fn seek(&mut self, frame_pos: usize, mode: ExplicitSeekMode) {
        self.frame_pos = frame_pos;
        self.explicit_seek_mode = mode;
        self.stretch.reset();
    }

    pub(crate) fn clear_explicit_seek(&mut self) {
        self.explicit_seek_mode = ExplicitSeekMode::Normal;
    }

    pub fn smooth_tempo_ratio(&mut self, target: f32, settings: KeyLockSettings) -> f32 {
        if !target.is_finite() {
            return self.tempo_ratio_smoothed;
        }

        let mut target = target.clamp(SPEED_MIN, SPEED_MAX);
        if !self.tempo_ratio_smoothed.is_finite() {
            self.tempo_ratio_smoothed = target;
            return self.tempo_ratio_smoothed;
        }

        let max_step = settings.sanitized().smoothing_step;
        let delta = (target - self.tempo_ratio_smoothed).clamp(-max_step, max_step);
        self.tempo_ratio_smoothed = (self.tempo_ratio_smoothed + delta).clamp(SPEED_MIN, SPEED_MAX);
        target = self.tempo_ratio_smoothed;

        target
    }

    pub fn is_playing_sample(&self, sample_id: usize) -> bool {
        self.active && self.sample_id == sample_id
    }

    /// Pause playback: set the paused flag. Does not change frame_pos.
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Resume playback: clear the paused flag.
    pub fn resume(&mut self) {
        self.paused = false;
    }
}
