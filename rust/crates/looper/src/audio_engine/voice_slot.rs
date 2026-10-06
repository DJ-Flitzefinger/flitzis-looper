use crate::audio_engine::buffer_retirement::AudioBufferRetirement;
use crate::audio_engine::key_lock_preparation::KeyLockPreparationLane;
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
}

pub struct VoiceSlot {
    pub active: bool,
    pub sample_id: usize,
    pub sample: Option<SampleBuffer>,
    pub frame_pos: usize,
    pub volume: f32,
    pub(crate) source_playback: SourcePlayback,
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
        if let Some(old_sample) = self.sample.take() {
            retirement.retire_sample(old_sample);
        }

        self.start_inner(
            config.sample_id,
            config.sample,
            config.initial_frame_pos,
            config.volume,
            config.initial_tempo_ratio,
            config.start_output_frame,
        );
    }

    fn start_inner(
        &mut self,
        sample_id: usize,
        sample: SampleBuffer,
        initial_frame_pos: usize,
        volume: f32,
        initial_tempo_ratio: f64,
        _start_output_frame: Option<u64>,
    ) {
        self.active = true;
        self.sample_id = sample_id;
        self.sample = Some(sample);
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
        self.source_playback = SourcePlayback::new(0, ExplicitSeekMode::Normal, 1.0);
        self.paused = false;
        self.explicit_seek_mode = ExplicitSeekMode::Normal;
        self.stretch.reset();
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
        self.explicit_seek_mode = ExplicitSeekMode::Normal;
        self.source_playback.clear_explicit_seek();
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
