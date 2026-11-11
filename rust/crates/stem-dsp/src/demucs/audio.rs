//! Offline planar channel conversion and Julius 0.2.7 rational resampling.
//!
//! Source: Demucs 4.0.1 `audio.convert_audio{,_channels}` and Julius
//! `ResampleFrac`/`core.sinc`. Independent f32 arithmetic, not a bitwise promise
//! about Torch's platform-specific reductions or trigonometric dispatch.
//! Positive rates through 768 kHz and nonempty finite planar `[batch,channel,time]`
//! inputs are supported when the checked logical payload fits 512 MiB. Very
//! large coprime ratios can exceed that bound. Empty/zero-channel/nonfinite input
//! is deliberately refused. This is not a decoder, FFmpeg SRC, learned model,
//! productive binding, whole-process budget or runtime admission certificate.
//!
//! Calls are synchronous: hold immutable source readers, output and admission
//! through actual return. Any error invalidates ALL output, including its written
//! prefix. Return is not process/tree/reader END. Constructors allocate fallibly;
//! execution allocates nothing and reusable scratch is overwritten on every call.

use super::{BLOCK, Error, MAX_PAYLOAD_BYTES, checkpoint, finite, zeros};

pub const MAX_RATE: u32 = 768_000;

fn product(a: usize, b: usize) -> Result<usize, Error> {
    a.checked_mul(b).ok_or(Error::Geometry)
}

fn payload(parts: &[usize]) -> Result<usize, Error> {
    let floats = parts
        .iter()
        .try_fold(0usize, |sum, &n| sum.checked_add(n))
        .ok_or(Error::Geometry)?;
    let bytes = product(floats, size_of::<f32>())?;
    if bytes > MAX_PAYLOAD_BYTES {
        return Err(Error::Geometry);
    }
    Ok(bytes)
}

/// All leading dimensions flattened to batches; channel and time stay distinct.
#[derive(Clone, Copy, Debug)]
pub struct Channels {
    frames: usize,
    batches: usize,
    source: usize,
    target: usize,
    input_len: usize,
    output_len: usize,
}

impl Channels {
    pub fn new(frames: usize, batches: usize, source: usize, target: usize) -> Result<Self, Error> {
        if frames == 0
            || batches == 0
            || source == 0
            || target == 0
            || (source > 1 && source < target)
        {
            return Err(Error::Geometry);
        }
        let input_len = product(product(batches, source)?, frames)?;
        let output_len = product(product(batches, target)?, frames)?;
        payload(&[input_len.max(output_len)])?;
        Ok(Self {
            frames,
            batches,
            source,
            target,
            input_len,
            output_len,
        })
    }
    pub fn input_len(self) -> usize {
        self.input_len
    }
    pub fn output_len(self) -> usize {
        self.output_len
    }
    /// Simultaneous caller buffers for a standalone channel conversion.
    pub fn payload_bytes(self) -> Result<usize, Error> {
        payload(&[self.input_len, self.output_len])
    }

    /// Copy/expand/front-select, or average ALL source channels for mono.
    /// Unlike the original view-returning API, output is caller-owned planar PCM.
    pub fn convert_into(
        self,
        input: &[f32],
        output: &mut [f32],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        if input.len() != self.input_len || output.len() != self.output_len {
            return Err(Error::Geometry);
        }
        self.payload_bytes()?;
        finite(input, cancelled)?;
        for batch in 0..self.batches {
            for channel in 0..self.target {
                let start = (batch * self.target + channel) * self.frames;
                let target = &mut output[start..start + self.frames];
                if self.target != 1 || self.source == 1 {
                    let start = (batch * self.source + if self.source == 1 { 0 } else { channel })
                        * self.frames;
                    copy_into(&input[start..start + self.frames], target, cancelled)?;
                    continue;
                }
                // A block of downmixed frames must bound source loads too.
                let mono_block = (BLOCK / self.source).max(1);
                for (block, destination) in target.chunks_mut(mono_block).enumerate() {
                    checkpoint(cancelled)?;
                    for (i, value) in destination.iter_mut().enumerate() {
                        let time = block * mono_block + i;
                        let mut sum = 0.0_f32;
                        for source in 0..self.source {
                            if source != 0 && source.is_multiple_of(BLOCK) {
                                checkpoint(cancelled)?;
                            }
                            sum += input[(batch * self.source + source) * self.frames + time];
                        }
                        *value = sum / self.source as f32;
                        if !value.is_finite() {
                            return Err(Error::NonFinite);
                        }
                    }
                }
            }
        }
        checkpoint(cancelled)
    }
}

fn copy_into(input: &[f32], output: &mut [f32], cancelled: &dyn Fn() -> bool) -> Result<(), Error> {
    for (source, target) in input.chunks(BLOCK).zip(output.chunks_mut(BLOCK)) {
        checkpoint(cancelled)?;
        target.copy_from_slice(source);
    }
    Ok(())
}

/// Original filter defaults and length flags. Equal rates bypass length flags,
/// just as the source's early return does. Filter parameters remain finite,
/// positive, and rolloff <= 1 in this native supported domain.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub zeros: u32,
    pub rolloff: f64,
    pub output_length: Option<usize>,
    pub full: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            zeros: 24,
            rolloff: 0.945,
            output_length: None,
            full: false,
        }
    }
}

fn validate(old_rate: u32, new_rate: u32, options: Options) -> Result<(), Error> {
    let valid_rates = (1..=MAX_RATE).contains(&old_rate) && (1..=MAX_RATE).contains(&new_rate);
    let valid_filter = options.zeros > 0 && 0.0 < options.rolloff && options.rolloff <= 1.0;
    if valid_rates && valid_filter {
        Ok(())
    } else {
        Err(Error::Geometry)
    }
}

/// Pure checked geometry, before any kernel or caller payload allocation.
#[derive(Clone, Copy, Debug)]
pub struct ResampleGeometry {
    frames: usize,
    lanes: usize,
    old: usize,
    new: usize,
    width: usize,
    taps: usize,
    kernel_len: usize,
    output_frames: usize,
    input_len: usize,
    output_len: usize,
    payload_bytes: usize,
    options: Options,
}
impl ResampleGeometry {
    pub fn new(
        frames: usize,
        lanes: usize,
        old_rate: u32,
        new_rate: u32,
        options: Options,
    ) -> Result<Self, Error> {
        validate(old_rate, new_rate, options)?;
        if frames == 0 || lanes == 0 {
            return Err(Error::Geometry);
        }
        let (mut a, mut b) = (old_rate, new_rate);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        let old = (old_rate / a) as usize;
        let new = (new_rate / a) as usize;
        let input_len = product(frames, lanes)?;
        let (width, taps, kernel_len, output_frames) = if old == new {
            (0, 0, 0, frames)
        } else {
            let width_f = (f64::from(options.zeros) * old as f64
                / (old.min(new) as f64 * options.rolloff))
                .ceil();
            if !width_f.is_finite() || width_f >= isize::MAX as f64 {
                return Err(Error::Geometry);
            }
            let width = width_f as usize;
            let taps = product(width, 2)?.checked_add(old).ok_or(Error::Geometry)?;
            let kernel_len = product(taps, new)?;
            // Python division is f64, torch.as_tensor then defaults to f32.
            let float_length = (product(new, frames)? as f64 / old as f64) as f32;
            if !float_length.is_finite() || float_length.ceil() >= usize::MAX as f32 {
                return Err(Error::Geometry);
            }
            let maximum = float_length.ceil() as usize;
            if options.output_length.is_some_and(|n| n > maximum)
                || (options.full && options.output_length.is_some())
            {
                return Err(Error::Geometry);
            }
            let selected = options.output_length.unwrap_or_else(|| {
                if options.full {
                    maximum
                } else {
                    float_length.floor() as usize
                }
            });
            let raw = product((frames / old).checked_add(1).ok_or(Error::Geometry)?, new)?;
            // Check the maximum padded index even though there is no padded allocation.
            frames
                .checked_add(width)
                .and_then(|n| n.checked_add(width))
                .and_then(|n| n.checked_add(old))
                .ok_or(Error::Geometry)?;
            // The source slices the actual conv tensor, clipping even rounded lengths.
            (width, taps, kernel_len, selected.min(raw))
        };
        let output_len = product(output_frames, lanes)?;
        let payload_bytes = payload(&[input_len, output_len, kernel_len])?;
        Ok(Self {
            frames,
            lanes,
            old,
            new,
            width,
            taps,
            kernel_len,
            output_frames,
            input_len,
            output_len,
            payload_bytes,
            options,
        })
    }
    pub fn input_len(self) -> usize {
        self.input_len
    }
    pub fn output_len(self) -> usize {
        self.output_len
    }
    pub fn output_frames(self) -> usize {
        self.output_frames
    }
    pub fn kernel_len(self) -> usize {
        self.kernel_len
    }
    pub fn payload_bytes(self) -> usize {
        self.payload_bytes
    }
    pub fn reduced_rates(self) -> (usize, usize) {
        (self.old, self.new)
    }
}

/// Fixed-geometry reusable f32 FIR. Replicate padding and phase interleaving are
/// addressed virtually; no padded PCM or complete convolution tensor is retained.
pub struct ResampleFrac {
    geometry: ResampleGeometry,
    kernel: Vec<f32>,
}
impl ResampleFrac {
    pub fn new(geometry: ResampleGeometry, cancelled: &dyn Fn() -> bool) -> Result<Self, Error> {
        checkpoint(cancelled)?;
        let g = geometry;
        let mut kernel = zeros(g.kernel_len, "demucs.audio.kernel")?;
        if g.old != g.new {
            let sr = (g.old.min(g.new) as f64 * g.options.rolloff) as f32;
            let z = g.options.zeros as f32;
            for phase in 0..g.new {
                checkpoint(cancelled)?;
                let row = &mut kernel[phase * g.taps..(phase + 1) * g.taps];
                let offset = (-(phase as f64) / g.new as f64) as f32;
                let mut sum = 0.0_f32;
                for (i, value) in row.iter_mut().enumerate() {
                    if i.is_multiple_of(BLOCK) {
                        checkpoint(cancelled)?;
                    }
                    let idx = (i as isize - g.width as isize) as f32;
                    let mut t = (offset + idx / g.old as f32) * sr;
                    t = t.clamp(-z, z);
                    t *= std::f32::consts::PI;
                    let cosine = (t / z / 2.0).cos();
                    let window = cosine * cosine;
                    let sinc = if t == 0.0 { 1.0 } else { t.sin() / t };
                    *value = sinc * window;
                    sum += *value;
                }
                if !sum.is_finite() || sum == 0.0 {
                    return Err(Error::NonFinite);
                }
                for (i, value) in row.iter_mut().enumerate() {
                    if i.is_multiple_of(BLOCK) {
                        checkpoint(cancelled)?;
                    }
                    *value /= sum;
                    if !value.is_finite() {
                        return Err(Error::NonFinite);
                    }
                }
            }
        }
        checkpoint(cancelled)?;
        Ok(Self { geometry, kernel })
    }
    pub fn geometry(&self) -> ResampleGeometry {
        self.geometry
    }
    pub fn kernel(&self) -> &[f32] {
        &self.kernel
    }

    pub fn process_into(
        &self,
        input: &[f32],
        output: &mut [f32],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        let g = self.geometry;
        if input.len() != g.input_len || output.len() != g.output_len {
            return Err(Error::Geometry);
        }
        finite(input, cancelled)?;
        if g.old == g.new {
            copy_into(input, output, cancelled)?;
        } else {
            for lane in 0..g.lanes {
                let source = &input[lane * g.frames..(lane + 1) * g.frames];
                for (group, targets) in output[lane * g.output_frames..(lane + 1) * g.output_frames]
                    .chunks_mut(g.new)
                    .enumerate()
                {
                    let start = group * g.old;
                    let interior = start
                        .checked_sub(g.width)
                        .and_then(|i| source.get(i..i + g.taps));
                    for (value, row) in targets.iter_mut().zip(self.kernel.chunks_exact(g.taps)) {
                        let mut sum = 0.0_f32;
                        // Check even a single output with a very large kernel.
                        for (block, weights) in row.chunks(BLOCK).enumerate() {
                            checkpoint(cancelled)?;
                            let first = block * BLOCK;
                            if let Some(samples) = interior {
                                for (&sample, &weight) in
                                    samples[first..first + weights.len()].iter().zip(weights)
                                {
                                    sum += sample * weight;
                                }
                            } else {
                                for (i, &weight) in weights.iter().enumerate() {
                                    let index = (start + first + i)
                                        .saturating_sub(g.width)
                                        .min(g.frames - 1);
                                    sum += source[index] * weight;
                                }
                            }
                        }
                        if !sum.is_finite() {
                            return Err(Error::NonFinite);
                        }
                        *value = sum;
                    }
                }
            }
        }
        checkpoint(cancelled)
    }
}

/// Checked logical extent of `convert_audio`, before any allocation. The source,
/// intermediate (if used), final output and kernels are counted together;
/// allocator/runtime/other caller owners are separate.
#[derive(Clone, Copy, Debug)]
pub struct ConversionGeometry {
    channels: Channels,
    resample: Option<ResampleGeometry>,
    intermediate_len: usize,
    payload_bytes: usize,
}
impl ConversionGeometry {
    pub fn new(
        channels: Channels,
        old_rate: u32,
        new_rate: u32,
        options: Options,
    ) -> Result<Self, Error> {
        validate(old_rate, new_rate, options)?;
        let resample = if old_rate == new_rate {
            None
        } else {
            Some(ResampleGeometry::new(
                channels.frames,
                product(channels.batches, channels.target)?,
                old_rate,
                new_rate,
                options,
            )?)
        };
        let intermediate_len = if old_rate == new_rate || channels.source == channels.target {
            0
        } else {
            channels.output_len
        };
        let payload_bytes = payload(&[
            channels.input_len,
            intermediate_len,
            resample.map_or(channels.output_len, |g| g.output_len),
            resample.map_or(0, |g| g.kernel_len),
        ])?;
        Ok(Self {
            channels,
            resample,
            intermediate_len,
            payload_bytes,
        })
    }
    pub fn input_len(self) -> usize {
        self.channels.input_len
    }
    pub fn output_frames(self) -> usize {
        self.resample
            .map_or(self.channels.frames, |g| g.output_frames)
    }
    pub fn output_len(self) -> usize {
        self.resample
            .map_or(self.channels.output_len, |g| g.output_len)
    }
    pub fn payload_bytes(self) -> usize {
        self.payload_bytes
    }
}

/// `convert_audio`: change channels before resampling. Equal rates convert
/// straight into final output; unchanged channels borrow the input directly.
pub struct Converter {
    geometry: ConversionGeometry,
    resampler: Option<ResampleFrac>,
    converted: Vec<f32>,
}
impl Converter {
    pub fn new(geometry: ConversionGeometry, cancelled: &dyn Fn() -> bool) -> Result<Self, Error> {
        checkpoint(cancelled)?;
        let resampler = geometry
            .resample
            .map(|g| ResampleFrac::new(g, cancelled))
            .transpose()?;
        let converted = zeros(geometry.intermediate_len, "demucs.audio.channels")?;
        checkpoint(cancelled)?;
        Ok(Self {
            geometry,
            resampler,
            converted,
        })
    }
    pub fn geometry(&self) -> ConversionGeometry {
        self.geometry
    }
    pub fn kernel(&self) -> &[f32] {
        self.resampler.as_ref().map_or(&[], ResampleFrac::kernel)
    }
    pub fn convert_into(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        let g = self.geometry;
        if output.len() != g.output_len() {
            return Err(Error::Geometry);
        }
        let Some(resampler) = &self.resampler else {
            return g.channels.convert_into(input, output, cancelled);
        };
        if self.converted.is_empty() {
            return resampler.process_into(input, output, cancelled);
        }
        g.channels
            .convert_into(input, &mut self.converted, cancelled)?;
        resampler.process_into(&self.converted, output, cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn original_lengths_noop_and_checked_payloads() {
        let g = ResampleGeometry::new(200_180, 1, 44_100, 48_000, Options::default()).unwrap();
        assert_eq!(g.output_frames(), 217_883); // rational floor would be one less.
        let both = Options {
            full: true,
            output_length: Some(usize::MAX),
            ..Options::default()
        };
        assert_eq!(
            ResampleGeometry::new(17, 2, 44_100, 44_100, both)
                .unwrap()
                .output_frames(),
            17
        );
        assert!(ResampleGeometry::new(17, 2, 44_100, 48_000, both).is_err());
        assert!(ResampleGeometry::new(usize::MAX, 2, 44_100, 48_000, Options::default()).is_err());
        assert!(ResampleGeometry::new(1, 1, 767_999, 768_000, Options::default()).is_err());
        assert!(ResampleGeometry::new(1, 1, 0, 44_100, Options::default()).is_err());
        assert!(ResampleGeometry::new(1, 1, MAX_RATE + 1, 44_100, Options::default()).is_err());
        assert!(Channels::new(1, 1, 2, 3).is_err());
        assert!(Channels::new(usize::MAX, 2, 2, 2).is_err());
        assert!(Channels::new(0, 1, 1, 1).is_err());
        // Same-rate expansion has source + final output only; no phantom copy.
        let expansion = Channels::new(20_000_000, 1, 1, 5).unwrap();
        let g = ConversionGeometry::new(expansion, 44_100, 44_100, both).unwrap();
        assert_eq!(g.payload_bytes(), 480_000_000);
        assert!(ConversionGeometry::new(expansion, 48_000, 44_100, Options::default()).is_err());
        let select = Channels::new(20_000_000, 1, 4, 2).unwrap();
        assert!(select.payload_bytes().is_ok());
        assert!(ResampleGeometry::new(20_000_000, 2, 96_000, 44_100, Options::default()).is_ok());
        assert!(ConversionGeometry::new(select, 96_000, 44_100, Options::default()).is_err());
    }

    #[test]
    fn channel_axis_and_full_conversion_cancellation_reuse() {
        let channels = Channels::new(3, 2, 3, 1).unwrap();
        let input = [
            1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12., 13., 14., 15., 16., 17., 18.,
        ];
        let mut converted = [0.; 6];
        channels
            .convert_into(&input, &mut converted, &|| false)
            .unwrap();
        assert_eq!(converted, [4., 5., 6., 13., 14., 15.]);
        let g = ConversionGeometry::new(channels, 48_000, 44_100, Options::default()).unwrap();
        let mut work = Converter::new(g, &|| false).unwrap();
        let mut output = vec![9.; work.geometry().output_len()];
        assert_eq!(
            work.convert_into(&input, &mut output, &|| true),
            Err(Error::Cancelled)
        );
        work.convert_into(&input, &mut output, &|| false).unwrap();
        let expected = output.clone();
        let calls = Cell::new(0);
        work.convert_into(&input, &mut output, &|| {
            calls.set(calls.get() + 1);
            false
        })
        .unwrap();
        let total = calls.get();
        calls.set(0);
        assert_eq!(
            work.convert_into(&input, &mut output, &|| {
                calls.set(calls.get() + 1);
                calls.get() == total
            }),
            Err(Error::Cancelled)
        );
        work.convert_into(&input, &mut output, &|| false).unwrap();
        assert_eq!(output, expected);
        let mut bad = input;
        bad[17] = f32::NAN;
        assert_eq!(
            work.convert_into(&bad, &mut output, &|| false),
            Err(Error::NonFinite)
        );
        assert_eq!(
            work.convert_into(&input[..17], &mut output, &|| false),
            Err(Error::Geometry)
        );
        assert!(matches!(
            ResampleFrac::new(g.resample.unwrap(), &|| true),
            Err(Error::Cancelled)
        ));
    }

    #[test]
    fn inner_kernel_cancel_zero_output_and_constant_tail() {
        let g = ResampleGeometry::new(
            1,
            1,
            768_000,
            44_100,
            Options {
                full: true,
                ..Options::default()
            },
        )
        .unwrap();
        let calls = Cell::new(0);
        assert!(matches!(
            ResampleFrac::new(g, &|| {
                calls.set(calls.get() + 1);
                calls.get() == 3
            }),
            Err(Error::Cancelled)
        ));
        let work = ResampleFrac::new(g, &|| false).unwrap();
        let mut output = vec![0.; g.output_len()];
        work.process_into(&[0.25], &mut output, &|| false).unwrap();
        assert!((output[0] - 0.25).abs() < 2e-5);
        let long = ResampleGeometry::new(
            1,
            1,
            4097,
            1,
            Options {
                zeros: 1,
                full: true,
                ..Options::default()
            },
        )
        .unwrap();
        let work = ResampleFrac::new(long, &|| false).unwrap();
        calls.set(0);
        output[0] = -7.;
        assert_eq!(
            work.process_into(&[0.25], &mut output, &|| {
                calls.set(calls.get() + 1);
                calls.get() == 3
            }),
            Err(Error::Cancelled)
        );
        assert_eq!(output[0], -7.); // second tap block, before the sole output is written
        let zero = ResampleGeometry::new(1, 1, 48_000, 16_000, Options::default()).unwrap();
        assert_eq!(zero.output_frames(), 0);
        ResampleFrac::new(zero, &|| false)
            .unwrap()
            .process_into(&[1.], &mut [], &|| false)
            .unwrap();
    }
}
