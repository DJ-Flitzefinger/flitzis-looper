//! Offline BS-RoFormer spectral trial, independent of Python and tensor runtimes.
//!
//! Matches the pinned vendored model's stereo STFT, feature ordering, complex
//! masking and length-preserving ISTFT. It does not implement the learned network,
//! CUDA AMP, track chunking, overlap-add, file custody or productive dispatch.
//! Callers retain immutable readers and admit the complete transient envelope
//! BEFORE allocating inputs/masks or calling this core, through actual return.

use rustfft::{FftPlanner, num_complex::Complex32};
use std::{fmt, sync::Arc};

pub const RATE: u32 = 44_100;
pub const FFT: usize = 2048;
pub const HOP: usize = 441;
pub const BINS: usize = FFT / 2 + 1;
pub const CHANNELS: usize = 2;
pub const STEMS: usize = 4;
pub const MAX_CHUNK_FRAMES: usize = 485_100;
pub const MAX_TRANSIENT_BYTES: usize = 1 << 30;
const BLOCK: usize = 4096;
const FFT_SCRATCH: usize = FFT * 4;
// Fixed-size FFT planning is separate from fallible payload allocation. This is
// a conservative caller allowance, not an allocator/RSS or whole-process claim.
const PLAN_ALLOWANCE: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Shape,
    NonFinite,
    Allocation,
    Cancelled,
    Envelope,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Shape => "unsupported complete BS spectral chunk geometry",
            Self::NonFinite => "nonfinite BS spectral input or reconstruction",
            Self::Allocation => "bounded BS spectral allocation failed",
            Self::Cancelled => "BS spectral work cancelled",
            Self::Envelope => "invalid BS inverse overlap envelope",
        })
    }
}
impl std::error::Error for Error {}

/// One complete stereo release chunk, with checked extents before payload I/O.
#[derive(Debug, Clone, Copy)]
pub struct Geometry {
    frames: usize,
    steps: usize,
    features: usize,
    inverse_frames: usize,
    transient_bytes: usize,
}

impl Geometry {
    pub fn new(frames: usize, rate: u32) -> Result<Self, Error> {
        if rate != RATE || !(FFT / 2 + 1..=MAX_CHUNK_FRAMES).contains(&frames) {
            return Err(Error::Shape);
        }
        let steps = frames / HOP + 1;
        let features = steps * BINS * CHANNELS * 2;
        let inverse_frames = FFT + (steps - 1) * HOP;
        // Simultaneous caller PCM + spectral features + all four complex masks +
        // four-source output + inverse accumulation/envelope + fixed FFT work.
        // No batches or whole-track buffers are implicitly admitted here.
        let floats = CHANNELS * frames
            + (1 + STEMS) * features
            + STEMS * CHANNELS * frames
            + 2 * inverse_frames
            + FFT
            + 2 * (FFT + FFT_SCRATCH);
        let transient_bytes = floats * size_of::<f32>() + PLAN_ALLOWANCE;
        if transient_bytes > MAX_TRANSIENT_BYTES || FFT / 2 + frames > inverse_frames {
            return Err(Error::Shape);
        }
        Ok(Self {
            frames,
            steps,
            features,
            inverse_frames,
            transient_bytes,
        })
    }

    pub fn frames(self) -> usize {
        self.frames
    }
    pub fn steps(self) -> usize {
        self.steps
    }
    pub fn feature_len(self) -> usize {
        self.features
    }
    pub fn mask_len(self) -> usize {
        STEMS * self.features
    }
    pub fn transient_bytes(self) -> usize {
        self.transient_bytes
    }
}

fn checkpoint(cancelled: &dyn Fn() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

fn finite(values: &[f32], cancelled: &dyn Fn() -> bool) -> Result<(), Error> {
    for block in values.chunks(BLOCK) {
        checkpoint(cancelled)?;
        if block.iter().any(|x| !x.is_finite()) {
            return Err(Error::NonFinite);
        }
    }
    Ok(())
}

fn zeros<T: Default + Clone>(len: usize) -> Result<Vec<T>, Error> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| Error::Allocation)?;
    values.resize(len, T::default());
    Ok(values)
}

struct Transform {
    fft: Arc<dyn rustfft::Fft<f32>>,
    buffer: Vec<Complex32>,
    scratch: Vec<Complex32>,
    window: Vec<f32>,
}

impl Transform {
    fn new(inverse: bool, cancelled: &dyn Fn() -> bool) -> Result<Self, Error> {
        checkpoint(cancelled)?;
        let mut planner = FftPlanner::new();
        let fft = if inverse {
            planner.plan_fft_inverse(FFT)
        } else {
            planner.plan_fft_forward(FFT)
        };
        let scratch_len = fft.get_inplace_scratch_len();
        if scratch_len > FFT_SCRATCH {
            return Err(Error::Shape);
        }
        let mut window = zeros(FFT)?;
        for (i, value) in window.iter_mut().enumerate() {
            // Periodic float32 Hann. FFT/vector cosine implementations may
            // differ in final bits; numerical parity requires an original oracle.
            let angle = (i as f32) * (2.0_f32 * std::f32::consts::PI / FFT as f32);
            *value = (1.0 - angle.cos()) * 0.5;
        }
        Ok(Self {
            fft,
            buffer: zeros(FFT)?,
            scratch: zeros(scratch_len)?,
            window,
        })
    }

    fn run(&mut self) {
        self.fft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
    }
}

/// Immutable features in `[time, frequency, stereo, real/imag]` order.
/// Shape-private ownership prevents reconstruction from accepting forged spectra.
#[derive(Debug)]
pub struct Spectrum {
    geometry: Geometry,
    features: Vec<f32>,
}

impl Spectrum {
    pub fn geometry(&self) -> Geometry {
        self.geometry
    }
    pub fn features(&self) -> &[f32] {
        &self.features
    }

    /// Planar stereo input, complete centered reflection and unnormalized STFT.
    pub fn analyze(pcm: &[f32], rate: u32, cancelled: &dyn Fn() -> bool) -> Result<Self, Error> {
        if !pcm.len().is_multiple_of(CHANNELS) {
            return Err(Error::Shape);
        }
        let geometry = Geometry::new(pcm.len() / CHANNELS, rate)?;
        finite(pcm, cancelled)?;
        let mut transform = Transform::new(false, cancelled)?;
        let mut features = zeros(geometry.features)?;
        for time in 0..geometry.steps {
            for channel in 0..CHANNELS {
                checkpoint(cancelled)?;
                for i in 0..FFT {
                    let position = (time * HOP + i) as isize - (FFT / 2) as isize;
                    // Both reflect pads are < input length, exactly as required
                    // by the original centered torch.stft reflect admission.
                    let source = if position < 0 {
                        -position as usize
                    } else if position as usize >= geometry.frames {
                        2 * geometry.frames - 2 - position as usize
                    } else {
                        position as usize
                    };
                    transform.buffer[i] = Complex32::new(
                        pcm[channel * geometry.frames + source] * transform.window[i],
                        0.0,
                    );
                }
                transform.run();
                for bin in 0..BINS {
                    let offset = ((time * BINS + bin) * CHANNELS + channel) * 2;
                    features[offset] = transform.buffer[bin].re;
                    features[offset + 1] = transform.buffer[bin].im;
                }
            }
        }
        finite(&features, cancelled)?;
        checkpoint(cancelled)?;
        Ok(Self { geometry, features })
    }

    /// Masks are `[stem, time, frequency, stereo, real/imag]`, in the pinned
    /// drums/bass/other/vocals order. Return complete planar `[stem, stereo, PCM]`.
    /// Inputs remain borrowed through actual completion; partial output is private.
    pub fn reconstruct(
        &self,
        masks: &[f32],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<f32>, Error> {
        let g = self.geometry;
        if masks.len() != g.mask_len() {
            return Err(Error::Shape);
        }
        finite(masks, cancelled)?;
        let mut transform = Transform::new(true, cancelled)?;
        let mut envelope: Vec<f32> = zeros(g.inverse_frames)?;
        for time in 0..g.steps {
            checkpoint(cancelled)?;
            for i in 0..FFT {
                envelope[time * HOP + i] += transform.window[i] * transform.window[i];
            }
        }
        let trimmed = &envelope[FFT / 2..FFT / 2 + g.frames];
        if trimmed.iter().any(|x| !x.is_finite() || *x <= 1e-11) {
            return Err(Error::Envelope);
        }
        let mut output = zeros(STEMS * CHANNELS * g.frames)?;
        let mut accumulation: Vec<f32> = zeros(g.inverse_frames)?;
        for stem in 0..STEMS {
            for channel in 0..CHANNELS {
                accumulation.fill(0.0);
                for time in 0..g.steps {
                    checkpoint(cancelled)?;
                    for bin in 0..BINS {
                        let index = ((time * BINS + bin) * CHANNELS + channel) * 2;
                        let mask = stem * g.features + index;
                        let source = Complex32::new(self.features[index], self.features[index + 1]);
                        let mask = Complex32::new(masks[mask], masks[mask + 1]);
                        let value = source * mask;
                        if !value.re.is_finite() || !value.im.is_finite() {
                            return Err(Error::NonFinite);
                        }
                        // Real irfft ignores imaginary DC and Nyquist parts.
                        transform.buffer[bin] = if bin == 0 || bin == FFT / 2 {
                            Complex32::new(value.re, 0.0)
                        } else {
                            value
                        };
                    }
                    for bin in 1..FFT / 2 {
                        transform.buffer[FFT - bin] = transform.buffer[bin].conj();
                    }
                    transform.run();
                    for i in 0..FFT {
                        accumulation[time * HOP + i] +=
                            transform.buffer[i].re / FFT as f32 * transform.window[i];
                    }
                }
                let offset = (stem * CHANNELS + channel) * g.frames;
                for start in (0..g.frames).step_by(BLOCK) {
                    checkpoint(cancelled)?;
                    for i in start..(start + BLOCK).min(g.frames) {
                        let value = accumulation[FFT / 2 + i] / trimmed[i];
                        if !value.is_finite() {
                            return Err(Error::NonFinite);
                        }
                        output[offset + i] = value;
                    }
                }
            }
        }
        checkpoint(cancelled)?;
        Ok(output)
    }
}
