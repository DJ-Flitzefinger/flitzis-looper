//! Bounded offline PCM16 conversion. No Python, file, engine, thread or device dependency.
//!
//! The caller retains source readers, reserves host memory before reading spans,
//! checks cancellation around each synchronous block, and owns verified file commit.
//! Binary64 evaluation order is part of the contract; never enable fast-math/FMA.

use std::fmt;

pub const MAX_BLOCK_FRAMES: usize = 4096;
pub const MAX_PCM_BYTES: usize = 1024 * 1024 * 1024;
pub const POLICY: &str = "source-bound-aligned-pcm16-native-v3";

#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub rate: u32,
    pub channels: u16,
    pub frames: u64,
}

impl Shape {
    fn validate(self) -> Result<(), Error> {
        if self.rate == 0
            || self.channels == 0
            || self.channels > u16::MAX / 2
            || self.frames == 0
            || u64::from(self.rate) * u64::from(self.channels) * 2 > u64::from(u32::MAX)
            || self.frames > (u64::MAX - 72) / (u64::from(self.channels) * 2)
        {
            return Err(Error::Shape);
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub struct Source<'a> {
    pub shape: Shape,
    pub first: u64,
    pub pcm: &'a [u8],
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Shape,
    Span,
    Budget,
    Allocation,
    Cancelled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Shape => "invalid bounded stem conversion geometry",
            Self::Span => "incomplete or mispositioned stem PCM span",
            Self::Budget => "aligned stem block exceeds the transient PCM limit",
            Self::Allocation => "bounded stem payload allocation failed",
            Self::Cancelled => "stem conversion cancelled",
        })
    }
}

impl std::error::Error for Error {}

/// Exact positioned span used by the legacy binary64 global-frame algorithm.
/// Requires validated shapes, positive frames and a checked target interval.
pub fn span(source: Shape, target_rate: u32, start: u64, frames: usize) -> (u64, u64) {
    let ratio = f64::from(source.rate) / f64::from(target_rate);
    let first = ((start as f64 * ratio).floor() as u64).min(source.frames);
    let last = start.saturating_add(frames.saturating_sub(1) as u64);
    let end = (last as f64 * ratio).floor() as u64;
    let end = end.saturating_add(2).min(source.frames);
    (first, end.saturating_sub(first))
}

/// PCM cap includes all retained raw spans, five native outputs, five Python
/// return copies and a conservative writer copy. Fixed metadata uses host scratch.
pub fn required_bytes(sources: &[Source<'_>; 4], samples: usize) -> Result<usize, Error> {
    sources.iter().try_fold(
        samples.checked_mul(22).ok_or(Error::Budget)?,
        |total, source| total.checked_add(source.pcm.len()).ok_or(Error::Budget),
    )
}

fn decode(pcm: &[u8], index: usize) -> f64 {
    let value = i16::from_le_bytes([pcm[2 * index], pcm[2 * index + 1]]);
    if value == i16::MIN {
        -1.0
    } else {
        f64::from(value) / 32767.0
    }
}

fn sample(source: Source<'_>, frame: u64, channel: usize, channels: usize) -> f64 {
    if frame >= source.shape.frames {
        return 0.0;
    }
    let count = usize::from(source.shape.channels);
    let offset = (frame - source.first) as usize * count;
    if count == channels {
        decode(source.pcm, offset + channel)
    } else if count == 1 {
        decode(source.pcm, offset)
    } else if channels == 1 {
        // CPython 3.12+ sum(array('d')) uses compensated Neumaier addition.
        // Preserve its operation order, including final compensation, before /n.
        let mut hi = decode(source.pcm, offset);
        let mut lo = 0.0;
        for index in 1..count {
            let value = decode(source.pcm, offset + index);
            let next = hi + value;
            lo += if hi.abs() >= value.abs() {
                (hi - next) + value
            } else {
                (value - next) + hi
            };
            hi = next;
        }
        (if lo != 0.0 && lo.is_finite() {
            hi + lo
        } else {
            hi
        }) / count as f64
    } else if channel < count {
        decode(source.pcm, offset + channel)
    } else {
        0.0
    }
}

pub fn pcm16(value: f64) -> i16 {
    let value = if value.is_finite() { value } else { 0.0 };
    if value >= 1.0 {
        i16::MAX
    } else if value <= -1.0 {
        i16::MIN
    } else {
        (value * 32767.0).round_ties_even() as i16
    }
}

/// Convert one complete block in vocals/drums/bass/melody/instrumental order.
/// Validation and the complete prospective PCM budget precede all allocations.
/// Cancellation is Rust-owned and optional; no Python callback crosses this core.
pub fn convert(
    sources: [Source<'_>; 4],
    target: Shape,
    start: u64,
    frames: usize,
    limit: usize,
    cancelled: impl Fn() -> bool,
) -> Result<[Vec<u8>; 5], Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    target.validate()?;
    if frames == 0
        || frames > MAX_BLOCK_FRAMES
        || start
            .checked_add(frames as u64)
            .is_none_or(|end| end > target.frames)
    {
        return Err(Error::Shape);
    }
    for source in sources {
        source.shape.validate()?;
        let (first, count) = span(source.shape, target.rate, start, frames);
        let bytes = count
            .checked_mul(u64::from(source.shape.channels) * 2)
            .and_then(|size| usize::try_from(size).ok())
            .ok_or(Error::Span)?;
        if first != source.first || bytes != source.pcm.len() {
            return Err(Error::Span);
        }
    }
    let channels = usize::from(target.channels);
    let samples = frames.checked_mul(channels).ok_or(Error::Budget)?;
    if limit == 0 || limit > MAX_PCM_BYTES || required_bytes(&sources, samples)? > limit {
        return Err(Error::Budget);
    }
    let bytes = samples.checked_mul(2).ok_or(Error::Budget)?;
    let mut output: [Vec<u8>; 5] = std::array::from_fn(|_| Vec::new());
    for payload in &mut output {
        payload
            .try_reserve_exact(bytes)
            .map_err(|_| Error::Allocation)?;
    }
    for local in 0..frames {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let global = start + local as u64;
        for channel in 0..channels {
            let mut values = [0.0; 4];
            for (index, source) in sources.iter().copied().enumerate() {
                let ratio = f64::from(source.shape.rate) / f64::from(target.rate);
                let position = global as f64 * ratio;
                let floor = position.floor();
                let frame = floor as u64;
                let fraction = position - floor;
                let before = sample(source, frame, channel, channels);
                let after = sample(source, frame.saturating_add(1), channel, channels);
                let value = before + (after - before) * fraction;
                values[index] = value;
                output[index].extend_from_slice(&pcm16(value).to_le_bytes());
            }
            let mut sum = 0.0;
            for value in &values[1..] {
                sum = (sum + value).clamp(-1.0, 1.0);
            }
            output[4].extend_from_slice(&pcm16(sum).to_le_bytes());
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
