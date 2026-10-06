//! Non-realtime waveform projection from a bounded, frame-aligned PCM region.

use std::ops::Range;

pub(crate) struct WaveformData {
    pub(crate) is_raw: bool,
    pub(crate) xs: Vec<f64>,
    pub(crate) y_min: Vec<f32>,
    pub(crate) y_max: Option<Vec<f32>>,
}

/// Cover the requested source seconds with a clamped half-open frame interval.
pub(crate) fn source_frame_range(
    start_s: f64,
    end_s: f64,
    sample_rate_hz: f64,
    total_frames: usize,
) -> Range<usize> {
    let start = frame_bound(start_s, sample_rate_hz, false).min(total_frames);
    let end = frame_bound(end_s, sample_rate_hz, true)
        .min(total_frames)
        .max(start);
    start..end
}

fn frame_bound(seconds: f64, sample_rate_hz: f64, round_up: bool) -> usize {
    let frame = seconds * sample_rate_hz;
    let integer = frame.round();
    // A frame-derived second value can multiply back a few binary64 ULPs
    // above/below its integer. Recover only this arithmetic noise before the
    // directional covering round; genuine fractional positions stay covered.
    let tolerance = 2.0 * f64::EPSILON * frame.abs().max(1.0);
    let frame = if (frame - integer).abs() <= tolerance {
        integer
    } else {
        frame
    };
    if round_up {
        frame.ceil() as usize
    } else {
        frame.floor() as usize
    }
}

/// Render a selected PCM slice while retaining its absolute loaded-source origin.
///
/// The slice contains whole frames beginning at `start_frame`. All allocations
/// and sample scans belong to the control-side waveform request, never the callback.
pub(crate) fn render_region(
    samples: &[f32],
    channels: usize,
    sample_rate_hz: f64,
    start_frame: usize,
    width_px: usize,
) -> Option<WaveformData> {
    if channels == 0 || width_px == 0 || !sample_rate_hz.is_finite() || sample_rate_hz <= 0.0 {
        return None;
    }
    let frame_count = samples.len() / channels;
    if frame_count == 0 {
        return None;
    }

    let mono = |frame: usize| {
        let index = frame * channels;
        if channels == 2 {
            (samples[index] + samples[index + 1]) * 0.5
        } else {
            samples[index]
        }
    };
    if frame_count < width_px.saturating_mul(2) {
        let xs = (0..frame_count)
            .map(|frame| (start_frame + frame) as f64 / sample_rate_hz)
            .collect();
        let y_min = (0..frame_count).map(mono).collect();
        return Some(WaveformData {
            is_raw: true,
            xs,
            y_min,
            y_max: None,
        });
    }

    let mut xs = Vec::with_capacity(width_px);
    let mut y_min = Vec::with_capacity(width_px);
    let mut y_max = Vec::with_capacity(width_px);
    for bucket in 0..width_px {
        // Integer division assigns every source frame once, without f32
        // bucket arithmetic dropping a long region's tail or moving its X.
        let start = (bucket as u128 * frame_count as u128 / width_px as u128) as usize;
        let end = ((bucket + 1) as u128 * frame_count as u128 / width_px as u128) as usize;
        let mut minimum = f32::MAX;
        let mut maximum = f32::MIN;
        for frame in start..end {
            let value = mono(frame);
            minimum = minimum.min(value);
            maximum = maximum.max(value);
        }
        if minimum == f32::MAX {
            minimum = 0.0;
            maximum = 0.0;
        }
        xs.push((start_frame + start) as f64 / sample_rate_hz);
        y_min.push(minimum);
        y_max.push(maximum);
    }
    Some(WaveformData {
        is_raw: false,
        xs,
        y_min,
        y_max: Some(y_max),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_frame_queries_recover_exact_long_addresses_at_all_loaded_rates() {
        for rate in [44_100_u32, 48_000, 96_000] {
            let rate_f64 = f64::from(rate);
            for seconds in [599.5, 600.0, 1_800.0] {
                let first = (seconds * rate_f64) as usize;
                for frame in first..first + 16 {
                    assert_eq!(
                        source_frame_range(
                            frame as f64 / rate_f64,
                            (frame + 1) as f64 / rate_f64,
                            rate_f64,
                            first + 32,
                        ),
                        frame..frame + 1,
                        "rate={rate} frame={frame}"
                    );
                }
                assert_eq!(
                    source_frame_range(
                        (first as f64 + 0.25) / rate_f64,
                        (first as f64 + 0.75) / rate_f64,
                        rate_f64,
                        first + 32,
                    ),
                    first..first + 1,
                );
            }
        }
    }

    #[test]
    fn sparse_long_regions_keep_sixteen_distinct_frame_x_values() {
        let samples: Vec<_> = (0..16).map(|frame| frame as f32 / 16.0).collect();
        for rate in [44_100_u32, 48_000, 96_000] {
            let rate_f64 = f64::from(rate);
            for seconds in [599.5, 600.0, 1_800.0] {
                let first = (seconds * rate_f64) as usize;
                let data = render_region(&samples, 1, rate_f64, first, 16).unwrap();
                assert!(data.is_raw);
                assert_eq!(data.y_min, samples);
                assert!(data.y_max.is_none());
                assert_eq!(data.xs.len(), 16);
                assert!(data.xs.windows(2).all(|pair| pair[0] < pair[1]));
                for (offset, seconds) in data.xs.iter().enumerate() {
                    assert_eq!((seconds * rate_f64).round() as usize, first + offset);
                }
                let old_xs: Vec<_> = (first..first + 16)
                    .map(|frame| frame as f32 / rate as f32)
                    .collect();
                assert!(old_xs.windows(2).any(|pair| pair[0] == pair[1]));
            }
        }
    }

    #[test]
    fn envelope_uses_complete_integer_buckets_and_absolute_frame_x() {
        let mut samples = vec![0.25_f32; 127];
        samples[0] = -0.75;
        samples[126] = 0.875;
        let first = 172_800_003;
        let data = render_region(&samples, 1, 96_000.0, first, 5).unwrap();
        assert!(!data.is_raw);
        assert_eq!(data.y_min, [-0.75, 0.25, 0.25, 0.25, 0.25]);
        assert_eq!(data.y_max.unwrap(), [0.25, 0.25, 0.25, 0.25, 0.875]);
        let addressed_frames: Vec<_> = data
            .xs
            .iter()
            .map(|x| (x * 96_000.0).round() as usize)
            .collect();
        assert_eq!(
            addressed_frames,
            [first, first + 25, first + 50, first + 76, first + 101]
        );
    }

    #[test]
    fn waveform_bounds_keep_fractional_covering_and_signed_clamp_semantics() {
        assert_eq!(source_frame_range(-2.0, 0.025, 100.0, 10), 0..3);
        assert_eq!(source_frame_range(0.0125, 0.0275, 100.0, 10), 1..3);
        assert_eq!(source_frame_range(0.09, 20.0, 100.0, 10), 9..10);
        assert_eq!(source_frame_range(20.0, 30.0, 100.0, 10), 10..10);
        assert_eq!(source_frame_range(0.09, 0.02, 100.0, 10), 9..9);
        assert!(render_region(&[0.5], 1, 48_000.0, 0, 0).is_none());
        assert!(render_region(&[], 1, 48_000.0, 0, 16).is_none());
    }

    #[test]
    fn mono_projection_preserves_existing_stereo_average_in_both_modes() {
        let samples = [1.0, -1.0, 0.5, 0.25];
        let raw = render_region(&samples, 2, 48_000.0, 0, 16).unwrap();
        assert!(raw.is_raw);
        assert_eq!(raw.y_min, [0.0, 0.375]);
        let envelope = render_region(&samples, 2, 48_000.0, 0, 1).unwrap();
        assert!(!envelope.is_raw);
        assert_eq!(envelope.y_min, [0.0]);
        assert_eq!(envelope.y_max.unwrap(), [0.375]);
    }
}
