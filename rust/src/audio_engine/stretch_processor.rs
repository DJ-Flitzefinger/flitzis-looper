use signalsmith_dsp::stretch::SignalsmithStretch;

/// Default block size passed to Signalsmith Stretch.
///
/// This trades off latency vs quality; chosen as a conservative starting point.
pub const DEFAULT_BLOCK_SAMPLES: usize = 1024;

/// Default interval (hop) size passed to Signalsmith Stretch.
pub const DEFAULT_INTERVAL_SAMPLES: usize = 256;

/// Default split computation setting.
///
/// When enabled, Signalsmith may spread heavy work to reduce worst-case spikes.
pub const DEFAULT_SPLIT_COMPUTATION: bool = true;

pub struct StretchProcessor {
    channels: usize,
    stretch: SignalsmithStretch<f32>,
    input: Vec<Vec<f32>>,
    output: Vec<Vec<f32>>,
}

unsafe impl Send for StretchProcessor {}

impl StretchProcessor {
    pub fn new(channels: usize) -> Self {
        let mut stretch = SignalsmithStretch::<f32>::new();
        stretch.configure(
            channels,
            DEFAULT_BLOCK_SAMPLES,
            DEFAULT_INTERVAL_SAMPLES,
            DEFAULT_SPLIT_COMPUTATION,
        );

        let input = (0..channels)
            .map(|_| Vec::with_capacity(DEFAULT_BLOCK_SAMPLES))
            .collect();
        let output = (0..channels)
            .map(|_| Vec::with_capacity(DEFAULT_BLOCK_SAMPLES))
            .collect();

        Self {
            channels,
            stretch,
            input,
            output,
        }
    }

    pub fn set_transpose_semitones(&mut self, semitones: f32) {
        if semitones.is_finite() {
            self.stretch.set_transpose_semitones(semitones, 0.0);
        }
    }

    pub fn input_buffers_mut(&mut self, input_samples: usize) -> &mut [Vec<f32>] {
        for channel in &mut self.input {
            ensure_len(channel, input_samples);
        }
        &mut self.input
    }

    pub fn process(&mut self, input_samples: usize, output_samples: usize) {
        if self.channels == 0 {
            return;
        }

        for channel in &mut self.output {
            ensure_len(channel, output_samples);
        }

        self.stretch
            .process(&self.input, input_samples, &mut self.output, output_samples);
    }

    pub fn output_buffers(&self) -> &[Vec<f32>] {
        &self.output
    }
}

fn ensure_len(buffer: &mut Vec<f32>, len: usize) {
    if buffer.len() != len {
        buffer.resize(len, 0.0);
    }
}
