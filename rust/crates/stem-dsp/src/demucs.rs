//! Offline Demucs 4.0.1 HTDemucs `_spec` / scale-zero `_ispec` arithmetic.
//!
//! This is the unnormalized spectral boundary. The separate `host` module
//! provides normalization and physical track arithmetic, and `audio` handles
//! channel conversion and rational resampling. The `hybrid` module composes
//! both normalized branches around a caller-supplied predictor.
//! Inputs are planar lanes (batch/source/channel axes flattened). Spectra use
//! CaC `[lane, real/imag, frequency, time]`, matching `_magnitude` / `_mask`.
//! Calls are synchronous and exclusive; immutable input readers and caller-owned
//! output/admission must remain held through return. An error invalidates the
//! entire output, including any prefix already written. Return is not OS/reader
//! END. There is no worker, file publication, process or realtime integration.

pub mod audio;
pub mod host;
pub mod hybrid;
pub mod offline;
pub mod reduction;

use rustfft::{Fft, FftPlanner, num_complex::Complex32};
use std::{fmt, sync::Arc};

pub const RATE: u32 = 44_100;
pub const FFT: usize = 4096;
pub const HOP: usize = FFT / 4;
pub const BINS: usize = FFT / 2; // Nyquist is deliberately discarded.
const PAD: usize = HOP / 2 * 3;
const TRIM: usize = FFT / 2 + PAD;
const BLOCK: usize = 4096;
const MAX_SCRATCH: usize = FFT * 4;
const MAX_WORKING_BYTES: usize = (2 * (FFT + MAX_SCRATCH) * 2 + 3 * FFT) * 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Geometry,
    NonFinite,
    Allocation {
        phase: &'static str,
        requested_bytes: usize,
    },
    Cancelled,
    Envelope,
    Predictor,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Geometry => {
                f.write_str("unsupported HTDemucs spectral geometry or payload extent")
            }
            Self::NonFinite => f.write_str("nonfinite HTDemucs spectral input or output"),
            Self::Allocation {
                phase,
                requested_bytes,
            } => write!(
                f,
                "HTDemucs allocation failed: phase={phase} requested_bytes={requested_bytes}"
            ),
            Self::Cancelled => f.write_str("HTDemucs spectral work cancelled"),
            Self::Envelope => f.write_str("invalid HTDemucs inverse overlap envelope"),
            Self::Predictor => f.write_str("HTDemucs normalized predictor failed"),
        }
    }
}
impl std::error::Error for Error {}

/// Complete physical length at the model's 44100 Hz boundary. No segment rounding
/// changes the original length. Training-segment padding remains the host's job.
#[derive(Debug, Clone, Copy)]
pub struct Geometry {
    frames: usize,
    lanes: usize,
    steps: usize,
    pcm_len: usize,
    feature_len: usize,
    payload_bytes: usize,
}

impl Geometry {
    pub fn new(frames: usize, lanes: usize, rate: u32) -> Result<Self, Error> {
        if frames == 0 || lanes == 0 || rate != RATE {
            return Err(Error::Geometry);
        }
        let steps = frames.checked_add(HOP - 1).ok_or(Error::Geometry)? / HOP;
        steps
            .checked_add(4)
            .and_then(|n| n.checked_mul(HOP))
            .and_then(|n| n.checked_add(FFT))
            .ok_or(Error::Geometry)?;
        let pcm_len = frames.checked_mul(lanes).ok_or(Error::Geometry)?;
        let feature_len = steps
            .checked_mul(BINS * 2)
            .and_then(|n| n.checked_mul(lanes))
            .ok_or(Error::Geometry)?;
        let payload_bytes = pcm_len
            .checked_mul(2)
            .and_then(|n| n.checked_add(feature_len))
            .and_then(|n| n.checked_mul(4))
            .and_then(|n| n.checked_add(MAX_WORKING_BYTES))
            .ok_or(Error::Geometry)?;
        Ok(Self {
            frames,
            lanes,
            steps,
            pcm_len,
            feature_len,
            payload_bytes,
        })
    }
    pub fn frames(self) -> usize {
        self.frames
    }
    pub fn lanes(self) -> usize {
        self.lanes
    }
    pub fn steps(self) -> usize {
        self.steps
    }
    pub fn pcm_len(self) -> usize {
        self.pcm_len
    }
    pub fn feature_len(self) -> usize {
        self.feature_len
    }
    /// Includes one input PCM, one CaC spectrum, one output PCM and the maximum
    /// core vector requests. Excludes FFT plans, allocator overhead, runtime and
    /// any other caller/model owners. It cannot qualify a WHOLE process budget.
    pub fn payload_bytes(self) -> usize {
        self.payload_bytes
    }

    fn feature(self, lane: usize, part: usize, frequency: usize, time: usize) -> usize {
        ((lane * 2 + part) * BINS + frequency) * self.steps + time
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

fn zeros<T: Default + Clone>(len: usize, phase: &'static str) -> Result<Vec<T>, Error> {
    let requested_bytes = len.checked_mul(size_of::<T>()).ok_or(Error::Geometry)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| Error::Allocation {
            phase,
            requested_bytes,
        })?;
    values.resize(len, T::default());
    Ok(values)
}

/// Virtual `hdemucs.pad1d` with its short-input zero extension BEFORE reflection.
/// Retained STFT frame t starts at t*HOP in this signal: dropping the first two
/// center-STFT frames cancels its half-FFT offset. No second reflect copy needed.
struct Padding {
    length: usize,
    zero_left: usize,
    extended: usize,
    reflect_left: usize,
}
impl Padding {
    fn new(g: Geometry) -> Self {
        let right = PAD + g.steps * HOP - g.frames;
        let extra = (right + 1).saturating_sub(g.frames);
        let zero_right = right.min(extra);
        let zero_left = extra - zero_right;
        Self {
            length: g.frames,
            zero_left,
            extended: g.frames + extra,
            reflect_left: PAD - zero_left,
        }
    }
    fn sample(&self, pcm: &[f32], position: usize) -> f32 {
        let reflected = if position < self.reflect_left {
            self.reflect_left - position
        } else {
            let relative = position - self.reflect_left;
            if relative >= self.extended {
                2 * self.extended - 2 - relative
            } else {
                relative
            }
        };
        if reflected < self.zero_left || reflected - self.zero_left >= self.length {
            0.0
        } else {
            pcm[reflected - self.zero_left]
        }
    }
}

struct Transform {
    plan: Arc<dyn Fft<f32>>,
    buffer: Vec<Complex32>,
    scratch: Vec<Complex32>,
}
impl Transform {
    fn new(plan: Arc<dyn Fft<f32>>) -> Result<Self, Error> {
        let scratch = plan.get_inplace_scratch_len();
        if scratch > MAX_SCRATCH {
            return Err(Error::Geometry);
        }
        Ok(Self {
            plan,
            buffer: zeros(FFT, "demucs.fft")?,
            scratch: zeros(scratch, "demucs.scratch")?,
        })
    }
    fn run(&mut self) {
        self.plan
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
    }
}

/// Fixed 4096-point work, reused across arbitrary admitted lengths/lanes. No
/// allocation occurs in analyze_into/reconstruct_into. FFT planning can allocate
/// infallibly in RustFFT/System; vector errors below are fallible logical requests,
/// not an OOM or finite successful-process guarantee. This is offline work only.
pub struct Workspace {
    forward: Transform,
    inverse: Transform,
    window: Vec<f32>,
    sum: Vec<f32>,
    envelope: Vec<f32>,
}

impl Workspace {
    pub fn new(cancelled: &dyn Fn() -> bool) -> Result<Self, Error> {
        checkpoint(cancelled)?;
        let mut planner = FftPlanner::new();
        let forward = Transform::new(planner.plan_fft_forward(FFT))?;
        checkpoint(cancelled)?;
        let inverse = Transform::new(planner.plan_fft_inverse(FFT))?;
        let mut window = zeros(FFT, "demucs.window")?;
        for (i, value) in window.iter_mut().enumerate() {
            let angle = i as f32 * (2.0_f32 * std::f32::consts::PI / FFT as f32);
            *value = (1.0 - angle.cos()) * 0.5;
        }
        checkpoint(cancelled)?;
        Ok(Self {
            forward,
            inverse,
            window,
            sum: zeros(FFT, "demucs.sum")?,
            envelope: zeros(FFT, "demucs.envelope")?,
        })
    }

    /// Actual fixed logical work-vector bytes; FFT plan/native backing excluded.
    pub fn working_payload_bytes(&self) -> usize {
        (self.forward.buffer.len()
            + self.forward.scratch.len()
            + self.inverse.buffer.len()
            + self.inverse.scratch.len())
            * size_of::<Complex32>()
            + (self.window.len() + self.sum.len() + self.envelope.len()) * size_of::<f32>()
    }

    pub fn analyze_into(
        &mut self,
        g: Geometry,
        pcm: &[f32],
        output: &mut [f32],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        if pcm.len() != g.pcm_len || output.len() != g.feature_len {
            return Err(Error::Geometry);
        }
        finite(pcm, cancelled)?;
        let padding = Padding::new(g);
        for lane in 0..g.lanes {
            let input = &pcm[lane * g.frames..(lane + 1) * g.frames];
            for time in 0..g.steps {
                checkpoint(cancelled)?;
                for i in 0..FFT {
                    self.forward.buffer[i] =
                        Complex32::new(padding.sample(input, time * HOP + i) * self.window[i], 0.0);
                }
                self.forward.run();
                for frequency in 0..BINS {
                    let value = self.forward.buffer[frequency] * (1.0 / 64.0);
                    if !value.re.is_finite() || !value.im.is_finite() {
                        return Err(Error::NonFinite);
                    }
                    output[g.feature(lane, 0, frequency, time)] = value.re;
                    output[g.feature(lane, 1, frequency, time)] = value.im;
                }
            }
        }
        checkpoint(cancelled)
    }

    /// Direct CaC reconstruction (not multiplicative BS masks). Zero Nyquist and
    /// two zero time frames on EACH side match `_ispec(scale=0)`. Even zero frames
    /// contribute Hann-squared envelope. The rolling ring includes that envelope
    /// and emits exactly g.frames after both original trims, with no whole-track
    /// inverse accumulation. Higher inverse scales and other Demucs families are
    /// deliberately outside this fixed original HTDemucs boundary.
    pub fn reconstruct_into(
        &mut self,
        g: Geometry,
        spectrum: &[f32],
        output: &mut [f32],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        if spectrum.len() != g.feature_len || output.len() != g.pcm_len {
            return Err(Error::Geometry);
        }
        finite(spectrum, cancelled)?;
        for lane in 0..g.lanes {
            checkpoint(cancelled)?;
            self.sum.fill(0.0);
            self.envelope.fill(0.0);
            for frame in 0..g.steps + 4 {
                checkpoint(cancelled)?;
                self.inverse.buffer.fill(Complex32::default());
                if (2..g.steps + 2).contains(&frame) {
                    let time = frame - 2;
                    // Real inverse ignores the imaginary DC component.
                    self.inverse.buffer[0].re = spectrum[g.feature(lane, 0, 0, time)];
                    for frequency in 1..BINS {
                        let value = Complex32::new(
                            spectrum[g.feature(lane, 0, frequency, time)],
                            spectrum[g.feature(lane, 1, frequency, time)],
                        );
                        self.inverse.buffer[frequency] = value;
                        self.inverse.buffer[FFT - frequency] = value.conj();
                    }
                    self.inverse.run();
                }
                let start = frame * HOP;
                for i in 0..FFT {
                    let slot = (start + i) % FFT;
                    self.sum[slot] += (self.inverse.buffer[i].re * (1.0 / 64.0)) * self.window[i];
                    self.envelope[slot] += self.window[i] * self.window[i];
                }
                for raw in start..start + HOP {
                    let slot = raw % FFT;
                    if (TRIM..TRIM + g.frames).contains(&raw) {
                        let denominator = self.envelope[slot];
                        if !denominator.is_finite() || denominator <= 1e-11 {
                            return Err(Error::Envelope);
                        }
                        let value = self.sum[slot] / denominator;
                        if !value.is_finite() {
                            return Err(Error::NonFinite);
                        }
                        output[lane * g.frames + raw - TRIM] = value;
                    }
                    self.sum[slot] = 0.0;
                    self.envelope[slot] = 0.0;
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
    fn original_geometry_preserves_release_tail_and_rejects_overflow() {
        let g = Geometry::new(343_980, 2, RATE).unwrap();
        assert_eq!(
            (g.frames(), g.steps(), g.feature_len()),
            (343_980, 336, 2 * 2 * 2048 * 336)
        );
        assert!(Geometry::new(usize::MAX, 2, RATE).is_err());
        assert!(Geometry::new(1, usize::MAX, RATE).is_err());
        assert!(Geometry::new(0, 2, RATE).is_err());
        assert!(Geometry::new(1, 0, RATE).is_err());
        assert!(Geometry::new(343_980, 2, 48_000).is_err());
        let large = Geometry::new(20_000_000, 2, RATE).unwrap();
        assert!(large.payload_bytes() > 512 * 1024 * 1024);
        assert!(Geometry::new(705_601, 2, RATE).is_ok());
    }

    #[test]
    fn cancellation_and_bad_inputs_leave_workspace_reusable() {
        assert!(matches!(Workspace::new(&|| true), Err(Error::Cancelled)));
        let g = Geometry::new(4097, 2, RATE).unwrap();
        let mut work = Workspace::new(&|| false).unwrap();
        assert!(work.working_payload_bytes() <= MAX_WORKING_BYTES);
        let pcm = vec![0.0; g.pcm_len()];
        let mut z = vec![0.0; g.feature_len()];
        let mut out = vec![17.0; g.pcm_len()];
        let calls = Cell::new(0);
        let cancel = || {
            calls.set(calls.get() + 1);
            calls.get() == 5
        };
        assert_eq!(
            work.analyze_into(g, &pcm, &mut z, &cancel),
            Err(Error::Cancelled)
        );
        work.analyze_into(g, &pcm, &mut z, &|| false).unwrap();
        calls.set(0);
        let cancel = || {
            calls.set(calls.get() + 1);
            calls.get() == g.feature_len().div_ceil(BLOCK) + 5
        };
        assert_eq!(
            work.reconstruct_into(g, &z, &mut out, &cancel),
            Err(Error::Cancelled)
        );
        work.reconstruct_into(g, &z, &mut out, &|| false).unwrap();
        assert!(out.iter().all(|&x| x == 0.0));
        z[0] = f32::NAN;
        assert_eq!(
            work.reconstruct_into(g, &z, &mut out, &|| false),
            Err(Error::NonFinite)
        );
        assert_eq!(
            work.analyze_into(g, &[], &mut z, &|| true),
            Err(Error::Geometry)
        );
        let mut bad_pcm = pcm;
        bad_pcm[0] = f32::INFINITY;
        assert_eq!(
            work.analyze_into(g, &bad_pcm, &mut z, &|| false),
            Err(Error::NonFinite)
        );
    }
}
