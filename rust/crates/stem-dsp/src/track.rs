//! Offline release track arithmetic. Source, predictor and private sink remain
//! caller-owned; no application provenance, admission or learned model is supplied.
use crate::{BLOCK, CHANNELS, Error, MAX_CHUNK_FRAMES, RATE, STEMS, checkpoint, finite, zeros};

pub const CHUNK: usize = MAX_CHUNK_FRAMES;
pub const BATCH: usize = 2;
pub const STEP: usize = CHUNK / 2;
pub const RING: usize = CHUNK + STEP;
const FADE: usize = CHUNK / 10;
const LANES: usize = STEMS * CHANNELS;

/// Checked streaming geometry; even a one-frame track becomes a full model chunk.
#[derive(Debug, Clone, Copy)]
pub struct TrackPlan {
    source_frames: usize,
    border: usize,
    total: usize,
}

impl TrackPlan {
    pub fn new(source_frames: usize, rate: u32) -> Result<Self, Error> {
        if source_frames == 0 || rate != RATE {
            return Err(Error::Shape);
        }
        let border = if source_frames > 2 * STEP { STEP } else { 0 };
        let total = source_frames.checked_add(2 * border).ok_or(Error::Shape)?;
        // Validate source byte extents and the final advanced batch position
        // before callbacks or payload allocation, including on 32-bit targets.
        source_frames
            .checked_mul(CHANNELS * 4)
            .ok_or(Error::Shape)?;
        total.checked_add(BATCH * STEP).ok_or(Error::Shape)?;
        Ok(Self {
            source_frames,
            border,
            total,
        })
    }

    pub fn source_frames(self) -> usize {
        self.source_frames
    }
    pub fn border(self) -> usize {
        self.border
    }
    pub fn virtual_frames(self) -> usize {
        self.total
    }
    pub fn chunks(self) -> usize {
        self.total.div_ceil(STEP)
    }

    /// Fixed core vectors plus the maximum complete returned prediction batch.
    /// Callback-internal allocations and model/spectral work need separate promises.
    pub fn core_pcm_payload_bytes(self) -> usize {
        (BATCH * CHANNELS * CHUNK
            + CHANNELS * CHUNK
            + LANES * RING
            + RING
            + LANES * CHUNK
            + CHUNK
            + BATCH * LANES * CHUNK)
            * size_of::<f32>()
    }

    /// Original conservative simultaneous PCM allowance, unchanged batch2/overlap2.
    /// Excludes weights, learned/spectral activations, decoder and process memory.
    pub fn pcm_transient_bytes(self) -> usize {
        BATCH * 2 * CHUNK * 4 * 4
            + BATCH * LANES * CHUNK * 4 * 3
            + (LANES + 1) * RING * 4
            + 2 * CHUNK * 4 * 3
            + LANES * CHUNK * 2 * 2
            + CHUNK * 4 * 3
    }
}

struct Buffers {
    input: Vec<f32>,
    source: Vec<f32>,
    result: Vec<f32>,
    counter: Vec<f32>,
    output: Vec<f32>,
    window: Vec<f32>,
}

impl Buffers {
    fn new() -> Result<Self, Error> {
        Ok(Self {
            input: zeros(BATCH * CHANNELS * CHUNK)?,
            source: zeros(CHANNELS * CHUNK)?,
            result: zeros(LANES * RING)?,
            counter: zeros(RING)?,
            output: zeros(LANES * CHUNK)?,
            window: zeros(CHUNK)?,
        })
    }

    fn read_chunk<R>(
        &mut self,
        plan: TrackPlan,
        batch: usize,
        start: usize,
        length: usize,
        read: &mut R,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error>
    where
        R: FnMut(usize, usize, &mut [f32]) -> Result<usize, Error>,
    {
        let end = start + length;
        let mut position = start;
        while position < end {
            checkpoint(cancelled)?;
            let (stop, original, reverse) = if position < plan.border {
                let stop = end.min(plan.border);
                (stop, plan.border - stop + 1, true)
            } else if position < plan.border + plan.source_frames {
                (
                    end.min(plan.border + plan.source_frames),
                    position - plan.border,
                    false,
                )
            } else {
                (
                    end,
                    plan.source_frames - 1 - (end - plan.border - plan.source_frames),
                    true,
                )
            };
            let count = stop - position;
            let source = &mut self.source[..CHANNELS * count];
            if read(original, count, source)? != count {
                return Err(Error::Source);
            }
            finite(source, cancelled)?;
            for channel in 0..CHANNELS {
                let target = (batch * CHANNELS + channel) * CHUNK + position - start;
                for block in (0..count).step_by(BLOCK) {
                    checkpoint(cancelled)?;
                    for i in block..(block + BLOCK).min(count) {
                        let index = if reverse { count - 1 - i } else { i };
                        self.input[target + i] = source[channel * count + index];
                    }
                }
            }
            position = stop;
        }
        for channel in 0..CHANNELS {
            let offset = (batch * CHANNELS + channel) * CHUNK;
            for block in (length..CHUNK).step_by(BLOCK) {
                checkpoint(cancelled)?;
                for i in block..(block + BLOCK).min(CHUNK) {
                    self.input[offset + i] = if length > CHUNK / 2 + 1 {
                        self.input[offset + 2 * length - 2 - i]
                    } else {
                        0.0
                    };
                }
            }
        }
        Ok(())
    }

    fn add(
        &mut self,
        plan: TrackPlan,
        base: usize,
        locations: &[(usize, usize)],
        prediction: &[f32],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        if prediction.len() != locations.len() * LANES * CHUNK {
            return Err(Error::Prediction);
        }
        // Validate the ENTIRE batch before mutating the ring or exposing a block.
        finite(prediction, cancelled)?;
        for (batch, &(start, length)) in locations.iter().enumerate() {
            self.window.fill(1.0);
            for i in 0..FADE {
                let value = i as f32 / (FADE - 1) as f32;
                if start != 0 {
                    self.window[i] = value;
                }
                if start + STEP < plan.total {
                    self.window[CHUNK - 1 - i] = value;
                }
            }
            let offset = start - base;
            for block in (0..length).step_by(BLOCK) {
                checkpoint(cancelled)?;
                for i in block..(block + BLOCK).min(length) {
                    self.counter[offset + i] += self.window[i];
                    for lane in 0..LANES {
                        let index = lane * RING + offset + i;
                        let weighted =
                            prediction[(batch * LANES + lane) * CHUNK + i] * self.window[i];
                        self.result[index] += weighted;
                        if !self.result[index].is_finite() {
                            return Err(Error::NonFinite);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn flush<W>(
        &mut self,
        plan: TrackPlan,
        base: usize,
        position: usize,
        write: &mut W,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<usize, Error>
    where
        W: FnMut(usize, usize, &[f32]) -> Result<(), Error>,
    {
        let flushed = (position - base).min(plan.total - base);
        let begin = plan.border.saturating_sub(base).min(flushed);
        let end = (plan.border + plan.source_frames)
            .saturating_sub(base)
            .min(flushed);
        let count = end - begin;
        if count > 0 {
            for block in (0..count).step_by(BLOCK) {
                checkpoint(cancelled)?;
                for i in block..(block + BLOCK).min(count) {
                    let denominator = self.counter[begin + i];
                    if !denominator.is_finite() || denominator <= 0.0 {
                        return Err(Error::Envelope);
                    }
                    for lane in 0..LANES {
                        let value = self.result[lane * RING + begin + i] / denominator;
                        if !value.is_finite() {
                            return Err(Error::NonFinite);
                        }
                        self.output[lane * count + i] = value;
                    }
                }
            }
            checkpoint(cancelled)?;
            write(
                base + begin - plan.border,
                count,
                &self.output[..LANES * count],
            )?;
            checkpoint(cancelled)?;
        }
        for lane in self.result.chunks_exact_mut(RING) {
            checkpoint(cancelled)?;
            lane.copy_within(flushed.., 0);
            lane[RING - flushed..].fill(0.0);
        }
        self.counter.copy_within(flushed.., 0);
        self.counter[RING - flushed..].fill(0.0);
        Ok(flushed)
    }
}

/// Process one complete track with fixed release batching and bounded PCM storage.
///
/// `read` fills planar stereo for the exact physical span and returns read frames.
/// `predict` synchronously returns complete finite `[batch, stem, stereo, CHUNK]`
/// from `[batch, stereo, CHUNK]`; this boundary does not supply the learned net.
/// `write` consumes consecutive planar `[stem, stereo, frames]` blocks at physical
/// offsets. It MUST stage privately: a later error/cancellation invalidates the
/// ENTIRE attempt, including already written blocks. Only successful actual return
/// can permit caller verification/publication. Callbacks must cooperate with
/// cancellation and retain source/model/output readers through their real finish.
///
/// Caller admits the COMPLETE source/model/spectral/process envelope BEFORE
/// allocation, preserving original floors and promises until tree AND readers end.
/// The original PCM allowance is not a whole-model admission or RSS guarantee.
pub fn process_track<R, P, W>(
    plan: TrackPlan,
    mut read: R,
    mut predict: P,
    mut write: W,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), Error>
where
    R: FnMut(usize, usize, &mut [f32]) -> Result<usize, Error>,
    P: FnMut(&[f32], usize) -> Result<Vec<f32>, Error>,
    W: FnMut(usize, usize, &[f32]) -> Result<(), Error>,
{
    checkpoint(cancelled)?;
    let mut buffers = Buffers::new()?;
    let mut position = 0;
    let mut base = 0;
    while position < plan.total {
        let mut locations = [(0, 0); BATCH];
        let mut batch = 0;
        while position < plan.total && batch < BATCH {
            let length = CHUNK.min(plan.total - position);
            buffers.read_chunk(plan, batch, position, length, &mut read, cancelled)?;
            locations[batch] = (position, length);
            position += STEP;
            batch += 1;
        }
        checkpoint(cancelled)?;
        let prediction = predict(&buffers.input[..batch * CHANNELS * CHUNK], batch)?;
        checkpoint(cancelled)?;
        buffers.add(plan, base, &locations[..batch], &prediction, cancelled)?;
        drop(prediction);
        base += buffers.flush(plan, base, position, &mut write, cancelled)?;
    }
    if base != plan.total {
        return Err(Error::Envelope);
    }
    checkpoint(cancelled)
}
