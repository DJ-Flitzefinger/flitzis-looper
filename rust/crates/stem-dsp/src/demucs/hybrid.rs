//! Offline CPU HTDemucs CaC evaluation at the forward boundary.
//!
//! The input is planar [batch, channel, physical frame] at 44100 Hz after any
//! apply/TensorChunk context padding. The caller supplies the training length and
//! either supplies original statistics or explicitly admits a CPU profile to
//! measure the real Native right-padded waveform and spectrum independently.
//! The final prefix crop belongs to forward. Any outer center trim remains with
//! apply. The predictor supplies both decoder outputs synchronously.

use super::{
    BLOCK, Error, Geometry as SpectralGeometry, MAX_WORKING_BYTES, RATE,
    Workspace as SpectralWorkspace, checkpoint,
    host::Statistics,
    reduction::{self, CpuProfile},
    zeros,
};

#[derive(Clone, Copy, Debug)]
pub struct BranchStatistics {
    /// Original correction-one reduction over (channel, part, frequency, step).
    pub spectral: Statistics,
    /// Original correction-one reduction over (channel, training frame).
    pub time: Statistics,
}

#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    batches: usize,
    channels: usize,
    sources: usize,
    physical_frames: usize,
    analysis: SpectralGeometry,
    inverse_lane: SpectralGeometry,
    payload_bytes: usize,
}

impl Geometry {
    /// The caller supplies the peak simultaneous logical bytes of the predictor,
    /// other live readers and other buffers in addition to these explicit owners.
    /// The checked logical extent excludes FFT plans, allocator overhead and
    /// process/runtime backing.
    pub fn new(
        batches: usize,
        channels: usize,
        sources: usize,
        physical_frames: usize,
        training_frames: usize,
        additional_payload_bytes: usize,
    ) -> Result<Self, Error> {
        let input_lanes = batches.checked_mul(channels).ok_or(Error::Geometry)?;
        let output_lanes = input_lanes.checked_mul(sources).ok_or(Error::Geometry)?;
        // Original correction-one waveform std is undefined for one scalar.
        if output_lanes == 0
            || physical_frames == 0
            || physical_frames > training_frames
            || (channels == 1 && training_frames == 1)
        {
            return Err(Error::Geometry);
        }
        let analysis = SpectralGeometry::new(training_frames, input_lanes, RATE)?;
        let inverse_lane = SpectralGeometry::new(training_frames, 1, RATE)?;
        let physical_input = input_lanes
            .checked_mul(physical_frames)
            .ok_or(Error::Geometry)?;
        let physical_output = output_lanes
            .checked_mul(physical_frames)
            .ok_or(Error::Geometry)?;
        let decoded_time = output_lanes
            .checked_mul(training_frames)
            .ok_or(Error::Geometry)?;
        let decoded_spectral = output_lanes
            .checked_mul(inverse_lane.feature_len())
            .ok_or(Error::Geometry)?;
        let floats = [
            physical_input,
            physical_output,
            analysis.pcm_len(),
            analysis.feature_len(),
            decoded_time,
            decoded_spectral,
            training_frames,
        ]
        .into_iter()
        .try_fold(0usize, usize::checked_add)
        .ok_or(Error::Geometry)?;
        let stats = batches
            .checked_mul(size_of::<BranchStatistics>() + 2 * size_of::<Statistics>())
            .ok_or(Error::Geometry)?;
        let payload_bytes = floats
            .checked_mul(size_of::<f32>())
            .and_then(|n| n.checked_add(stats))
            .and_then(|n| n.checked_add(MAX_WORKING_BYTES))
            .and_then(|n| n.checked_add(reduction::WORKING_BYTES))
            .and_then(|n| n.checked_add(additional_payload_bytes))
            .ok_or(Error::Geometry)?;
        Ok(Self {
            batches,
            channels,
            sources,
            physical_frames,
            analysis,
            inverse_lane,
            payload_bytes,
        })
    }

    pub fn batches(self) -> usize {
        self.batches
    }
    pub fn channels(self) -> usize {
        self.channels
    }
    pub fn sources(self) -> usize {
        self.sources
    }
    pub fn physical_frames(self) -> usize {
        self.physical_frames
    }
    pub fn training_frames(self) -> usize {
        self.analysis.frames()
    }
    pub fn spectral_steps(self) -> usize {
        self.analysis.steps()
    }
    pub fn input_len(self) -> usize {
        self.analysis.lanes() * self.physical_frames
    }
    pub fn output_len(self) -> usize {
        self.input_len() * self.sources
    }
    /// Simultaneous logical extent, including the caller's declared payload.
    pub fn payload_bytes(self) -> usize {
        self.payload_bytes
    }
}

/// Given geometry, read normalized spectral [B,C,2,F,T] and time [B,C,L],
/// and write every value of spectral [B,S,C,2,F,T] and time [B,S,C,L].
/// The adjacent complex parts are real then imaginary. This is the full
/// predicted CaC spectrum. Return the first failure and poll cancellation
/// during callback work. All borrowed owners remain live until return.
pub type Predictor<'a> = dyn FnMut(Geometry, &[f32], &[f32], &mut [f32], &mut [f32], &dyn Fn() -> bool) -> Result<(), Error>
    + 'a;

pub struct Workspace {
    geometry: Geometry,
    fft: SpectralWorkspace,
    spectral: Vec<f32>,
    time: Vec<f32>,
    decoded_spectral: Vec<f32>,
    decoded_time: Vec<f32>,
    inverse: Vec<f32>,
    spectral_statistics: Vec<Statistics>,
    time_statistics: Vec<Statistics>,
}

impl Workspace {
    pub fn new(g: Geometry, cancelled: &dyn Fn() -> bool) -> Result<Self, Error> {
        checkpoint(cancelled)?;
        let work = Self {
            geometry: g,
            fft: SpectralWorkspace::new(cancelled)?,
            spectral: zeros(g.analysis.feature_len(), "demucs.hybrid.spectral")?,
            time: zeros(g.analysis.pcm_len(), "demucs.hybrid.time")?,
            decoded_spectral: zeros(
                g.analysis.feature_len() * g.sources,
                "demucs.hybrid.decoded_spectral",
            )?,
            decoded_time: zeros(
                g.analysis.pcm_len() * g.sources,
                "demucs.hybrid.decoded_time",
            )?,
            inverse: zeros(g.training_frames(), "demucs.hybrid.inverse")?,
            spectral_statistics: zeros(g.batches, "demucs.hybrid.spectral_statistics")?,
            time_statistics: zeros(g.batches, "demucs.hybrid.time_statistics")?,
        };
        checkpoint(cancelled)?;
        Ok(work)
    }

    /// Compute one eval forward with no host allocations during the call.
    /// Statistics must come from the caller's exact original-dispatch input,
    /// including right training padding. This call validates the provided extent
    /// and arithmetic, while the caller establishes statistic provenance.
    /// Any error invalidates the entire output, including any prefix written.
    pub fn run(
        &mut self,
        pcm: &[f32],
        statistics: &[BranchStatistics],
        output: &mut [f32],
        predictor: &mut Predictor<'_>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        let g = self.geometry;
        if pcm.len() != g.input_len()
            || output.len() != g.output_len()
            || statistics.len() != g.batches
        {
            return Err(Error::Geometry);
        }
        self.prepare(pcm, cancelled)?;
        self.evaluate(Some(statistics), output, predictor, cancelled)
    }

    /// Measure both real Native branches using the explicitly admitted original
    /// CPU profile. No original tensor/statistic is supplied and no mathematical
    /// or supplied-stat fallback occurs. FFT differences remain independent of
    /// same-tensor reducer parity. Any error invalidates the whole result.
    pub fn run_independent(
        &mut self,
        profile: CpuProfile,
        pcm: &[f32],
        output: &mut [f32],
        predictor: &mut Predictor<'_>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        let g = self.geometry;
        if pcm.len() != g.input_len()
            || output.len() != g.output_len()
            || !(1..=2).contains(&g.channels)
        {
            return Err(Error::Geometry);
        }
        self.prepare(pcm, cancelled)?;
        // Geometry already includes every live owner, including the conservative
        // borrowed-stat allowance. Subtract only owners counted by the reducer.
        let additional = |input: usize| {
            g.payload_bytes
                - input * size_of::<f32>()
                - g.batches * size_of::<Statistics>()
                - reduction::WORKING_BYTES
        };
        profile.time(
            &self.time,
            [g.batches, g.channels, g.training_frames()],
            [g.channels * g.training_frames(), g.training_frames(), 1],
            &mut self.time_statistics,
            additional(self.time.len()),
            cancelled,
        )?;
        profile.planar_cac(
            &self.spectral,
            [g.batches, g.channels, super::BINS, g.spectral_steps()],
            &mut self.spectral_statistics,
            additional(self.spectral.len()),
            cancelled,
        )?;
        self.evaluate(None, output, predictor, cancelled)
    }

    /// Statistics are valid only after a successful independent run.
    pub fn independent_statistics(&self) -> (&[Statistics], &[Statistics]) {
        (&self.spectral_statistics, &self.time_statistics)
    }

    fn prepare(&mut self, pcm: &[f32], cancelled: &dyn Fn() -> bool) -> Result<(), Error> {
        let g = self.geometry;
        let physical = g.physical_frames;
        let training = g.training_frames();
        for (src, dst) in pcm
            .chunks_exact(physical)
            .zip(self.time.chunks_exact_mut(training))
        {
            let (prefix, padding) = dst.split_at_mut(physical);
            for (input, copy) in src.chunks(BLOCK).zip(prefix.chunks_mut(BLOCK)) {
                checkpoint(cancelled)?;
                copy.copy_from_slice(input);
            }
            for block in padding.chunks_mut(BLOCK) {
                checkpoint(cancelled)?;
                block.fill(0.0);
            }
        }
        self.fft
            .analyze_into(g.analysis, &self.time, &mut self.spectral, cancelled)?;
        checkpoint(cancelled)
    }

    fn evaluate(
        &mut self,
        statistics: Option<&[BranchStatistics]>,
        output: &mut [f32],
        predictor: &mut Predictor<'_>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        let g = self.geometry;
        let physical = g.physical_frames;
        let training = g.training_frames();
        let spectral_item = g.channels * g.inverse_lane.feature_len();
        let time_item = g.channels * training;
        for batch in 0..g.batches {
            let stats = self.statistics(statistics, batch);
            stats.spectral.normalize_branch_in_place(
                &mut self.spectral[batch * spectral_item..(batch + 1) * spectral_item],
                cancelled,
            )?;
            stats.time.normalize_branch_in_place(
                &mut self.time[batch * time_item..(batch + 1) * time_item],
                cancelled,
            )?;
        }

        for decoded in [&mut self.decoded_spectral, &mut self.decoded_time] {
            for block in decoded.chunks_mut(BLOCK) {
                checkpoint(cancelled)?;
                block.fill(f32::NAN);
            }
        }
        checkpoint(cancelled)?;
        predictor(
            g,
            &self.spectral,
            &self.time,
            &mut self.decoded_spectral,
            &mut self.decoded_time,
            cancelled,
        )?;
        checkpoint(cancelled)?;

        let spectral_item = spectral_item * g.sources;
        let time_item = time_item * g.sources;
        for batch in 0..g.batches {
            let stats = self.statistics(statistics, batch);
            stats.spectral.restore_in_place(
                &mut self.decoded_spectral[batch * spectral_item..(batch + 1) * spectral_item],
                cancelled,
            )?;
            stats.time.restore_in_place(
                &mut self.decoded_time[batch * time_item..(batch + 1) * time_item],
                cancelled,
            )?;
        }
        for ((spectrum, time), physical_output) in self
            .decoded_spectral
            .chunks_exact(g.inverse_lane.feature_len())
            .zip(self.decoded_time.chunks_exact_mut(training))
            .zip(output.chunks_exact_mut(physical))
        {
            self.fft
                .reconstruct_into(g.inverse_lane, spectrum, &mut self.inverse, cancelled)?;
            // Sum every training sample, including the part the forward crop drops.
            for (time, inverse) in time.chunks_mut(BLOCK).zip(self.inverse.chunks(BLOCK)) {
                checkpoint(cancelled)?;
                for (sample, &spectral) in time.iter_mut().zip(inverse) {
                    *sample += spectral;
                    if !sample.is_finite() {
                        return Err(Error::NonFinite);
                    }
                }
            }
            for (src, dst) in time[..physical]
                .chunks(BLOCK)
                .zip(physical_output.chunks_mut(BLOCK))
            {
                checkpoint(cancelled)?;
                dst.copy_from_slice(src);
            }
        }
        checkpoint(cancelled)
    }

    fn statistics(&self, supplied: Option<&[BranchStatistics]>, batch: usize) -> BranchStatistics {
        supplied.map_or_else(
            || BranchStatistics {
                spectral: self.spectral_statistics[batch],
                time: self.time_statistics[batch],
            },
            |stats| stats[batch],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn stats() -> [BranchStatistics; 1] {
        [BranchStatistics {
            spectral: Statistics::from_reference(0.0, 0.0).unwrap(),
            time: Statistics::from_reference(2.0, 0.5).unwrap(),
        }]
    }

    #[test]
    fn checks_simultaneous_payload_geometry_before_construction() {
        let g = Geometry::new(2, 2, 4, 3, 5, 0).unwrap();
        assert_eq!(
            (g.input_len(), g.output_len(), g.spectral_steps()),
            (12, 48, 1)
        );
        assert!(Geometry::new(1, 1, 1, 1, 1, 0).is_err());
        assert!(Geometry::new(1, 2, 1, 2, 1, 0).is_err());
        assert!(Geometry::new(usize::MAX, 2, 1, 1, 2, 0).is_err());
        assert!(Geometry::new(1, 2, 1, 1, 2, usize::MAX).is_err());
        assert!(SpectralGeometry::new(1_600_001, 2, RATE).is_ok());
        let large = Geometry::new(1, 2, 12, 1_600_001, 1_600_001, 0).unwrap();
        assert!(large.payload_bytes() > 512 * 1024 * 1024);
    }

    #[test]
    fn rejects_shape_failure_and_nonfinite_or_unwritten_decoder_tails() {
        let g = Geometry::new(1, 1, 2, 3, 5, 0).unwrap();
        let mut work = Workspace::new(g, &|| false).unwrap();
        let pcm = [1.0, 2.0, 3.0];
        let stats = stats();
        let mut out = [0.0; 6];
        let mut unreachable =
            |_, _: &[_], _: &[_], _: &mut [_], _: &mut [_], _: &dyn Fn() -> bool| panic!();
        assert_eq!(
            work.run(&pcm, &[], &mut out, &mut unreachable, &|| false),
            Err(Error::Geometry)
        );
        assert_eq!(
            work.run(&[f32::NAN; 3], &stats, &mut out, &mut unreachable, &|| {
                false
            }),
            Err(Error::NonFinite)
        );

        let mut error = |_, _: &[_], _: &[_], _: &mut [_], _: &mut [_], _: &dyn Fn() -> bool| {
            Err(Error::Envelope)
        };
        assert_eq!(
            work.run(&pcm, &stats, &mut out, &mut error, &|| false),
            Err(Error::Envelope)
        );

        for leave_spectral_unwritten in [false, true] {
            let mut partial = |_,
                               _: &[_],
                               _: &[_],
                               spectral: &mut [f32],
                               time: &mut [f32],
                               _: &dyn Fn() -> bool| {
                time.fill(2.0);
                if leave_spectral_unwritten {
                    let tail = spectral.len() - 1;
                    spectral[..tail].fill(1.0);
                } else {
                    spectral.fill(1.0);
                    // Outside the physical crop but still part of forward.
                    *time.last_mut().unwrap() = f32::INFINITY;
                }
                Ok(())
            };
            assert_eq!(
                work.run(&pcm, &stats, &mut out, &mut partial, &|| false),
                Err(Error::NonFinite)
            );
        }
        let mut unwritten =
            |_, _: &[_], _: &[_], spectral: &mut [f32], time: &mut [f32], _: &dyn Fn() -> bool| {
                spectral.fill(0.0);
                let tail = time.len() - 1;
                time[..tail].fill(0.0);
                Ok(())
            };
        assert_eq!(
            work.run(&pcm, &stats, &mut out, &mut unwritten, &|| false),
            Err(Error::NonFinite)
        );
    }

    #[test]
    fn cancellation_after_callback_and_at_final_check_allows_reuse() {
        let g = Geometry::new(1, 1, 2, 3, 5, 0).unwrap();
        let mut work = Workspace::new(g, &|| false).unwrap();
        let mut out = [0.0; 6];
        let calls = Cell::new(0);
        let cancel_at = Cell::new(usize::MAX);
        let cancelled = || {
            calls.set(calls.get() + 1);
            calls.get() == cancel_at.get()
        };
        let mut fill = |_,
                        _: &[_],
                        time_input: &[f32],
                        spectral: &mut [f32],
                        time: &mut [f32],
                        _: &dyn Fn() -> bool| {
            assert_eq!(time_input[4], -2.0 / (0.5 + 1e-5));
            spectral.fill(2.0);
            time.fill(4.0);
            Ok(())
        };
        work.run(&[1.0, 2.0, 3.0], &stats(), &mut out, &mut fill, &cancelled)
            .unwrap();
        assert_eq!(out, [4.0; 6]);
        cancel_at.set(calls.get());
        calls.set(0);
        assert_eq!(
            work.run(&[1.0, 2.0, 3.0], &stats(), &mut out, &mut fill, &cancelled),
            Err(Error::Cancelled)
        );
        calls.set(0);
        let mut late = |_, _: &[_], _: &[_], _: &mut [_], _: &mut [_], _: &dyn Fn() -> bool| {
            cancel_at.set(calls.get() + 1);
            Ok(())
        };
        assert_eq!(
            work.run(&[1.0, 2.0, 3.0], &stats(), &mut out, &mut late, &cancelled),
            Err(Error::Cancelled)
        );
        work.run(&[1.0, 2.0, 3.0], &stats(), &mut out, &mut fill, &|| false)
            .unwrap();
        assert_eq!(out, [4.0; 6]);
    }
}
