//! Original CPU reduction for the explicitly admitted Torch wheel and layouts.
//!
//! Source: ATen ReduceOps.cpp, cpu/SumKernel.cpp, cpu/ReduceOpsKernel.cpp,
//! cpu/Reduce.h, SharedReduceOps.h and TensorIteratorReduce.cpp at TORCH_GIT.
//! Slices start at the first logical value and store samples in physical order.
//! Callers attest the original dispatch and strides and keep all owners live
//! through return. A failure invalidates the entire output statistic slice.

use super::{BINS, BLOCK, Error, MAX_PAYLOAD_BYTES, checkpoint, finite, host::Statistics};

pub const TORCH_VERSION: &str = "2.12.0+cu130";
pub const TORCH_GIT: &str = "7661cd9c6b841b62b7f411aa52ec51f05457263b";
const LANES: usize = 8;
const LEVELS: usize = 4;
const GROUP: usize = 4;
/// Maximum simultaneously live fixed reduction scratch and scalar state.
pub const WORKING_BYTES: usize = size_of::<[[[f32; LANES]; GROUP]; LEVELS]>()
    + size_of::<[f32; LANES]>()
    + size_of::<[f64; 3]>()
    + size_of::<Statistics>();

/// Windows x64, pinned Torch wheel, AVX512-reported CPU, serial intra-op.
/// ATen uses eight float lanes for its sum here because the registered sum
/// kernel falls back to AVX2. Native reproduces the arithmetic order without
/// importing Torch or issuing AVX instructions.
#[derive(Clone, Copy, Debug)]
pub struct CpuProfile(());

impl CpuProfile {
    /// Attest observed metadata of the original CPU f32 execution. The caller
    /// is responsible for identifying the wheel and reduction context.
    pub fn admit(
        torch_version: &str,
        torch_git: &str,
        capability: &str,
        intraop_threads: usize,
    ) -> Option<Self> {
        (cfg!(all(windows, target_arch = "x86_64", target_env = "msvc"))
            && torch_version == TORCH_VERSION
            && torch_git == TORCH_GIT
            && capability == "AVX512"
            && intraop_threads == 1)
            .then_some(Self(()))
    }

    /// Correction-one statistics of post-right-pad [B,C,L], reducing C and L
    /// together for each batch. Supports packed mono/stereo planar and packed
    /// stereo interleaved input. Strides are in f32 elements.
    ///
    /// Additional bytes include all other simultaneously live logical payload.
    /// The 512 MiB limit is separate from allocator and process admission.
    pub fn time(
        self,
        pcm: &[f32],
        [batches, channels, frames]: [usize; 3],
        strides: [usize; 3],
        statistics: &mut [Statistics],
        additional_payload_bytes: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        let item = channels.checked_mul(frames).ok_or(Error::Geometry)?;
        // A singleton batch axis does not participate in address traversal.
        let strides = [
            if batches == 1 { item } else { strides[0] },
            strides[1],
            strides[2],
        ];
        if !(1..=2).contains(&channels)
            || !(strides == [item, frames, 1] || (channels == 2 && strides == [item, 1, channels]))
        {
            return Err(Error::Geometry);
        }
        // TensorIterator moves the stride-one reduced axis innermost and
        // coalesces both reduced axes, so either packed layout reduces in
        // storage order.
        branch(
            pcm,
            batches,
            item,
            statistics,
            additional_payload_bytes,
            cancelled,
        )
    }

    /// Correction-one statistics of original stereo CaC [B,4,2048,T], reducing
    /// all but B. Admit the packed output of the stereo complex-to-channel
    /// reshape. The original mono CaC view has a different reduction layout.
    pub fn cac(
        self,
        features: &[f32],
        [batches, planes, bins, steps]: [usize; 4],
        strides: [usize; 4],
        statistics: &mut [Statistics],
        additional_payload_bytes: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        let plane = bins.checked_mul(steps).ok_or(Error::Geometry)?;
        let item = planes.checked_mul(plane).ok_or(Error::Geometry)?;
        if planes != 4 || bins != BINS || strides != [item, plane, steps, 1] {
            return Err(Error::Geometry);
        }
        branch(
            features,
            batches,
            item,
            statistics,
            additional_payload_bytes,
            cancelled,
        )
    }

    /// Statistics of Native packed planar [B,C,part,F,T] features. This adapter
    /// reproduces the original CaC reduction traversal, not the physical strides
    /// of this buffer. Stereo is packed in both representations. Original mono
    /// has part stride 1, frequency stride 2 and time stride 4098 (Nyquist gap):
    /// TensorIterator coalesces part/frequency and SumKernel accumulates one
    /// 4096-element vector cascade per time row. No gather allocation is needed.
    pub fn planar_cac(
        self,
        features: &[f32],
        [batches, channels, bins, steps]: [usize; 4],
        statistics: &mut [Statistics],
        additional_payload_bytes: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        if bins != BINS || steps == 0 {
            return Err(Error::Geometry);
        }
        let row = 2 * BINS;
        let item = row.checked_mul(steps).ok_or(Error::Geometry)?;
        if channels == 2 {
            let item = item.checked_mul(2).ok_or(Error::Geometry)?;
            return self.cac(
                features,
                [batches, 4, bins, steps],
                [item, bins * steps, steps, 1],
                statistics,
                additional_payload_bytes,
                cancelled,
            );
        }
        if channels != 1
            || bins != BINS
            || steps == 0
            || batches == 0
            || statistics.len() != batches
            || batches.checked_mul(item) != Some(features.len())
        {
            return Err(Error::Geometry);
        }
        guard_payload(features.len(), batches, additional_payload_bytes)?;
        for (values, stats) in features.chunks_exact(item).zip(statistics) {
            finite(values, cancelled)?;
            let sample = |i: usize| {
                let step = i / row;
                let inner = i % row;
                values[((inner % 2) * BINS + inner / 2) * steps + step]
            };
            let mut sum = 0.0_f32;
            for step in 0..steps {
                sum += sum_row::<LANES>(row, &|i| sample(step * row + i), cancelled)?;
            }
            *stats = moments(item, &sample, sum / item as f32, batches == 1, cancelled)?;
        }
        checkpoint(cancelled)
    }

    /// Original CLI [C,L] reference. First reduce channels for each sample,
    /// then separately compute global mean and correction-one std of that
    /// reference. No epsilon is used by CLI and zero std is refused.
    pub fn cli(
        self,
        pcm: &[f32],
        [channels, frames]: [usize; 2],
        [channel_stride, frame_stride]: [usize; 2],
        additional_payload_bytes: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Statistics, Error> {
        if !(1..=2).contains(&channels)
            || frames < 2
            || channels.checked_mul(frames) != Some(pcm.len())
            || !([channel_stride, frame_stride] == [frames, 1]
                || (channels == 2 && [channel_stride, frame_stride] == [1, 2]))
        {
            return Err(Error::Geometry);
        }
        guard_payload(pcm.len(), 1, additional_payload_bytes)?;
        finite(pcm, cancelled)?;
        // Re-evaluate the same rounded mono sample on each pass. The original
        // uses a separate f32 sum and division, even for two channels.
        let sample = |frame| {
            let i: usize = frame * frame_stride;
            let left = 0.0_f32 + pcm[i];
            if channels == 1 {
                left
            } else {
                (left + pcm[i + channel_stride]) / 2.0
            }
        };
        let stats = reduce(frames, sample, true, cancelled)?;
        if stats.std() == 0.0 {
            return Err(Error::NonFinite);
        }
        checkpoint(cancelled)?;
        Ok(stats)
    }
}

fn guard_payload(samples: usize, statistics: usize, additional: usize) -> Result<(), Error> {
    let stats_bytes = statistics
        .checked_mul(size_of::<Statistics>())
        .ok_or(Error::Geometry)?;
    samples
        .checked_mul(size_of::<f32>())
        .and_then(|n| n.checked_add(stats_bytes))
        .and_then(|n| n.checked_add(WORKING_BYTES))
        .and_then(|n| n.checked_add(additional))
        .filter(|&n| n <= MAX_PAYLOAD_BYTES)
        .map(|_| ())
        .ok_or(Error::Geometry)
}

fn branch(
    input: &[f32],
    batches: usize,
    item: usize,
    statistics: &mut [Statistics],
    additional: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), Error> {
    if item < 2
        || batches == 0
        || statistics.len() != batches
        || batches.checked_mul(item) != Some(input.len())
    {
        return Err(Error::Geometry);
    }
    guard_payload(input.len(), batches, additional)?;
    checkpoint(cancelled)?;
    for (values, stats) in input.chunks_exact(item).zip(statistics) {
        finite(values, cancelled)?;
        *stats = reduce(item, |i| values[i], batches == 1, cancelled)?;
    }
    checkpoint(cancelled)
}

fn reduce(
    count: usize,
    sample: impl Fn(usize) -> f32,
    single_output: bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<Statistics, Error> {
    let sum = if count < LANES {
        sum_row::<1>(count, &sample, cancelled)?
    } else {
        sum_row::<LANES>(count, &sample, cancelled)?
    };
    moments(count, &sample, sum / count as f32, single_output, cancelled)
}

fn moments(
    count: usize,
    sample: &impl Fn(usize) -> f32,
    mean: f32,
    single_output: bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<Statistics, Error> {
    if !mean.is_finite() {
        return Err(Error::NonFinite);
    }

    let mut squares = 0.0_f64;
    let mut center = if single_output { f64::from(mean) } else { 0.0 };
    for i in 0..count {
        if i.is_multiple_of(BLOCK) {
            checkpoint(cancelled)?;
        }
        let value = f64::from(sample(i));
        let delta = value - center;
        if !single_output {
            // Multiple results dispatch Welford per item. The mean returned to
            // the caller still comes from the separate f32 cascade.
            center += delta / (i + 1) as f64;
        }
        squares += delta * (value - center);
    }
    let std = (squares / (count - 1) as f64).sqrt() as f32;
    let stats = Statistics::from_reference(mean, std)?;
    checkpoint(cancelled)?;
    Ok(stats)
}

// SumKernel's four interleaved accumulators and four carry levels. W=1
// represents the scalar inner kernel and W=8 its vector inner kernel.
fn sum_row<const W: usize>(
    count: usize,
    sample: &impl Fn(usize) -> f32,
    cancelled: &dyn Fn() -> bool,
) -> Result<f32, Error> {
    let vectors = count / W;
    let groups = vectors / GROUP;
    let ceil_log2 = usize::BITS - groups.saturating_sub(1).leading_zeros();
    let level_power = (ceil_log2 / LEVELS as u32).max(4);
    let mask = (1usize << level_power) - 1;
    let mut levels = [[[0.0_f32; W]; GROUP]; LEVELS];
    for group in 0..groups {
        if group.is_multiple_of(BLOCK / (GROUP * W)) {
            checkpoint(cancelled)?;
        }
        for (part, lanes) in levels[0].iter_mut().enumerate() {
            for (lane, value) in lanes.iter_mut().enumerate() {
                *value += sample((group * GROUP + part) * W + lane);
            }
        }
        let completed = group + 1;
        if completed & mask == 0 {
            for level in 1..LEVELS {
                let (lower, upper) = levels.split_at_mut(level);
                for (from, to) in lower[level - 1]
                    .iter_mut()
                    .flatten()
                    .zip(upper[0].iter_mut().flatten())
                {
                    *to += *from;
                    *from = 0.0;
                }
                if completed & (mask << (level as u32 * level_power)) != 0 {
                    break;
                }
            }
        }
    }
    let (lowest, upper) = levels.split_first_mut().expect("fixed cascade levels");
    for carry in upper {
        for (partial, &value) in lowest.iter_mut().flatten().zip(carry.iter().flatten()) {
            *partial += value;
        }
    }
    let (first, rest) = lowest.split_first_mut().expect("fixed accumulator group");
    for vector in groups * GROUP..vectors {
        for (lane, value) in first.iter_mut().enumerate() {
            *value += sample(vector * W + lane);
        }
    }
    for part in rest {
        for (partial, &value) in first.iter_mut().zip(part.iter()) {
            *partial += value;
        }
    }
    // The scalar tail enters the result before the vector lanes.
    let mut sum = 0.0_f32;
    for i in vectors * W..count {
        sum += sample(i);
    }
    for &value in first.iter() {
        sum += value;
    }
    Ok(0.0 + sum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn profile_rejects_unobserved_original_dispatch() {
        let admit =
            |capability, threads| CpuProfile::admit(TORCH_VERSION, TORCH_GIT, capability, threads);
        assert!(admit("AVX2", 1).is_none());
        assert!(admit("AVX512", 16).is_none());
        assert!(CpuProfile::admit(TORCH_VERSION, "other", "AVX512", 1).is_none());
        assert_eq!(
            admit("AVX512", 1).is_some(),
            cfg!(all(windows, target_arch = "x86_64", target_env = "msvc"))
        );
    }

    #[test]
    fn refuses_layouts_and_total_payload_before_read() {
        let profile = CpuProfile(());
        let pcm = [1.0, 2.0, 3.0, 4.0];
        let stats = Statistics::from_reference(0.0, 0.0).unwrap();
        let mut out = [stats];
        let no_poll = || panic!("must refuse before dispatch");
        assert!(matches!(
            profile.planar_cac(&pcm, [1, 2, usize::MAX, 2], &mut out, 0, &no_poll),
            Err(Error::Geometry)
        ));
        assert!(matches!(
            profile.time(&pcm, [1, 2, 2], [8, 4, 1], &mut out, 0, &no_poll),
            Err(Error::Geometry)
        ));
        assert!(matches!(
            profile.cac(
                &pcm,
                [1, 2, BINS, 1],
                [2, 1, 2, 4098],
                &mut out,
                0,
                &no_poll
            ),
            Err(Error::Geometry)
        ));
        let allowance = MAX_PAYLOAD_BYTES - WORKING_BYTES - size_of_val(&pcm) - size_of_val(&out);
        assert!(matches!(
            profile.time(
                &pcm,
                [1, 2, 2],
                [4, 2, 1],
                &mut out,
                allowance + 1,
                &no_poll
            ),
            Err(Error::Geometry)
        ));
        profile
            .time(&pcm, [1, 2, 2], [4, 2, 1], &mut out, allowance, &|| false)
            .unwrap();
        assert!(matches!(
            profile.cli(&pcm, [2, 2], [2, 1], usize::MAX, &no_poll),
            Err(Error::Geometry)
        ));
        assert!(matches!(
            profile.cli(&[1.0, 2.0], [2, 1], [1, 1], 0, &no_poll),
            Err(Error::Geometry)
        ));
        assert!(matches!(
            profile.cli(&[2.0; 4], [2, 2], [2, 1], 0, &|| false),
            Err(Error::NonFinite)
        ));
    }

    #[test]
    fn cancels_during_reduction_and_at_return_and_keeps_first_failure() {
        let profile = CpuProfile(());
        let mut pcm = [2.0_f32; 8200];
        let zero = Statistics::from_reference(0.0, 0.0).unwrap();
        let mut out = [zero; 2];
        let calls = Cell::new(0);
        let stop = Cell::new(usize::MAX);
        let cancelled = || {
            calls.set(calls.get() + 1);
            calls.get() >= stop.get()
        };
        let run = |input: &[f32], out: &mut [Statistics]| {
            profile.time(input, [2, 1, 4100], [4100, 4100, 1], out, 0, &cancelled)
        };
        run(&pcm, &mut out).unwrap();
        let final_poll = calls.get();
        for at in [5, final_poll] {
            calls.set(0);
            stop.set(at);
            assert_eq!(run(&pcm, &mut out), Err(Error::Cancelled));
        }
        pcm[1] = f32::NAN;
        calls.set(0);
        stop.set(3);
        assert_eq!(run(&pcm, &mut out), Err(Error::NonFinite));
        pcm[1] = 2.0;
        calls.set(0);
        stop.set(usize::MAX);
        run(&pcm, &mut out).unwrap();
        assert!(out.iter().all(|s| s.mean() == 2.0 && s.std() == 0.0));
    }
}
