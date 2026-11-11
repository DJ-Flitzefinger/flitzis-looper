//! Original model -> supplied shifts -> split -> model boundary orchestration.
//!
//! Offsets are supplied in original draw order, independently for every model.
//! Empty offsets mean shifts=0, not one zero-offset shift. No Python random
//! generator, device/model state, learned runtime, resampling or normalization
//! is implemented here. The caller establishes each model's valid/training
//! contract, source identity/order and actual reader/admission custody.

use super::{
    BLOCK, Chunk, Error, MAX_PAYLOAD_BYTES, RATE, Track, TrackWorkspace, checkpoint, extent,
    finite, zeros,
};

pub const MAX_SHIFT: usize = (RATE / 2) as usize;

/// One model's source-backed HT boundary and bag configuration. `valid_frames`
/// is original explicit segment length or independently established valid_length;
/// training padding remains separate. This fixed-boundary adapter does not
/// implement arbitrary models' dynamic valid_length methods.
pub struct Model<'a> {
    pub sources: &'a [&'a str],
    pub channels: usize,
    pub samplerate: u32,
    pub segment: usize,
    pub overlap: f64,
    pub transition_power: f32,
    pub valid_frames: usize,
    pub training_frames: usize,
    pub split: bool,
    pub offsets: &'a [usize],
    pub weights: &'a [f64],
}

/// Synchronous callback in exact model/shift/chunk order. Every training output
/// scalar must be written, including the right pad. No result is valid until
/// the complete enclosing run returns Ok, including its final cancel check.
pub type Predictor<'a> = dyn FnMut(usize, Chunk, &[f32], &mut [f32]) -> Result<(), Error> + 'a;

pub struct Workspace<'a> {
    models: &'a [Model<'a>],
    frames: usize,
    channels: usize,
    sources: usize,
    tracks: Vec<TrackWorkspace>,
    padded: Vec<f32>,
    shifted: Vec<f32>,
    estimate: Vec<f32>,
    totals: Vec<f64>,
    payload_bytes: usize,
}

struct Plan {
    channels: usize,
    sources: usize,
    shifted_frames: usize,
    padded: usize,
    shift_buffer: usize,
    output: usize,
    bytes: usize,
}

impl<'a> Workspace<'a> {
    pub fn new(
        frames: usize,
        models: &'a [Model<'a>],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, Error> {
        let Plan {
            channels,
            sources,
            shifted_frames,
            padded,
            shift_buffer,
            output,
            bytes,
        } = Self::plan(frames, models, cancelled)?;
        let mut totals = Vec::new();
        totals
            .try_reserve_exact(sources)
            .map_err(|_| Error::Allocation {
                phase: "demucs.apply.totals",
                requested_bytes: sources * size_of::<f64>(),
            })?;
        totals.resize(sources, 0.0_f64);
        for m in models {
            for (dst, src) in totals.chunks_mut(BLOCK).zip(m.weights.chunks(BLOCK)) {
                checkpoint(cancelled)?;
                for (total, weight) in dst.iter_mut().zip(src) {
                    *total += weight;
                }
            }
        }
        if totals
            .iter()
            .any(|&v| !v.is_finite() || !(v as f32).is_finite() || v as f32 == 0.0)
        {
            return Err(Error::NonFinite);
        }
        let mut tracks = Vec::new();
        tracks
            .try_reserve_exact(models.len())
            .map_err(|_| Error::Allocation {
                phase: "demucs.apply.models",
                requested_bytes: models.len() * size_of::<TrackWorkspace>(),
            })?;
        for m in models {
            let n = if m.offsets.is_empty() {
                frames
            } else {
                shifted_frames
            };
            tracks.push(TrackWorkspace::new(
                Track::new(
                    n,
                    channels,
                    sources,
                    m.segment,
                    m.overlap,
                    m.transition_power,
                    m.valid_frames,
                    m.training_frames,
                )?,
                cancelled,
            )?);
        }
        let work = Self {
            models,
            frames,
            channels,
            sources,
            tracks,
            padded: zeros(padded, "demucs.apply.padded")?,
            shifted: zeros(shift_buffer, "demucs.apply.shifted")?,
            estimate: zeros(output, "demucs.apply.estimate")?,
            totals,
            payload_bytes: bytes,
        };
        checkpoint(cancelled)?;
        Ok(work)
    }

    /// Validate and count all coexisting apply buffers without allocating.
    /// A composing caller adds its other live owners before constructing either.
    pub fn planned_payload_bytes(
        frames: usize,
        models: &[Model<'_>],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<usize, Error> {
        Ok(Self::plan(frames, models, cancelled)?.bytes)
    }

    fn plan(
        frames: usize,
        models: &[Model<'_>],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Plan, Error> {
        checkpoint(cancelled)?;
        let first = models.first().ok_or(Error::Geometry)?;
        let channels = first.channels;
        let sources = first.sources.len();
        let shifted_frames = frames.checked_add(MAX_SHIFT).ok_or(Error::Geometry)?;
        let padded_frames = shifted_frames
            .checked_add(MAX_SHIFT)
            .ok_or(Error::Geometry)?;
        let input = frames.checked_mul(channels).ok_or(Error::Geometry)?;
        let output = input.checked_mul(sources).ok_or(Error::Geometry)?;
        let shifted = shifted_frames
            .checked_mul(channels)
            .and_then(|v| v.checked_mul(sources))
            .ok_or(Error::Geometry)?;
        let has_shifts = models.iter().any(|m| !m.offsets.is_empty());
        let padded = if has_shifts {
            padded_frames.checked_mul(channels).ok_or(Error::Geometry)?
        } else {
            0
        };
        let shift_buffer = if has_shifts { shifted } else { output };
        let mut bytes = extent(&[input, output, padded, shift_buffer, output])?;
        bytes = bytes
            .checked_add(
                sources
                    .checked_mul(size_of::<f64>())
                    .ok_or(Error::Geometry)?,
            )
            .ok_or(Error::Geometry)?;
        // Validate/count every coexisting workspace BEFORE allocating any of it.
        for m in models {
            checkpoint(cancelled)?;
            if m.sources != first.sources
                || m.channels != channels
                || m.samplerate != RATE
                || m.weights.len() != sources
            {
                return Err(Error::Geometry);
            }
            let mut longest = frames;
            for block in m.offsets.chunks(BLOCK) {
                checkpoint(cancelled)?;
                for &offset in block {
                    if offset > MAX_SHIFT {
                        return Err(Error::Geometry);
                    }
                    longest = longest.max(shifted_frames - offset);
                }
            }
            for block in m.weights.chunks(BLOCK) {
                checkpoint(cancelled)?;
                if block
                    .iter()
                    .any(|v| !v.is_finite() || !(*v as f32).is_finite())
                {
                    return Err(Error::Geometry);
                }
            }
            let n = if m.offsets.is_empty() {
                frames
            } else {
                shifted_frames
            };
            let t = Track::new(
                n,
                channels,
                sources,
                m.segment,
                m.overlap,
                m.transition_power,
                m.valid_frames,
                m.training_frames,
            )?;
            if !m.split && longest > m.valid_frames {
                return Err(Error::Geometry);
            }
            // Track payload includes source/output owned above: count only its
            // actual envelope/triangle/training buffers, with no double count.
            bytes = bytes
                .checked_add(t.payload_bytes - (t.input_len() + t.output_len()) * size_of::<f32>())
                .ok_or(Error::Geometry)?;
            bytes = bytes
                .checked_add(size_of::<TrackWorkspace>())
                .ok_or(Error::Geometry)?;
            bytes = bytes
                .checked_add(std::mem::size_of_val(m.offsets))
                .and_then(|v| v.checked_add(std::mem::size_of_val(m.weights)))
                .ok_or(Error::Geometry)?;
        }
        if frames == 0 || bytes > MAX_PAYLOAD_BYTES {
            return Err(Error::Geometry);
        }
        // Validate source denominators before any allocation, including totals.
        for source in 0..sources {
            let mut total = 0.0_f64;
            for m in models {
                checkpoint(cancelled)?;
                total += m.weights[source];
            }
            if !total.is_finite() || !(total as f32).is_finite() || total as f32 == 0.0 {
                return Err(Error::NonFinite);
            }
        }
        Ok(Plan {
            channels,
            sources,
            shifted_frames,
            padded,
            shift_buffer,
            output,
            bytes,
        })
    }

    /// Declared scalar buffers, TrackWorkspace records, numeric offset/weight
    /// controls, borrowed source and caller output. Borrowed Model/source-name
    /// descriptors, learned weights,
    /// FFT plans, allocator, process/runtime and other external owners remain
    /// separate. This is a logical bound, NOT WHOLE/admission evidence.
    pub fn payload_bytes(&self) -> usize {
        self.payload_bytes
    }

    /// Runs without vector allocation. Caller retains immutable model controls,
    /// source readers, exclusive output and admission through all callbacks.
    /// On ANY error all output is invalid; a later run starts from clean sums.
    pub fn run(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        predictor: &mut Predictor<'_>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        let lanes = self.channels * self.sources;
        if input.len() != self.frames * self.channels || output.len() != self.frames * lanes {
            return Err(Error::Geometry);
        }
        finite(input, cancelled)?;
        clear(output, cancelled)?;
        if !self.padded.is_empty() {
            clear(&mut self.padded, cancelled)?;
            let padded_frames = self.frames + 2 * MAX_SHIFT;
            for channel in 0..self.channels {
                for (i, block) in input[channel * self.frames..(channel + 1) * self.frames]
                    .chunks(BLOCK)
                    .enumerate()
                {
                    checkpoint(cancelled)?;
                    let start = channel * padded_frames + MAX_SHIFT + i * BLOCK;
                    self.padded[start..start + block.len()].copy_from_slice(block);
                }
            }
        }
        for (index, (m, track)) in self.models.iter().zip(&mut self.tracks).enumerate() {
            checkpoint(cancelled)?;
            clear(&mut self.estimate, cancelled)?;
            let count = m.offsets.len().max(1);
            for shift in 0..count {
                let (backing, origin, frames, crop) = if m.offsets.is_empty() {
                    (input, 0, self.frames, 0)
                } else {
                    let offset = m.offsets[shift];
                    (
                        &self.padded[..],
                        offset,
                        self.frames + MAX_SHIFT - offset,
                        MAX_SHIFT - offset,
                    )
                };
                let target = &mut self.shifted[..frames * lanes];
                let mut model = |chunk, input: &[f32], output: &mut [f32]| {
                    predictor(index, chunk, input, output)
                };
                if m.split {
                    track.run_view(backing, origin, frames, target, &mut model, cancelled)?;
                } else {
                    no_split(
                        track, backing, origin, frames, target, &mut model, cancelled,
                    )?;
                }
                for lane in 0..lanes {
                    for start in (0..self.frames).step_by(BLOCK) {
                        checkpoint(cancelled)?;
                        for i in start..(start + BLOCK).min(self.frames) {
                            self.estimate[lane * self.frames + i] +=
                                target[lane * frames + crop + i];
                        }
                    }
                }
            }
            for source in 0..self.sources {
                let weight = m.weights[source] as f32;
                let start = source * self.channels * self.frames;
                for (src, dst) in self.estimate[start..start + self.channels * self.frames]
                    .chunks_mut(BLOCK)
                    .zip(output[start..start + self.channels * self.frames].chunks_mut(BLOCK))
                {
                    checkpoint(cancelled)?;
                    for (x, y) in src.iter_mut().zip(dst) {
                        *x /= count as f32;
                        *x *= weight;
                        *y += *x;
                        if !x.is_finite() || !y.is_finite() {
                            return Err(Error::NonFinite);
                        }
                    }
                }
            }
        }
        for source in 0..self.sources {
            let start = source * self.channels * self.frames;
            for block in output[start..start + self.channels * self.frames].chunks_mut(BLOCK) {
                checkpoint(cancelled)?;
                for x in block {
                    *x /= self.totals[source] as f32;
                    if !x.is_finite() {
                        return Err(Error::NonFinite);
                    }
                }
            }
        }
        checkpoint(cancelled)
    }
}

fn clear(values: &mut [f32], cancelled: &dyn Fn() -> bool) -> Result<(), Error> {
    for block in values.chunks_mut(BLOCK) {
        checkpoint(cancelled)?;
        block.fill(0.0);
    }
    Ok(())
}

fn no_split(
    track: &mut TrackWorkspace,
    source: &[f32],
    origin: usize,
    frames: usize,
    output: &mut [f32],
    model: &mut super::Predictor<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), Error> {
    let t = track.track;
    let chunk = Chunk {
        offset: origin,
        frames,
        valid_frames: t.valid_frames,
        training_frames: t.training_frames,
    };
    chunk.prepare_checked(
        source,
        source.len() / t.channels,
        t.channels,
        &mut track.input,
        cancelled,
    )?;
    for block in track.prediction.chunks_mut(BLOCK) {
        checkpoint(cancelled)?;
        block.fill(f32::NAN);
    }
    model(chunk, &track.input, &mut track.prediction)?;
    checkpoint(cancelled)?;
    finite(&track.prediction, cancelled)?;
    let trim = (t.valid_frames - frames) / 2;
    for (lane, dst) in output.chunks_mut(frames).enumerate() {
        for (i, block) in dst.chunks_mut(BLOCK).enumerate() {
            checkpoint(cancelled)?;
            let start = lane * t.training_frames + trim + i * BLOCK;
            block.copy_from_slice(&track.prediction[start..start + block.len()]);
        }
    }
    checkpoint(cancelled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn model<'a>(offsets: &'a [usize], weights: &'a [f64]) -> Model<'a> {
        Model {
            sources: &["a", "b"],
            channels: 1,
            samplerate: RATE,
            segment: 8193,
            overlap: 0.6,
            transition_power: 1.5,
            valid_frames: 8193,
            training_frames: 8197,
            split: true,
            offsets,
            weights,
        }
    }

    #[test]
    fn first_error_unwritten_nonfinite_final_cancel_and_fresh_reuse() {
        let models = [
            model(&[0, MAX_SHIFT], &[0.1, 2.0]),
            model(&[17, 1023], &[0.2, 0.0]),
        ];
        let mut w = Workspace::new(19, &models, &|| false).unwrap();
        let input = [0.25; 19];
        let mut output = [0.0; 38];
        let calls = Cell::new(0);
        assert_eq!(
            w.run(
                &input,
                &mut output,
                &mut |_, _, _, _| {
                    calls.set(calls.get() + 1);
                    Err(Error::Envelope)
                },
                &|| false
            ),
            Err(Error::Envelope)
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(
            w.run(&input, &mut output, &mut |_, _, _, _| Ok(()), &|| false),
            Err(Error::NonFinite)
        );
        let mut identity = |_: usize, chunk: Chunk, src: &[f32], dst: &mut [f32]| {
            for lane in dst.chunks_mut(chunk.training_frames) {
                lane.copy_from_slice(src);
            }
            Ok(())
        };
        w.run(&input, &mut output, &mut identity, &|| false)
            .unwrap();
        assert!(output.iter().all(|&v| (v - 0.25).abs() < 1e-6));
        let checks = Cell::new(0);
        w.run(&input, &mut output, &mut identity, &|| {
            checks.set(checks.get() + 1);
            false
        })
        .unwrap();
        let last = checks.get();
        checks.set(0);
        assert_eq!(
            w.run(&input, &mut output, &mut identity, &|| {
                checks.set(checks.get() + 1);
                checks.get() == last
            }),
            Err(Error::Cancelled)
        );
        assert_eq!(
            w.run(
                &input,
                &mut output,
                &mut |_, _, _, dst| {
                    dst.fill(f32::INFINITY);
                    Ok(())
                },
                &|| false
            ),
            Err(Error::NonFinite)
        );
        w.run(&input, &mut output, &mut identity, &|| false)
            .unwrap();
        assert!(output.iter().all(|&v| (v - 0.25).abs() < 1e-6));
    }

    #[test]
    fn rejects_incompatible_models_offsets_denominators_and_aggregate_extent() {
        assert!(Workspace::new(usize::MAX, &[model(&[], &[1.0, 1.0])], &|| false).is_err());
        assert!(Workspace::new(19, &[model(&[MAX_SHIFT + 1], &[1.0, 1.0])], &|| false).is_err());
        assert!(Workspace::new(19, &[model(&[], &[0.0, 1.0])], &|| false).is_err());
        let mut other = model(&[], &[1.0, 1.0]);
        other.sources = &["b", "a"];
        assert!(Workspace::new(19, &[model(&[], &[1.0, 1.0]), other], &|| false).is_err());
        let mut other = model(&[0], &[1.0, 1.0]);
        other.split = false;
        assert!(Workspace::new(19, &[other], &|| false).is_err());
        assert!(Workspace::new(20_000_000, &[model(&[0], &[1.0, 1.0])], &|| false).is_err());
        assert!(Workspace::new(19, &[model(&[], &[1.0, 1.0])], &|| true).is_err());
    }
}
