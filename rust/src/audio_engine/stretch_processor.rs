use crate::messages::{
    KEY_LOCK_HEAD_COUNT_MIN, KeyLockInterpolation, KeyLockSettings, KeyLockWindow,
};

/// Default maximum block size handled by the per-voice DSP wrapper.
///
/// The CPAL stream currently requests 512 output frames. At the maximum supported tempo ratio of
/// 2.0x this needs 1024 source frames, so this bound covers the real-time callback path without
/// resizing.
pub const DEFAULT_BLOCK_SAMPLES: usize = 1024;

/// Fixed per-channel pitch-compensation delay line length.
const PITCH_DELAY_BUFFER_SAMPLES: usize = 2048;

const PITCH_FACTOR_EPSILON: f32 = 0.001;

pub struct StretchProcessor {
    channels: usize,
    input: Vec<Vec<f32>>,
    varispeed: Vec<Vec<f32>>,
    output: Vec<Vec<f32>>,
    pitch_delay: Vec<Vec<f32>>,
    pitch_write_pos: Vec<usize>,
    pitch_phase: Vec<f32>,
}

unsafe impl Send for StretchProcessor {}

impl StretchProcessor {
    pub fn new(channels: usize) -> Self {
        let input = (0..channels)
            .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
            .collect();
        let varispeed = (0..channels)
            .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
            .collect();
        let output = (0..channels)
            .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
            .collect();
        let pitch_delay = (0..channels)
            .map(|_| vec![0.0; PITCH_DELAY_BUFFER_SAMPLES])
            .collect();

        Self {
            channels,
            input,
            varispeed,
            output,
            pitch_delay,
            pitch_write_pos: vec![0; channels],
            pitch_phase: vec![0.0; channels],
        }
    }

    pub fn reset(&mut self) {
        for channel in &mut self.varispeed {
            channel.fill(0.0);
        }
        for channel in &mut self.output {
            channel.fill(0.0);
        }
        for delay in &mut self.pitch_delay {
            delay.fill(0.0);
        }
        self.pitch_write_pos.fill(0);
        self.pitch_phase.fill(0.0);
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
        settings: KeyLockSettings,
    ) {
        if self.channels == 0 {
            return;
        }

        let input_samples = input_samples.clamp(1, DEFAULT_BLOCK_SAMPLES);
        let output_samples = output_samples.min(DEFAULT_BLOCK_SAMPLES);
        let pitch_factor = pitch_compensation_factor(tempo_ratio);
        let settings = settings.sanitized();

        for channel in 0..self.channels {
            render_varispeed(
                &self.input[channel][..input_samples],
                &mut self.varispeed[channel][..output_samples],
            );

            if preserve_pitch && (pitch_factor - 1.0).abs() > PITCH_FACTOR_EPSILON {
                process_pitch_compensation_channel(
                    &self.varispeed[channel][..output_samples],
                    &mut self.output[channel][..output_samples],
                    pitch_factor,
                    &mut self.pitch_delay[channel],
                    &mut self.pitch_write_pos[channel],
                    &mut self.pitch_phase[channel],
                    settings,
                );
            } else {
                prime_pitch_delay_channel(
                    &self.varispeed[channel][..output_samples],
                    &mut self.pitch_delay[channel],
                    &mut self.pitch_write_pos[channel],
                );
                self.output[channel][..output_samples]
                    .copy_from_slice(&self.varispeed[channel][..output_samples]);
            }
        }
    }

    pub fn output_buffers(&self) -> &[Vec<f32>] {
        &self.output
    }

    #[cfg(test)]
    pub(crate) fn processing_capacity(&self) -> usize {
        self.input.first().map_or(0, Vec::len)
    }

    #[cfg(test)]
    pub(crate) fn pitch_delay_capacity(&self) -> usize {
        self.pitch_delay.first().map_or(0, Vec::len)
    }
}

fn pitch_compensation_factor(tempo_ratio: f32) -> f32 {
    if !tempo_ratio.is_finite() || tempo_ratio <= 0.0 {
        return 1.0;
    }

    (1.0 / tempo_ratio).clamp(0.5, 2.0)
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

fn prime_pitch_delay_channel(input: &[f32], delay: &mut [f32], write_pos: &mut usize) {
    if delay.is_empty() {
        return;
    }

    for sample in input {
        delay[*write_pos] = *sample;
        *write_pos = (*write_pos + 1) % delay.len();
    }
}

fn process_pitch_compensation_channel(
    input: &[f32],
    output: &mut [f32],
    pitch_factor: f32,
    delay: &mut [f32],
    write_pos: &mut usize,
    phase: &mut f32,
    settings: KeyLockSettings,
) {
    if delay.is_empty() {
        output.fill(0.0);
        return;
    }

    let pitch_factor = pitch_factor.clamp(0.5, 2.0);
    let phase_step = (1.0 - pitch_factor) / settings.delay_range_samples;
    let head_count = usize::from(settings.head_count.max(KEY_LOCK_HEAD_COUNT_MIN));
    let head_count_f32 = head_count as f32;

    for (sample, out) in input.iter().zip(output.iter_mut()) {
        delay[*write_pos] = *sample;

        let base_phase = phase.rem_euclid(1.0);
        let mut shifted_sum = 0.0;
        let mut weight_sum = 0.0;
        for head in 0..head_count {
            let head_phase = (base_phase + head as f32 / head_count_f32).rem_euclid(1.0);
            let delay_samples =
                settings.delay_min_samples + head_phase * settings.delay_range_samples;
            let weight = window_weight(head_phase, settings.window);
            let shifted = read_delay_line(delay, *write_pos, delay_samples, settings.interpolation);
            shifted_sum += shifted * weight;
            weight_sum += weight;
        }
        *out = (shifted_sum / weight_sum.max(f32::EPSILON)) * settings.output_gain;

        *phase = (*phase + phase_step).rem_euclid(1.0);
        *write_pos = (*write_pos + 1) % delay.len();
    }
}

fn window_weight(phase: f32, window: KeyLockWindow) -> f32 {
    match window {
        KeyLockWindow::Triangle => (1.0 - (phase * 2.0 - 1.0).abs()).max(0.0),
        KeyLockWindow::Hann => 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos(),
    }
}

fn read_delay_line(
    delay: &[f32],
    write_pos: usize,
    delay_samples: f32,
    interpolation: KeyLockInterpolation,
) -> f32 {
    if delay.is_empty() {
        return 0.0;
    }

    let read_pos = wrap_delay_read_pos(write_pos as f32 - delay_samples, delay.len());
    let index = (read_pos.floor() as usize).min(delay.len() - 1);
    let next = (index + 1) % delay.len();
    let frac = (read_pos - index as f32).clamp(0.0, 1.0);

    match interpolation {
        KeyLockInterpolation::Linear => delay[index] + (delay[next] - delay[index]) * frac,
        KeyLockInterpolation::Cubic => {
            let prev = if index == 0 {
                delay.len() - 1
            } else {
                index - 1
            };
            let next_2 = (index + 2) % delay.len();
            cubic_hermite(delay[prev], delay[index], delay[next], delay[next_2], frac)
        }
    }
}

fn cubic_hermite(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * y1)
        + (-y0 + y2) * t
        + (2.0 * y0 - 5.0 * y1 + 4.0 * y2 - y3) * t2
        + (-y0 + 3.0 * y1 - 3.0 * y2 + y3) * t3)
}

fn wrap_delay_read_pos(read_pos: f32, len: usize) -> f32 {
    if len == 0 || !read_pos.is_finite() {
        return 0.0;
    }

    let len_f32 = len as f32;
    let wrapped = read_pos.rem_euclid(len_f32);
    if !wrapped.is_finite() || wrapped >= len_f32 {
        0.0
    } else {
        wrapped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::KeyLockQuality;

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
        assert_eq!(processor.pitch_delay_capacity(), PITCH_DELAY_BUFFER_SAMPLES);
    }

    #[test]
    fn delay_read_position_wraps_endpoint_inside_buffer() {
        assert_eq!(wrap_delay_read_pos(2048.0, 2048), 0.0);
        assert_eq!(wrap_delay_read_pos(2048.25, 2048), 0.25);
        assert_eq!(wrap_delay_read_pos(-0.25, 2048), 2047.75);
    }

    #[test]
    fn key_lock_quality_configs_stay_inside_preallocated_delay_line() {
        for quality in [
            KeyLockQuality::Performance,
            KeyLockQuality::Balanced,
            KeyLockQuality::High,
            KeyLockQuality::VeryHigh,
        ] {
            let settings = KeyLockSettings::from_quality(quality).sanitized();
            assert!(settings.head_count >= 2);
            assert!(
                settings.delay_min_samples + settings.delay_range_samples
                    <= crate::messages::KEY_LOCK_DELAY_TOTAL_SAMPLES_MAX
            );
        }

        assert_eq!(
            KeyLockSettings::from_quality(KeyLockQuality::Performance).interpolation,
            KeyLockInterpolation::Linear
        );
        assert_eq!(
            KeyLockSettings::from_quality(KeyLockQuality::High).interpolation,
            KeyLockInterpolation::Cubic
        );
        assert_eq!(
            KeyLockSettings::from_quality(KeyLockQuality::VeryHigh).head_count,
            4
        );
    }

    #[test]
    fn neutral_key_lock_is_transparent() {
        let mut processor = StretchProcessor::new(1);
        let input = processor.input_buffers_mut(256);
        for (index, sample) in input[0].iter_mut().take(256).enumerate() {
            *sample = (index as f32 * 0.01).sin();
        }

        processor.process(
            256,
            256,
            1.0,
            true,
            KeyLockSettings::from_quality(KeyLockQuality::High),
        );
        let output = &processor.output_buffers()[0][..256];

        for (index, sample) in output.iter().enumerate() {
            let expected = (index as f32 * 0.01).sin();
            assert!((*sample - expected).abs() < 1.0e-6);
        }
    }

    #[test]
    fn pitch_compensation_reduces_varispeed_pitch_shift() {
        let sample_rate_hz = 48_000.0;
        let input_hz = 440.0;
        let mut processor = StretchProcessor::new(1);
        let mut varispeed = Vec::new();
        let mut locked = Vec::new();

        for chunk in 0..24 {
            let input = processor.input_buffers_mut(1024);
            for (index, sample) in input[0].iter_mut().take(1024).enumerate() {
                let absolute_index = chunk * 1024 + index;
                let phase =
                    absolute_index as f32 * input_hz * std::f32::consts::TAU / sample_rate_hz;
                *sample = phase.sin();
            }
            processor.process(
                1024,
                512,
                2.0,
                false,
                KeyLockSettings::from_quality(KeyLockQuality::High),
            );
            varispeed.extend_from_slice(&processor.output_buffers()[0][..512]);
        }

        processor.reset();
        for chunk in 0..24 {
            let input = processor.input_buffers_mut(1024);
            for (index, sample) in input[0].iter_mut().take(1024).enumerate() {
                let absolute_index = chunk * 1024 + index;
                let phase =
                    absolute_index as f32 * input_hz * std::f32::consts::TAU / sample_rate_hz;
                *sample = phase.sin();
            }
            processor.process(
                1024,
                512,
                2.0,
                true,
                KeyLockSettings::from_quality(KeyLockQuality::High),
            );
            locked.extend_from_slice(&processor.output_buffers()[0][..512]);
        }

        let skip = 4096;
        let varispeed_hz = estimate_frequency(&varispeed[skip..], sample_rate_hz);
        let locked_hz = estimate_frequency(&locked[skip..], sample_rate_hz);

        assert!(varispeed_hz > 800.0, "varispeed_hz={varispeed_hz}");
        assert!((360.0..560.0).contains(&locked_hz), "locked_hz={locked_hz}");
    }

    #[test]
    fn all_key_lock_quality_presets_render_finite_output() {
        for quality in [
            KeyLockQuality::Performance,
            KeyLockQuality::Balanced,
            KeyLockQuality::High,
            KeyLockQuality::VeryHigh,
        ] {
            let mut processor = StretchProcessor::new(2);
            for chunk in 0..8 {
                let input = processor.input_buffers_mut(1024);
                for (channel, channel_input) in input.iter_mut().enumerate().take(2) {
                    for (index, sample) in channel_input.iter_mut().take(1024).enumerate() {
                        let phase = (chunk * 1024 + index) as f32 * 0.031 + channel as f32;
                        *sample = phase.sin() * 0.5;
                    }
                }

                processor.process(
                    1024,
                    512,
                    1.75,
                    true,
                    KeyLockSettings::from_quality(quality),
                );

                for channel in processor.output_buffers().iter().take(2) {
                    assert!(channel[..512].iter().all(|sample| sample.is_finite()));
                }
            }
        }
    }

    #[test]
    fn custom_key_lock_settings_render_finite_output() {
        let settings = KeyLockSettings {
            delay_min_samples: 128.0,
            delay_range_samples: 1024.0,
            head_count: 1,
            interpolation: KeyLockInterpolation::Cubic,
            window: KeyLockWindow::Hann,
            smoothing_step: 0.03,
            output_gain: 1.25,
        };
        let mut processor = StretchProcessor::new(1);
        for chunk in 0..8 {
            let input = processor.input_buffers_mut(1024);
            for (index, sample) in input[0].iter_mut().take(1024).enumerate() {
                let phase = (chunk * 1024 + index) as f32 * 0.031;
                *sample = phase.sin() * 0.5;
            }

            processor.process(1024, 512, 1.5, true, settings);

            assert!(
                processor.output_buffers()[0][..512]
                    .iter()
                    .all(|sample| sample.is_finite())
            );
        }
    }
}
