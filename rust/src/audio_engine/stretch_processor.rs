use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;

/// Default maximum block size handled by the per-voice DSP wrapper.
///
/// The CPAL stream currently requests 512 output frames. At the maximum supported tempo ratio of
/// 2.0x this needs 1024 source frames, so this bound covers the real-time callback path without
/// resizing.
pub const DEFAULT_BLOCK_SAMPLES: usize = 1024;

const DEFAULT_SAMPLE_RATE_HZ: f32 = 48_000.0;
const RUBBERBAND_MIN_SAMPLE_RATE_HZ: f32 = 8_000.0;
const PITCH_SCALE_EPSILON: f64 = 0.001;

pub struct StretchProcessor {
    channels: usize,
    input: Vec<Vec<f32>>,
    varispeed: Vec<Vec<f32>>,
    output: Vec<Vec<f32>>,
    rubberband: Option<RubberBandLiveShifter>,
    rubberband_block_size: usize,
    rubberband_input: Vec<Vec<f32>>,
    rubberband_output: Vec<Vec<f32>>,
    rubberband_input_fifo: Vec<FixedFifo>,
    rubberband_output_fifo: Vec<FixedFifo>,
    rubberband_active: bool,
    rubberband_pitch_scale: f64,
}

unsafe impl Send for StretchProcessor {}

impl StretchProcessor {
    #[cfg(test)]
    pub fn new(channels: usize) -> Self {
        Self::with_sample_rate(channels, DEFAULT_SAMPLE_RATE_HZ)
    }

    pub fn with_sample_rate(channels: usize, sample_rate_hz: f32) -> Self {
        let input = (0..channels)
            .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
            .collect();
        let varispeed = (0..channels)
            .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
            .collect();
        let output = (0..channels)
            .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
            .collect();

        let rubberband = if channels == 0 {
            None
        } else {
            Some(
                RubberBandLiveShifter::new(sample_rate_to_u32(sample_rate_hz), channels)
                    .unwrap_or_else(|err| {
                        panic!("failed to initialize Rubber Band LiveShifter: {err}")
                    }),
            )
        };
        let rubberband_block_size = rubberband
            .as_ref()
            .map_or(DEFAULT_BLOCK_SAMPLES, RubberBandLiveShifter::block_size);
        let input_fifo_capacity = rubberband_block_size + DEFAULT_BLOCK_SAMPLES;
        let output_fifo_capacity = rubberband_block_size * 2 + DEFAULT_BLOCK_SAMPLES;

        Self {
            channels,
            input,
            varispeed,
            output,
            rubberband,
            rubberband_block_size,
            rubberband_input: (0..channels)
                .map(|_| vec![0.0; rubberband_block_size])
                .collect(),
            rubberband_output: (0..channels)
                .map(|_| vec![0.0; rubberband_block_size])
                .collect(),
            rubberband_input_fifo: (0..channels)
                .map(|_| FixedFifo::new(input_fifo_capacity))
                .collect(),
            rubberband_output_fifo: (0..channels)
                .map(|_| FixedFifo::new(output_fifo_capacity))
                .collect(),
            rubberband_active: false,
            rubberband_pitch_scale: 1.0,
        }
    }

    pub fn reset(&mut self) {
        for channel in &mut self.varispeed {
            channel.fill(0.0);
        }
        for channel in &mut self.output {
            channel.fill(0.0);
        }
        self.reset_rubberband_state();
    }

    pub fn input_buffers_mut(&mut self, input_samples: usize) -> &mut [Vec<f32>] {
        debug_assert!(input_samples <= DEFAULT_BLOCK_SAMPLES);
        &mut self.input
    }

    pub fn process(
        &mut self,
        input_samples: usize,
        output_samples: usize,
        tempo_ratio: f32,
        preserve_pitch: bool,
    ) {
        if self.channels == 0 {
            return;
        }

        let input_samples = input_samples.clamp(1, DEFAULT_BLOCK_SAMPLES);
        let output_samples = output_samples.min(DEFAULT_BLOCK_SAMPLES);
        let pitch_scale = rubberband_pitch_scale(tempo_ratio);

        for channel in 0..self.channels {
            render_varispeed(
                &self.input[channel][..input_samples],
                &mut self.varispeed[channel][..output_samples],
            );
        }

        if preserve_pitch
            && (pitch_scale - 1.0).abs() > PITCH_SCALE_EPSILON
            && self.rubberband.is_some()
        {
            self.process_rubberband(output_samples, pitch_scale);
        } else {
            self.deactivate_rubberband_if_needed();
            self.copy_varispeed_output(output_samples);
        }
    }

    pub fn output_buffers(&self) -> &[Vec<f32>] {
        &self.output
    }

    fn process_rubberband(&mut self, output_samples: usize, pitch_scale: f64) {
        if !self.rubberband_active {
            self.reset_rubberband_state();
            self.rubberband_active = true;
        }

        if (pitch_scale - self.rubberband_pitch_scale).abs() > PITCH_SCALE_EPSILON {
            let Some(rubberband) = self.rubberband.as_mut() else {
                self.copy_varispeed_output(output_samples);
                return;
            };
            if rubberband.set_pitch_scale(pitch_scale).is_err() {
                self.reset_rubberband_state();
                self.copy_varispeed_output(output_samples);
                return;
            }
            self.rubberband_pitch_scale = pitch_scale;
        }

        for channel in 0..self.channels {
            let written = self.rubberband_input_fifo[channel]
                .push_slice(&self.varispeed[channel][..output_samples]);
            if written != output_samples {
                self.reset_rubberband_state();
                self.copy_varispeed_output(output_samples);
                return;
            }
        }

        if !self.shift_available_rubberband_blocks() {
            self.reset_rubberband_state();
            self.copy_varispeed_output(output_samples);
            return;
        }

        for channel in 0..self.channels {
            let read = self.rubberband_output_fifo[channel]
                .pop_into(&mut self.output[channel][..output_samples]);
            if read < output_samples {
                self.output[channel][read..output_samples].fill(0.0);
            }
        }
    }

    fn shift_available_rubberband_blocks(&mut self) -> bool {
        if self.rubberband_block_size == 0 {
            return false;
        }

        let max_shift_blocks =
            (DEFAULT_BLOCK_SAMPLES / self.rubberband_block_size).saturating_add(2);
        let mut shifted_blocks = 0;

        while shifted_blocks < max_shift_blocks
            && self
                .rubberband_input_fifo
                .iter()
                .all(|fifo| fifo.len() >= self.rubberband_block_size)
        {
            for channel in 0..self.channels {
                let read = self.rubberband_input_fifo[channel]
                    .pop_into(&mut self.rubberband_input[channel][..self.rubberband_block_size]);
                if read != self.rubberband_block_size {
                    return false;
                }
            }

            let Some(rubberband) = self.rubberband.as_mut() else {
                return false;
            };
            if rubberband
                .shift(&self.rubberband_input, &mut self.rubberband_output)
                .is_err()
            {
                return false;
            }

            for channel in 0..self.channels {
                let written = self.rubberband_output_fifo[channel]
                    .push_slice(&self.rubberband_output[channel][..self.rubberband_block_size]);
                if written != self.rubberband_block_size {
                    return false;
                }
            }

            shifted_blocks += 1;
        }

        true
    }

    fn copy_varispeed_output(&mut self, output_samples: usize) {
        for channel in 0..self.channels {
            self.output[channel][..output_samples]
                .copy_from_slice(&self.varispeed[channel][..output_samples]);
        }
    }

    fn deactivate_rubberband_if_needed(&mut self) {
        if self.rubberband_active {
            self.reset_rubberband_state();
        }
    }

    fn reset_rubberband_state(&mut self) {
        if let Some(rubberband) = self.rubberband.as_mut() {
            rubberband.reset();
        }
        for channel in &mut self.rubberband_input {
            channel.fill(0.0);
        }
        for channel in &mut self.rubberband_output {
            channel.fill(0.0);
        }
        for fifo in &mut self.rubberband_input_fifo {
            fifo.reset();
        }
        for fifo in &mut self.rubberband_output_fifo {
            fifo.reset();
        }
        self.rubberband_active = false;
        self.rubberband_pitch_scale = 1.0;
    }

    #[cfg(test)]
    pub(crate) fn processing_capacity(&self) -> usize {
        self.input.first().map_or(0, Vec::len)
    }

    #[cfg(test)]
    pub(crate) fn rubberband_block_size(&self) -> usize {
        self.rubberband_block_size
    }

    #[cfg(test)]
    pub(crate) fn rubberband_start_delay(&self) -> usize {
        self.rubberband
            .as_ref()
            .map_or(0, RubberBandLiveShifter::start_delay)
    }

    #[cfg(test)]
    pub(crate) fn rubberband_input_fifo_capacity(&self) -> usize {
        self.rubberband_input_fifo
            .first()
            .map_or(0, FixedFifo::capacity)
    }

    #[cfg(test)]
    pub(crate) fn rubberband_output_fifo_capacity(&self) -> usize {
        self.rubberband_output_fifo
            .first()
            .map_or(0, FixedFifo::capacity)
    }
}

fn sample_rate_to_u32(sample_rate_hz: f32) -> u32 {
    if !sample_rate_hz.is_finite() || sample_rate_hz <= 0.0 {
        return DEFAULT_SAMPLE_RATE_HZ as u32;
    }

    sample_rate_hz
        .round()
        .clamp(RUBBERBAND_MIN_SAMPLE_RATE_HZ, u32::MAX as f32) as u32
}

fn rubberband_pitch_scale(tempo_ratio: f32) -> f64 {
    if !tempo_ratio.is_finite() || tempo_ratio <= 0.0 {
        return 1.0;
    }

    f64::from((1.0 / tempo_ratio).clamp(0.5, 2.0))
}

fn render_varispeed(input: &[f32], output: &mut [f32]) {
    if input.is_empty() || output.is_empty() {
        return;
    }

    if output.len() == 1 {
        output[0] = input[0];
        return;
    }

    if input.len() == 1 {
        output.fill(input[0]);
        return;
    }

    let scale = (input.len() - 1) as f32 / (output.len() - 1) as f32;
    for (index, sample) in output.iter_mut().enumerate() {
        let pos = index as f32 * scale;
        *sample = read_linear_slice(input, pos);
    }
}

fn read_linear_slice(input: &[f32], pos: f32) -> f32 {
    if input.is_empty() {
        return 0.0;
    }

    let pos = pos.clamp(0.0, (input.len() - 1) as f32);
    let index = pos.floor() as usize;
    let next = (index + 1).min(input.len() - 1);
    let frac = pos - index as f32;

    input[index] + (input[next] - input[index]) * frac
}

struct FixedFifo {
    buffer: Vec<f32>,
    read_pos: usize,
    write_pos: usize,
    len: usize,
}

impl FixedFifo {
    fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0.0; capacity.max(1)],
            read_pos: 0,
            write_pos: 0,
            len: 0,
        }
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.read_pos = 0;
        self.write_pos = 0;
        self.len = 0;
    }

    #[cfg(test)]
    fn capacity(&self) -> usize {
        self.buffer.len()
    }

    fn len(&self) -> usize {
        self.len
    }

    fn push_slice(&mut self, input: &[f32]) -> usize {
        let mut written = 0;
        for sample in input {
            if self.len == self.buffer.len() {
                break;
            }
            self.buffer[self.write_pos] = *sample;
            self.write_pos = (self.write_pos + 1) % self.buffer.len();
            self.len += 1;
            written += 1;
        }
        written
    }

    fn pop_into(&mut self, output: &mut [f32]) -> usize {
        let mut read = 0;
        for sample in output {
            if self.len == 0 {
                break;
            }
            *sample = self.buffer[self.read_pos];
            self.read_pos = (self.read_pos + 1) % self.buffer.len();
            self.len -= 1;
            read += 1;
        }
        read
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn estimate_frequency(samples: &[f32], sample_rate_hz: f32) -> f32 {
        let mut crossings = Vec::new();
        for index in 1..samples.len() {
            let prev = samples[index - 1];
            let current = samples[index];
            if prev <= 0.0 && current > 0.0 {
                let denom = current - prev;
                let frac = if denom.abs() > f32::EPSILON {
                    -prev / denom
                } else {
                    0.0
                };
                crossings.push(index as f32 - 1.0 + frac);
            }
        }

        if crossings.len() < 2 {
            return 0.0;
        }

        let span = crossings[crossings.len() - 1] - crossings[0];
        if span <= 0.0 {
            return 0.0;
        }

        (crossings.len() - 1) as f32 * sample_rate_hz / span
    }

    #[test]
    fn buffers_are_preallocated_for_callback_bounds() {
        let processor = StretchProcessor::new(2);

        assert_eq!(processor.processing_capacity(), DEFAULT_BLOCK_SAMPLES);
        assert!(processor.rubberband_block_size() > 0);
        assert!(processor.rubberband_start_delay() > 0);
        assert!(processor.rubberband_input_fifo_capacity() >= DEFAULT_BLOCK_SAMPLES);
        assert!(processor.rubberband_output_fifo_capacity() >= DEFAULT_BLOCK_SAMPLES);
    }

    #[test]
    fn pitch_scale_tracks_inverse_tempo_ratio() {
        assert!((rubberband_pitch_scale(2.0) - 0.5).abs() < f64::EPSILON);
        assert!((rubberband_pitch_scale(0.5) - 2.0).abs() < f64::EPSILON);
        assert!((rubberband_pitch_scale(1.0) - 1.0).abs() < f64::EPSILON);
        assert!((rubberband_pitch_scale(f32::NAN) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn neutral_key_lock_is_transparent() {
        let mut processor = StretchProcessor::new(1);
        let input = processor.input_buffers_mut(256);
        for (index, sample) in input[0].iter_mut().take(256).enumerate() {
            *sample = (index as f32 * 0.01).sin();
        }

        processor.process(256, 256, 1.0, true);
        let output = &processor.output_buffers()[0][..256];

        for (index, sample) in output.iter().enumerate() {
            let expected = (index as f32 * 0.01).sin();
            assert!((*sample - expected).abs() < 1.0e-6);
        }
    }

    #[test]
    fn key_lock_off_is_varispeed() {
        let mut processor = StretchProcessor::new(1);
        let input = processor.input_buffers_mut(1024);
        for (index, sample) in input[0].iter_mut().take(1024).enumerate() {
            *sample = index as f32;
        }

        processor.process(1024, 512, 2.0, false);

        assert_eq!(processor.output_buffers()[0][0], 0.0);
        assert!((processor.output_buffers()[0][511] - 1023.0).abs() < 1.0e-3);
    }

    #[test]
    fn unavailable_rubberband_output_uses_silence_fallback() {
        let mut processor = StretchProcessor::new(1);
        let output_samples = processor
            .rubberband_block_size()
            .saturating_sub(1)
            .clamp(1, DEFAULT_BLOCK_SAMPLES);
        let input_samples = (output_samples * 2).min(DEFAULT_BLOCK_SAMPLES);
        let input = processor.input_buffers_mut(input_samples);
        for sample in input[0].iter_mut().take(input_samples) {
            *sample = 0.5;
        }

        processor.process(input_samples, output_samples, 2.0, true);

        assert!(
            processor.output_buffers()[0][..output_samples]
                .iter()
                .all(|sample| *sample == 0.0)
        );
    }

    #[test]
    fn reset_clears_pending_rubberband_output() {
        let mut processor = StretchProcessor::new(1);
        let block_size = processor.rubberband_block_size().min(DEFAULT_BLOCK_SAMPLES);
        for chunk in 0..4 {
            let input = processor.input_buffers_mut(block_size);
            for (index, sample) in input[0].iter_mut().take(block_size).enumerate() {
                *sample = ((chunk * block_size + index) as f32 * 0.031).sin();
            }
            processor.process(block_size, block_size, 2.0, true);
        }

        processor.reset();

        let output_samples = block_size.saturating_sub(1).max(1);
        let input = processor.input_buffers_mut(output_samples);
        for sample in input[0].iter_mut().take(output_samples) {
            *sample = 0.5;
        }
        processor.process(output_samples, output_samples, 2.0, true);

        assert!(
            processor.output_buffers()[0][..output_samples]
                .iter()
                .all(|sample| *sample == 0.0)
        );
    }

    #[test]
    fn rubberband_key_lock_reduces_varispeed_pitch_shift() {
        let sample_rate_hz = 48_000.0;
        let input_hz = 440.0;
        let mut processor = StretchProcessor::new(1);
        let mut varispeed = Vec::new();
        let mut locked = Vec::new();

        for chunk in 0..48 {
            let input = processor.input_buffers_mut(1024);
            for (index, sample) in input[0].iter_mut().take(1024).enumerate() {
                let absolute_index = chunk * 1024 + index;
                let phase =
                    absolute_index as f32 * input_hz * std::f32::consts::TAU / sample_rate_hz;
                *sample = phase.sin();
            }
            processor.process(1024, 512, 2.0, false);
            varispeed.extend_from_slice(&processor.output_buffers()[0][..512]);
        }

        processor.reset();
        for chunk in 0..48 {
            let input = processor.input_buffers_mut(1024);
            for (index, sample) in input[0].iter_mut().take(1024).enumerate() {
                let absolute_index = chunk * 1024 + index;
                let phase =
                    absolute_index as f32 * input_hz * std::f32::consts::TAU / sample_rate_hz;
                *sample = phase.sin();
            }
            processor.process(1024, 512, 2.0, true);
            locked.extend_from_slice(&processor.output_buffers()[0][..512]);
        }

        let skip = processor.rubberband_start_delay() + processor.rubberband_block_size() * 2;
        let varispeed_hz = estimate_frequency(&varispeed[skip..], sample_rate_hz);
        let locked_hz = estimate_frequency(&locked[skip..], sample_rate_hz);

        assert!(varispeed_hz > 800.0, "varispeed_hz={varispeed_hz}");
        assert!((360.0..560.0).contains(&locked_hz), "locked_hz={locked_hz}");
    }

    #[test]
    fn rubberband_key_lock_renders_finite_output() {
        let mut processor = StretchProcessor::new(1);
        for chunk in 0..8 {
            let input = processor.input_buffers_mut(1024);
            for (index, sample) in input[0].iter_mut().take(1024).enumerate() {
                let phase = (chunk * 1024 + index) as f32 * 0.031;
                *sample = phase.sin() * 0.5;
            }

            processor.process(1024, 512, 1.5, true);

            assert!(
                processor.output_buffers()[0][..512]
                    .iter()
                    .all(|sample| sample.is_finite())
            );
        }
    }
}
