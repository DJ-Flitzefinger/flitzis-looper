//! Non-realtime waveform projection from a bounded, frame-aligned PCM region.

use super::AudioEngine;
use super::complete_context::CompleteSourceReader;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const MAX_WIDTH: usize = 16_384;
// Raw zoom produces fewer than 2*width points. Its xs/min/max preparation
// needs <32*width bytes; the retained xs/min plus ToPyArray copies need
// <48*width bytes. This conservative bound covers both executed stages.
const OUTPUT_PEAK_BYTES_PER_PIXEL: usize = 64;

fn output_peak_bytes(width: usize) -> Result<usize, String> {
    width
        .checked_mul(OUTPUT_PEAK_BYTES_PER_PIXEL)
        .ok_or_else(|| "waveform output budget overflow".into())
}

#[derive(Clone, PartialEq, Eq)]
struct Request {
    generation: u64,
    rate: u32,
    source: [u8; 32],
    first: usize,
    end: usize,
    width: usize,
}

#[derive(Default)]
struct Slot {
    request: Option<Request>,
    cancelled: Arc<AtomicBool>,
    data: Option<Arc<WaveformData>>,
    error: Option<String>,
    pending: bool,
}

/// One bounded projected view per pad; never caches complete playback PCM.
pub(super) struct WaveformRequests {
    slots: Vec<Arc<Mutex<Slot>>>,
}

impl Default for WaveformRequests {
    fn default() -> Self {
        Self {
            slots: (0..super::constants::NUM_SAMPLES)
                .map(|_| Arc::new(Mutex::new(Slot::default())))
                .collect(),
        }
    }
}

impl WaveformRequests {
    pub(super) fn cancel(&self, id: usize) -> Result<(), String> {
        let mut slot = self
            .slots
            .get(id)
            .ok_or("waveform id out of range")?
            .lock()
            .map_err(|_| "waveform state lock poisoned")?;
        slot.cancelled.store(true, Ordering::Release);
        *slot = Slot::default();
        Ok(())
    }
    pub(super) fn cancel_all(&self) {
        for slot in &self.slots {
            if let Ok(mut slot) = slot.lock() {
                slot.cancelled.store(true, Ordering::Release);
                *slot = Slot::default();
            }
        }
    }

    pub(super) fn status(&self, id: usize) -> Result<(String, Option<String>), String> {
        let slot = self
            .slots
            .get(id)
            .ok_or("waveform id out of range")?
            .lock()
            .map_err(|_| "waveform state lock poisoned")?;
        let status = if slot.error.is_some() {
            "error"
        } else if slot.pending {
            "pending"
        } else if slot.data.is_some() {
            "ready"
        } else {
            "idle"
        };
        Ok((status.into(), slot.error.clone()))
    }

    pub(super) fn request(
        &self,
        engine: &AudioEngine,
        id: usize,
        sample: &super::SampleBuffer,
        region: Range<usize>,
        width: usize,
    ) -> Result<Option<Arc<WaveformData>>, String> {
        if width == 0 || width > MAX_WIDTH {
            return Err("waveform width must be in 1..=16384".into());
        }
        let (generation, rate) = engine
            .loaded_source_generations
            .lock()
            .map_err(|_| "source generation lock poisoned")?[id];
        let rate = sample
            .residency
            .as_ref()
            .map_or(rate, |view| view.source.sample_rate_hz);
        if rate == 0 {
            return Err("waveform source rate unavailable".into());
        }
        let source = sample
            .residency
            .as_ref()
            .map_or([0; 32], |view| view.source.transform_sha256);
        let request = Request {
            generation,
            rate,
            source,
            first: region.start,
            end: region.end,
            width,
        };
        let state = self
            .slots
            .get(id)
            .ok_or("waveform id out of range")?
            .clone();
        let mut slot = state.lock().map_err(|_| "waveform state lock poisoned")?;
        if slot.request.as_ref() == Some(&request) {
            return Ok(slot.data.clone());
        }
        // Admission precedes preparation. A failed key is terminal until explicit
        // retry/navigation, rather than retrying a full queue every UI frame.
        let admitted = engine.cold_jobs.reserve().and_then(|reservation| {
            let reader = CompleteSourceReader::capture(engine, id, sample.clone())?;
            reader.admit_scan(super::cold_jobs::PCM_LIMIT_BYTES, output_peak_bytes(width)?)?;
            Ok((reservation, reader))
        });
        let (reservation, reader) = match admitted {
            Ok(value) => value,
            Err(error) => {
                slot.cancelled.store(true, Ordering::Release);
                *slot = Slot {
                    request: Some(request),
                    error: Some(error.clone()),
                    ..Slot::default()
                };
                return Err(error);
            }
        };
        slot.cancelled.store(true, Ordering::Release);
        let cancelled = Arc::new(AtomicBool::new(false));
        *slot = Slot {
            request: Some(request.clone()),
            cancelled: cancelled.clone(),
            data: None,
            error: None,
            pending: true,
        };
        drop(slot);
        let generations = engine.loaded_source_generations.clone();
        let shutdown = engine.cold_cancelled.clone();
        let cancelled_read = cancelled.clone();
        let job_request = request.clone();
        let job_state = state.clone();
        let submitted = engine.cold_jobs.submit(reservation, move || {
            let stale = || {
                cancelled_read.load(Ordering::Acquire)
                    || shutdown.load(Ordering::Acquire)
                    || generations
                        .lock()
                        .map_or(true, |values| values[id].0 != job_request.generation)
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                render_complete_region(
                    &reader,
                    job_request.first..job_request.end,
                    job_request.width,
                    job_request.rate,
                    &stale,
                )
            }))
            .unwrap_or_else(|_| Err("Waveform worker failed".into()));
            if stale() {
                return;
            }
            if let Ok(mut slot) = job_state.lock()
                && slot.request.as_ref() == Some(&job_request)
            {
                slot.pending = false;
                match result {
                    Ok(data) => slot.data = data.map(Arc::new),
                    Err(error) => slot.error = Some(error),
                }
            }
        });
        if let Err(error) = submitted {
            cancelled.store(true, Ordering::Release);
            let mut slot = state.lock().map_err(|_| "waveform state lock poisoned")?;
            if slot.request.as_ref() == Some(&request) {
                slot.pending = false;
                slot.error = Some(error.clone());
            }
            return Err(error);
        }
        Ok(None)
    }
}

/// Independent of resident storage: scan complete sealed content in fixed chunks.
pub(super) fn render_complete_region(
    reader: &CompleteSourceReader,
    region: Range<usize>,
    width: usize,
    rate: u32,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<WaveformData>, String> {
    if region.is_empty() || width == 0 || width > MAX_WIDTH {
        return Ok(None);
    }
    reader.admit_scan(super::cold_jobs::PCM_LIMIT_BYTES, output_peak_bytes(width)?)?;
    let channels = reader.reference.channels;
    if rate == 0 {
        return Err("waveform source rate unavailable".into());
    }
    let count = region.len();
    let raw = count < width * 2;
    let bins = if raw { count } else { width };
    let mut xs = Vec::with_capacity(bins);
    let mut minima = vec![f32::MAX; bins];
    let mut maxima = vec![f32::MIN; bins];
    for bin in 0..bins {
        let offset = if raw {
            bin
        } else {
            (bin as u128 * count as u128 / bins as u128) as usize
        };
        xs.push((region.start + offset) as f64 / f64::from(rate));
    }
    let mut bin = 0;
    reader
        .visit_region(region.clone(), cancelled, |first, samples| {
            for (offset, frame) in samples.chunks_exact(channels).enumerate() {
                let relative = first + offset - region.start;
                if raw {
                    bin = relative;
                } else {
                    while bin + 1 < bins
                        && relative >= ((bin + 1) as u128 * count as u128 / bins as u128) as usize
                    {
                        bin += 1;
                    }
                }
                let mono = if channels == 2 {
                    (frame[0] + frame[1]) * 0.5
                } else {
                    frame[0]
                };
                if !mono.is_finite() {
                    return Err(std::io::Error::other("non-finite waveform PCM"));
                }
                minima[bin] = minima[bin].min(mono);
                maxima[bin] = maxima[bin].max(mono);
            }
            Ok(())
        })
        .map_err(|error| error.to_string())?;
    Ok(Some(WaveformData {
        is_raw: raw,
        xs,
        y_min: minima,
        y_max: (!raw).then_some(maxima),
    }))
}

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
    fn raw_output_admission_covers_actual_numpy_overlap_at_maximum_zoom_width() {
        use numpy::ToPyArray;
        use pyo3::prelude::*;

        Python::initialize();
        let width = MAX_WIDTH;
        let frames = width * 2 - 1;
        let reader = CompleteSourceReader::new(
            crate::messages::SampleBuffer {
                channels: 1,
                samples: vec![0.25_f32; frames].into(),
                residency: None,
            },
            None,
        )
        .unwrap();
        let data = render_complete_region(&reader, 0..frames, width, 48_000, &|| false)
            .unwrap()
            .unwrap();
        assert!(data.is_raw && data.y_max.is_none());
        assert_eq!(data.xs.len(), frames);
        assert_eq!(data.y_min.len(), frames);
        let projected = std::mem::size_of_val(data.xs.as_slice())
            + std::mem::size_of_val(data.y_min.as_slice());
        let observed_output_peak = Python::attach(|py| {
            // The public API uses these same owned NumPy copies while the
            // ready slot still retains the immutable projected Rust vectors.
            let xs = data.xs.to_pyarray(py);
            let ys = data.y_min.to_pyarray(py);
            let numpy_bytes = xs.getattr("nbytes").unwrap().extract::<usize>().unwrap()
                + ys.getattr("nbytes").unwrap().extract::<usize>().unwrap();
            assert_eq!(numpy_bytes, projected);
            projected + numpy_bytes
        });
        assert!(observed_output_peak > width * 40);
        let output_budget = output_peak_bytes(width).unwrap();
        assert!(output_budget >= observed_output_peak);
        let held_and_scratch =
            reader.held_bytes().unwrap() + super::super::complete_context::READER_SCRATCH_BYTES;
        assert!(
            reader
                .admit_scan(held_and_scratch + observed_output_peak - 1, output_budget)
                .is_err()
        );
        assert_eq!(
            reader
                .admit_scan(held_and_scratch + output_budget, output_budget)
                .unwrap(),
            held_and_scratch + output_budget
        );
        assert!(output_peak_bytes(usize::MAX).is_err());
    }

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
