//! Independent Demucs 4.0.1 host arithmetic, one batch item per call.
//!
//! This is an offline, synchronous foundation, not a productive separator.
//! No neural model, random generator, resampling or runtime is supplied.
//! `apply` adds supplied-offset shifts and weighted bags. Borrowed input/readers,
//! exclusive output and caller admission must
//! survive every callback until return. Any error invalidates ALL output.

use super::{BLOCK, Error, MAX_PAYLOAD_BYTES, RATE, checkpoint, finite, zeros};

pub mod apply;

/// Mathematical mean and correction-one standard deviation, rounded to f32
/// before elementwise arithmetic. The independent f64 two-pass reduction is
/// deterministic, NOT Torch's device-dependent vector/Welford reduction order.
#[derive(Debug, Clone, Copy)]
pub struct Statistics {
    mean: f32,
    std: f32,
}

impl Statistics {
    pub fn mean(self) -> f32 {
        self.mean
    }
    pub fn std(self) -> f32 {
        self.std
    }

    /// Use separately established original-dispatch statistics. This validates
    /// scalars, not their provenance; caller must preserve per-item axes and the
    /// exact original input owner. Independent measurement below is NOT a parity
    /// substitute: high-DC/tiny-variance normalization failed original controls.
    pub fn from_reference(mean: f32, std: f32) -> Result<Self, Error> {
        if !mean.is_finite() || !std.is_finite() || std < 0.0 {
            return Err(Error::NonFinite);
        }
        Ok(Self { mean, std })
    }

    pub fn measure_independent(
        values: &[f32],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, Error> {
        extent(&[values.len()])?;
        finite(values, cancelled)?;
        Self::reduce(values.len(), |i| Ok(values[i]), cancelled)
    }

    fn reduce(
        count: usize,
        sample: impl Fn(usize) -> Result<f32, Error>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, Error> {
        if count < 2 {
            return Err(Error::Geometry);
        }
        let mut sum = 0.0_f64;
        for i in 0..count {
            if i % BLOCK == 0 {
                checkpoint(cancelled)?;
            }
            sum += f64::from(sample(i)?);
        }
        let mean = sum / count as f64;
        let mut square = 0.0_f64;
        for i in 0..count {
            if i % BLOCK == 0 {
                checkpoint(cancelled)?;
            }
            let delta = f64::from(sample(i)?) - mean;
            square += delta * delta;
        }
        let stats = Self {
            mean: mean as f32,
            std: (square / (count - 1) as f64).sqrt() as f32,
        };
        if !stats.mean.is_finite() || !stats.std.is_finite() {
            return Err(Error::NonFinite);
        }
        checkpoint(cancelled)?;
        Ok(stats)
    }

    /// Whole CLI reference is `wav.mean(0)`, NOT flattened channel statistics.
    /// No epsilon is allowed here; constant/singleton reference is refused rather
    /// than silently changing the undefined original normalization.
    pub fn channel_reference_independent(
        pcm: &[f32],
        channels: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, Error> {
        extent(&[pcm.len()])?;
        if channels == 0 || !pcm.len().is_multiple_of(channels) {
            return Err(Error::Geometry);
        }
        finite(pcm, cancelled)?;
        let frames = pcm.len() / channels;
        let stats = Self::reduce(
            frames,
            |i| {
                let mut sum = 0.0_f32;
                for channel in 0..channels {
                    if channels > BLOCK && channel.is_multiple_of(BLOCK) {
                        checkpoint(cancelled)?;
                    }
                    sum += pcm[channel * frames + i];
                }
                Ok(sum / channels as f32)
            },
            cancelled,
        )?;
        if stats.std == 0.0 {
            return Err(Error::NonFinite);
        }
        Ok(stats)
    }

    /// Normalize one flat per-batch C/F/T or C/L branch. CLI uses epsilon=0;
    /// HT branches use epsilon=1e-5. Separate subtract/divide operations match the
    /// source tensor expressions; zero branch std is valid with epsilon.
    pub fn normalize_into(
        self,
        input: &[f32],
        output: &mut [f32],
        epsilon: f32,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        extent(&[input.len(), output.len()])?;
        if input.len() != output.len() || !epsilon.is_finite() || epsilon < 0.0 {
            return Err(Error::Geometry);
        }
        finite(input, cancelled)?;
        let denominator = epsilon + self.std;
        if !denominator.is_finite() || denominator <= 0.0 {
            return Err(Error::NonFinite);
        }
        for (src, dst) in input.chunks(BLOCK).zip(output.chunks_mut(BLOCK)) {
            checkpoint(cancelled)?;
            for (&x, y) in src.iter().zip(dst) {
                *y = (x - self.mean) / denominator;
            }
        }
        finite(output, cancelled)?;
        checkpoint(cancelled)
    }

    /// Broadcast saved per-item statistics across all predicted sources. Raw
    /// std, without the normalization epsilon, restores each neural branch or
    /// CLI output. The caller separately sums spectral and waveform branches.
    pub fn restore_in_place(
        self,
        output: &mut [f32],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        extent(&[output.len()])?;
        finite(output, cancelled)?;
        for block in output.chunks_mut(BLOCK) {
            checkpoint(cancelled)?;
            for x in block {
                *x = *x * self.std + self.mean;
            }
        }
        finite(output, cancelled)?;
        checkpoint(cancelled)
    }
}

fn extent(lengths: &[usize]) -> Result<usize, Error> {
    let scalars = lengths
        .iter()
        .try_fold(0_usize, |n, &v| n.checked_add(v))
        .ok_or(Error::Geometry)?;
    let bytes = scalars
        .checked_mul(size_of::<f32>())
        .ok_or(Error::Geometry)?;
    if bytes > MAX_PAYLOAD_BYTES {
        return Err(Error::Geometry);
    }
    Ok(bytes)
}

/// Read complete original context around a physical chunk, zero only beyond
/// the WHOLE source edges, then right-zero-pad from valid to training length.
/// These are distinct operations when an explicit shorter segment is used.
#[derive(Debug, Clone, Copy)]
pub struct Chunk {
    pub offset: usize,
    pub frames: usize,
    pub valid_frames: usize,
    pub training_frames: usize,
}

impl Chunk {
    pub fn prepare_into(
        self,
        source: &[f32],
        channels: usize,
        output: &mut [f32],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        if channels == 0
            || !source.len().is_multiple_of(channels)
            || self.frames == 0
            || self.valid_frames < self.frames
            || self.training_frames < self.valid_frames
        {
            return Err(Error::Geometry);
        }
        extent(&[source.len(), output.len()])?;
        let total = source.len() / channels;
        if self
            .offset
            .checked_add(self.frames)
            .is_none_or(|end| end > total)
            || output.len()
                != channels
                    .checked_mul(self.training_frames)
                    .ok_or(Error::Geometry)?
        {
            return Err(Error::Geometry);
        }
        finite(source, cancelled)?;
        self.prepare_checked(source, total, channels, output, cancelled)
    }

    fn prepare_checked(
        self,
        source: &[f32],
        total: usize,
        channels: usize,
        output: &mut [f32],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        let left = (self.valid_frames - self.frames) / 2;
        for channel in 0..channels {
            for (block_index, block) in output
                [channel * self.training_frames..(channel + 1) * self.training_frames]
                .chunks_mut(BLOCK)
                .enumerate()
            {
                checkpoint(cancelled)?;
                for (j, x) in block.iter_mut().enumerate() {
                    let i = block_index * BLOCK + j;
                    *x = if i < self.valid_frames {
                        self.offset
                            .checked_add(i)
                            .and_then(|p| p.checked_sub(left))
                            .filter(|&p| p < total)
                            .map_or(0.0, |p| source[channel * total + p])
                    } else {
                        0.0
                    };
                }
            }
        }
        checkpoint(cancelled)
    }
}

/// Exact physical split geometry for a single model and no shifts. Segment
/// samples are supplied after original `int(rate*segment)`; `segment_samples`
/// provides the Python-f64 multiplication path. `valid_frames` is training length
/// without an override, or explicit segment length with an override.
#[derive(Debug, Clone, Copy)]
pub struct Track {
    frames: usize,
    channels: usize,
    sources: usize,
    segment: usize,
    stride: usize,
    valid_frames: usize,
    training_frames: usize,
    power: f32,
    payload_bytes: usize,
}

impl Track {
    pub fn segment_samples(seconds: f64, rate: u32) -> Result<usize, Error> {
        let frames = seconds * f64::from(rate);
        if rate != RATE || !frames.is_finite() || frames < 1.0 || frames >= usize::MAX as f64 {
            return Err(Error::Geometry);
        }
        Ok(frames as usize)
    }

    // Keep the original independent physical and model extents explicit.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        frames: usize,
        channels: usize,
        sources: usize,
        segment: usize,
        overlap: f64,
        transition_power: f32,
        valid_frames: usize,
        training_frames: usize,
    ) -> Result<Self, Error> {
        if frames == 0
            || channels == 0
            || sources == 0
            || segment == 0
            || segment > valid_frames
            || valid_frames > training_frames
            || !overlap.is_finite()
            || !(0.0..1.0).contains(&overlap)
            || !transition_power.is_finite()
            || transition_power < 1.0
        {
            return Err(Error::Geometry);
        }
        let stride = ((1.0 - overlap) * segment as f64) as usize;
        if stride == 0 || stride > segment {
            return Err(Error::Geometry);
        }
        let input = frames.checked_mul(channels).ok_or(Error::Geometry)?;
        let output = input.checked_mul(sources).ok_or(Error::Geometry)?;
        let model_input = training_frames
            .checked_mul(channels)
            .ok_or(Error::Geometry)?;
        let model_output = model_input.checked_mul(sources).ok_or(Error::Geometry)?;
        let payload_bytes = extent(&[input, output, frames, segment, model_input, model_output])?;
        Ok(Self {
            frames,
            channels,
            sources,
            segment,
            stride,
            valid_frames,
            training_frames,
            power: transition_power,
            payload_bytes,
        })
    }
    pub fn frames(self) -> usize {
        self.frames
    }
    pub fn input_len(self) -> usize {
        self.frames * self.channels
    }
    pub fn output_len(self) -> usize {
        self.input_len() * self.sources
    }
    pub fn stride(self) -> usize {
        self.stride
    }
    pub fn chunks(self) -> usize {
        self.frames.div_ceil(self.stride)
    }
    /// Source, full output, envelope, triangle and model boundary buffers only.
    /// Excludes neural/FFT plans, allocator/runtime/native/process backing and
    /// all other caller owners. This is NOT admission or a WHOLE process bound.
    pub fn payload_bytes(self) -> usize {
        self.payload_bytes
    }
}

/// Reusable fixed host vectors. Construction is fallible; run allocates no
/// vectors. The synchronous model callback must write ALL [source,channel,
/// training_frame] values, respect its own admission and return its first error.
/// No callback can publish a valid result before the entire run succeeds.
pub type Predictor<'a> = dyn FnMut(Chunk, &[f32], &mut [f32]) -> Result<(), Error> + 'a;

pub struct TrackWorkspace {
    track: Track,
    input: Vec<f32>,
    prediction: Vec<f32>,
    weight: Vec<f32>,
    sum_weight: Vec<f32>,
}

impl TrackWorkspace {
    pub fn new(track: Track, cancelled: &dyn Fn() -> bool) -> Result<Self, Error> {
        checkpoint(cancelled)?;
        let mut weight = zeros(track.segment, "demucs.host.triangle")?;
        let maximum = track.segment.div_ceil(2) as f32;
        for (i, x) in weight.iter_mut().enumerate() {
            if i % BLOCK == 0 {
                checkpoint(cancelled)?;
            }
            *x = ((i + 1).min(track.segment - i) as f32 / maximum).powf(track.power);
        }
        finite(&weight, cancelled)?;
        let work = Self {
            track,
            input: zeros(track.channels * track.training_frames, "demucs.host.input")?,
            prediction: zeros(
                track.sources * track.channels * track.training_frames,
                "demucs.host.prediction",
            )?,
            weight,
            sum_weight: zeros(track.frames, "demucs.host.envelope")?,
        };
        checkpoint(cancelled)?;
        Ok(work)
    }

    pub fn run(
        &mut self,
        source: &[f32],
        output: &mut [f32],
        model: &mut Predictor<'_>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        let t = self.track;
        if source.len() != t.input_len() || output.len() != t.output_len() {
            return Err(Error::Geometry);
        }
        finite(source, cancelled)?;
        self.run_view(source, 0, t.frames, output, model, cancelled)
    }

    // Nested TensorChunk keeps the entire backing tensor, while its logical
    // extent and absolute origin change. Never slice away neighboring context.
    fn run_view(
        &mut self,
        source: &[f32],
        origin: usize,
        frames: usize,
        output: &mut [f32],
        model: &mut Predictor<'_>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        let t = self.track;
        let total = source.len() / t.channels;
        if frames == 0
            || frames > t.frames
            || origin.checked_add(frames).is_none_or(|v| v > total)
            || output.len() != frames * t.channels * t.sources
        {
            return Err(Error::Geometry);
        }
        for block in output.chunks_mut(BLOCK) {
            checkpoint(cancelled)?;
            block.fill(0.0);
        }
        for block in self.sum_weight.chunks_mut(BLOCK) {
            checkpoint(cancelled)?;
            block.fill(0.0);
        }
        for offset in (0..frames).step_by(t.stride) {
            checkpoint(cancelled)?;
            let chunk = Chunk {
                offset: origin + offset,
                frames: t.segment.min(frames - offset),
                valid_frames: t.valid_frames,
                training_frames: t.training_frames,
            };
            chunk.prepare_checked(source, total, t.channels, &mut self.input, cancelled)?;
            // A successful callback that leaves an unwritten scalar must fail.
            for block in self.prediction.chunks_mut(BLOCK) {
                checkpoint(cancelled)?;
                block.fill(f32::NAN);
            }
            model(chunk, &self.input, &mut self.prediction)?;
            checkpoint(cancelled)?;
            finite(&self.prediction, cancelled)?;
            // HT first crops the right training pad to valid_frames; center_trim
            // then removes floor(delta/2) left and the remaining odd sample right.
            let trim = (t.valid_frames - chunk.frames) / 2;
            for lane in 0..t.channels * t.sources {
                for start in (0..chunk.frames).step_by(BLOCK) {
                    checkpoint(cancelled)?;
                    for i in start..(start + BLOCK).min(chunk.frames) {
                        output[lane * frames + offset + i] +=
                            self.weight[i] * self.prediction[lane * t.training_frames + trim + i];
                    }
                }
            }
            for start in (0..chunk.frames).step_by(BLOCK) {
                checkpoint(cancelled)?;
                for i in start..(start + BLOCK).min(chunk.frames) {
                    self.sum_weight[offset + i] += self.weight[i];
                }
            }
        }
        for lane in output.chunks_mut(frames) {
            for (start, block) in lane.chunks_mut(BLOCK).enumerate() {
                checkpoint(cancelled)?;
                for (i, x) in block.iter_mut().enumerate() {
                    let weight = self.sum_weight[start * BLOCK + i];
                    if !weight.is_finite() || weight <= 0.0 {
                        return Err(Error::Envelope);
                    }
                    *x /= weight;
                    if !x.is_finite() {
                        return Err(Error::NonFinite);
                    }
                }
            }
        }
        checkpoint(cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn guards_callback_first_error_late_cancel_and_reuse() {
        assert!(Track::new(usize::MAX, 2, 4, 7, 0.25, 1.0, 7, 9).is_err());
        assert!(Track::new(3, 2, 4, 7, 1.0, 1.0, 7, 9).is_err());
        assert!(Track::new(3, 2, 4, 7, 0.9999, 1.0, 7, 9).is_err());
        let t = Track::new(19, 2, 2, 7, 0.6, 1.5, 7, 9).unwrap();
        let mut w = TrackWorkspace::new(t, &|| false).unwrap();
        let source = vec![0.25; t.input_len()];
        let mut out = vec![0.0; t.output_len()];
        let calls = Cell::new(0);
        let mut fail = |_: Chunk, _: &[f32], _: &mut [f32]| {
            calls.set(calls.get() + 1);
            Err(Error::Envelope)
        };
        assert_eq!(
            w.run(&source, &mut out, &mut fail, &|| false),
            Err(Error::Envelope)
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(
            w.run(&source, &mut out, &mut |_, _, _| Ok(()), &|| false),
            Err(Error::NonFinite)
        );
        let mut identity = |_: Chunk, input: &[f32], output: &mut [f32]| {
            for dst in output.chunks_mut(input.len()) {
                dst.copy_from_slice(input);
            }
            Ok(())
        };
        w.run(&source, &mut out, &mut identity, &|| false).unwrap();
        assert!(out.iter().all(|&x| (x - 0.25).abs() < 1e-6));
        let checkpoints = Cell::new(0);
        w.run(&source, &mut out, &mut identity, &|| {
            checkpoints.set(checkpoints.get() + 1);
            false
        })
        .unwrap();
        let last = checkpoints.get();
        checkpoints.set(0);
        assert_eq!(
            w.run(&source, &mut out, &mut identity, &|| {
                checkpoints.set(checkpoints.get() + 1);
                checkpoints.get() == last
            }),
            Err(Error::Cancelled)
        );
        w.run(&source, &mut out, &mut identity, &|| false).unwrap();
        assert!(Statistics::channel_reference_independent(&[0.0; 4], 2, &|| false).is_err());
        assert!(Statistics::measure_independent(&[1.0], &|| false).is_err());
        let simple = Statistics::measure_independent(&[1.0, 2.0, 3.0], &|| false).unwrap();
        assert_eq!((simple.mean(), simple.std()), (2.0, 1.0));
        assert!(Statistics::from_reference(0.0, -1.0).is_err());
        assert!(Statistics::from_reference(f32::NAN, 1.0).is_err());
        let s = Statistics::measure_independent(&[3.0; 4], &|| false).unwrap();
        s.normalize_into(&[3.0; 4], &mut out[..4], 1e-5, &|| false)
            .unwrap();
        assert!(out[..4].iter().all(|&x| x == 0.0));
    }
}
