use crate::audio_engine::constants::{SPEED_MAX, SPEED_MIN};
use crate::audio_engine::eq3::Eq3State;
use crate::audio_engine::stretch_processor::StretchProcessor;
use crate::messages::SampleBuffer;

pub struct VoiceSlot {
    pub active: bool,
    pub sample_id: usize,
    pub sample: Option<SampleBuffer>,
    pub frame_pos: usize,
    pub volume: f32,
    tempo_ratio_smoothed: f32,
    pub stretch: StretchProcessor,
    pub eq_state: Vec<Eq3State>,
    pub paused: bool,
}

impl VoiceSlot {
    pub fn new(channels: usize) -> Self {
        Self {
            active: false,
            sample_id: 0,
            sample: None,
            frame_pos: 0,
            volume: 0.0,
            tempo_ratio_smoothed: 1.0,
            stretch: StretchProcessor::new(channels),
            eq_state: vec![Eq3State::default(); channels],
            paused: false,
        }
    }

    pub fn start(
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
        for state in &mut self.eq_state {
            state.reset();
        }
    }

    pub fn stop(&mut self) {
        self.active = false;
        self.sample = None;
        self.frame_pos = 0;
        self.volume = 0.0;
        self.tempo_ratio_smoothed = 1.0;
        self.paused = false;
        for state in &mut self.eq_state {
            state.reset();
        }
    }

    pub fn restart(&mut self, initial_frame_pos: usize, volume: f32, initial_tempo_ratio: f32) {
        self.frame_pos = initial_frame_pos;
        self.volume = volume;
        self.tempo_ratio_smoothed = initial_tempo_ratio;
        self.paused = false;
    }

    pub fn smooth_tempo_ratio(&mut self, target: f32) -> f32 {
        if !target.is_finite() {
            return self.tempo_ratio_smoothed;
        }

        let mut target = target.clamp(SPEED_MIN, SPEED_MAX);
        if !self.tempo_ratio_smoothed.is_finite() {
            self.tempo_ratio_smoothed = target;
            return self.tempo_ratio_smoothed;
        }

        let max_step = 0.05;
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
