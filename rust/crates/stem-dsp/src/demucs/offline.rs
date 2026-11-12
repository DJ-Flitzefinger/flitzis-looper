//! Independent offline CPU normalization through the existing apply/host/hybrid
//! boundaries. This composes supplied offsets and weighted bags with a synchronous
//! normalized-core predictor; it supplies no learned model, runtime or routing.

use super::{
    Error, checkpoint,
    host::{Statistics, apply},
    hybrid,
    reduction::CpuProfile,
    zeros,
};

/// The model index identifies the normalized core. All controls, original source,
/// predictor state and admission/readers remain live through the enclosing run.
pub type Predictor<'a> = dyn FnMut(
        usize,
        hybrid::Geometry,
        &[f32],
        &[f32],
        &mut [f32],
        &mut [f32],
        &dyn Fn() -> bool,
    ) -> Result<(), Error>
    + 'a;

pub struct Workspace<'a> {
    apply: apply::Workspace<'a>,
    hybrid: Vec<hybrid::Workspace>,
    normalized: Vec<f32>,
    profile: CpuProfile,
    frames: usize,
    channels: usize,
    payload_bytes: usize,
}

impl<'a> Workspace<'a> {
    /// Preflight the complete simultaneous union before any vector allocation.
    /// Additional bytes include all predictor allocations and other live readers.
    /// FFT plans, allocator and process/runtime/WHOLE admission remain separate.
    pub fn new(
        frames: usize,
        models: &'a [apply::Model<'a>],
        profile: CpuProfile,
        additional_payload_bytes: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, Error> {
        let bytes =
            Self::planned_payload_bytes(frames, models, additional_payload_bytes, cancelled)?;
        let channels = models[0].channels;
        let mut hybrid = Vec::new();
        hybrid
            .try_reserve_exact(models.len())
            .map_err(|_| Error::Allocation {
                phase: "demucs.offline.models",
                requested_bytes: models.len() * size_of::<hybrid::Workspace>(),
            })?;
        for model in models {
            let base = hybrid::Geometry::new(
                1,
                channels,
                model.sources.len(),
                model.training_frames,
                model.training_frames,
                0,
            )?;
            hybrid.push(hybrid::Workspace::new(
                hybrid::Geometry::new(
                    1,
                    channels,
                    model.sources.len(),
                    model.training_frames,
                    model.training_frames,
                    bytes - base.payload_bytes(),
                )?,
                cancelled,
            )?);
        }
        let work = Self {
            apply: apply::Workspace::new(frames, models, cancelled)?,
            hybrid,
            normalized: zeros(channels * frames, "demucs.offline.normalized")?,
            profile,
            frames,
            channels,
            payload_bytes: bytes,
        };
        checkpoint(cancelled)?;
        Ok(work)
    }

    pub fn planned_payload_bytes(
        frames: usize,
        models: &[apply::Model<'_>],
        additional_payload_bytes: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<usize, Error> {
        let mut bytes = apply::Workspace::planned_payload_bytes(frames, models, cancelled)?;
        let channels = models[0].channels;
        if !(1..=2).contains(&channels) {
            return Err(Error::Geometry);
        }
        let copy = frames
            .checked_mul(channels)
            .and_then(|v| v.checked_mul(size_of::<f32>()))
            .ok_or(Error::Geometry)?;
        bytes = bytes
            .checked_add(copy)
            .and_then(|v| v.checked_add(std::mem::size_of_val(models)))
            .and_then(|v| v.checked_add(size_of::<Statistics>()))
            .and_then(|v| v.checked_add(size_of::<Self>()))
            .and_then(|v| v.checked_add(additional_payload_bytes))
            .ok_or(Error::Geometry)?;
        for model in models {
            bytes = bytes
                .checked_add(std::mem::size_of_val(model.sources))
                .ok_or(Error::Geometry)?;
            for name in model.sources {
                checkpoint(cancelled)?;
                bytes = bytes.checked_add(name.len()).ok_or(Error::Geometry)?;
            }
            let g = hybrid::Geometry::new(
                1,
                channels,
                model.sources.len(),
                model.training_frames,
                model.training_frames,
                0,
            )?;
            // These exact callback input/output buffers already belong to Track.
            let internal = g.payload_bytes() - (g.input_len() + g.output_len()) * size_of::<f32>();
            bytes = bytes
                .checked_add(internal)
                .and_then(|v| v.checked_add(size_of::<hybrid::Workspace>()))
                .ok_or(Error::Geometry)?;
        }
        checkpoint(cancelled)?;
        Ok(bytes)
    }

    pub fn payload_bytes(&self) -> usize {
        self.payload_bytes
    }

    /// Independent HT branch statistics on every full training callback input.
    /// Existing apply retains physical source context, shifts, split, center trim
    /// and bag order. An error or late cancel invalidates the entire output.
    pub fn run(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        predictor: &mut Predictor<'_>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        let profile = self.profile;
        let models = &mut self.hybrid;
        self.apply.run(
            input,
            output,
            &mut |index, chunk, pcm, estimate| {
                if pcm.len() != self.channels * chunk.training_frames {
                    return Err(Error::Geometry);
                }
                models[index].run_independent(
                    profile,
                    pcm,
                    estimate,
                    &mut |g, spectral, time, decoded_spectral, decoded_time, cancel| {
                        predictor(
                            index,
                            g,
                            spectral,
                            time,
                            decoded_spectral,
                            decoded_time,
                            cancel,
                        )
                    },
                    cancelled,
                )
            },
            cancelled,
        )
    }

    /// Original CLI channel-reference statistics precede ALL shifts/context.
    /// Normalize once without epsilon; restore raw std/mean after the whole bag.
    /// Constant/singleton reference is refused. No original stats are supplied.
    pub fn run_cli(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        predictor: &mut Predictor<'_>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        if input.len() != self.normalized.len() {
            return Err(Error::Geometry);
        }
        let additional = self.payload_bytes
            - std::mem::size_of_val(input)
            - size_of::<Statistics>()
            - super::reduction::WORKING_BYTES;
        let stats = self.profile.cli(
            input,
            [self.channels, self.frames],
            [self.frames, 1],
            additional,
            cancelled,
        )?;
        stats.normalize_into(input, &mut self.normalized, 0.0, cancelled)?;
        let profile = self.profile;
        let models = &mut self.hybrid;
        self.apply.run(
            &self.normalized,
            output,
            &mut |index, _, pcm, estimate| {
                models[index].run_independent(
                    profile,
                    pcm,
                    estimate,
                    &mut |g, spectral, time, decoded_spectral, decoded_time, cancel| {
                        predictor(
                            index,
                            g,
                            spectral,
                            time,
                            decoded_spectral,
                            decoded_time,
                            cancel,
                        )
                    },
                    cancelled,
                )
            },
            cancelled,
        )?;
        // Statistics performs bounded finite/poll checks including final return.
        stats.restore_in_place(output, cancelled)?;
        checkpoint(cancelled)
    }
}

#[cfg(all(test, windows, target_arch = "x86_64", target_env = "msvc"))]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn profile() -> CpuProfile {
        CpuProfile::admit(
            "2.12.0+cu130",
            "7661cd9c6b841b62b7f411aa52ec51f05457263b",
            "AVX512",
            1,
        )
        .unwrap()
    }

    fn model() -> apply::Model<'static> {
        apply::Model {
            sources: &["a", "b"],
            channels: 1,
            samplerate: super::super::RATE,
            segment: 7,
            overlap: 0.25,
            transition_power: 1.0,
            valid_frames: 9,
            training_frames: 13,
            split: true,
            offsets: &[],
            weights: &[1.0, 1.0],
        }
    }

    #[test]
    fn checks_payload_union_overflow_and_channels_before_allocation() {
        let models = [model(), model()];
        let bytes = Workspace::planned_payload_bytes(17, &models, 0, &|| false).unwrap();
        let external = 512 * 1024 * 1024;
        assert_eq!(
            Workspace::planned_payload_bytes(17, &models, external, &|| false).unwrap(),
            bytes + external
        );
        assert!(matches!(
            Workspace::new(17, &models, profile(), usize::MAX, &|| false),
            Err(Error::Geometry)
        ));
        let one = Workspace::planned_payload_bytes(17, &models[..1], 0, &|| false).unwrap();
        assert!(bytes > one + size_of::<hybrid::Workspace>());
        let mut unsupported = model();
        unsupported.channels = 3;
        assert!(matches!(
            Workspace::new(17, &[unsupported], profile(), 0, &|| false),
            Err(Error::Geometry)
        ));
        let mut zero_weights = model();
        zero_weights.weights = &[0.0, 0.0];
        assert!(matches!(
            Workspace::planned_payload_bytes(17, &[zero_weights], 0, &|| false),
            Err(Error::NonFinite)
        ));
    }

    #[test]
    fn cli_undefined_reference_and_callback_failure_do_not_publish() {
        let models = [model()];
        let mut work = Workspace::new(17, &models, profile(), 0, &|| false).unwrap();
        let mut out = [0.0; 34];
        let mut unreachable =
            |_, _, _: &[_], _: &[_], _: &mut [_], _: &mut [_], _: &dyn Fn() -> bool| panic!();
        assert_eq!(
            work.run_cli(&[3.0; 17], &mut out, &mut unreachable, &|| false),
            Err(Error::NonFinite)
        );
        let input: Vec<_> = (0..17).map(|i| (i * i) as f32 * 0.125).collect();
        let mut failure =
            |_, _, _: &[_], _: &[_], _: &mut [_], _: &mut [_], _: &dyn Fn() -> bool| {
                Err(Error::Envelope)
            };
        assert_eq!(
            work.run_cli(&input, &mut out, &mut failure, &|| false),
            Err(Error::Envelope)
        );
        let mut incomplete =
            |_, _, _: &[_], _: &[_], ds: &mut [f32], dt: &mut [f32], _: &dyn Fn() -> bool| {
                ds.fill(0.0);
                let last = dt.len() - 1;
                dt[..last].fill(0.0);
                Ok(())
            };
        assert_eq!(
            work.run(&input, &mut out, &mut incomplete, &|| false),
            Err(Error::NonFinite)
        );
    }

    #[test]
    fn first_failure_and_final_cancellation_allow_workspace_reuse() {
        let models = [model()];
        let mut work = Workspace::new(17, &models, profile(), 0, &|| false).unwrap();
        let input: Vec<_> = (0..17).map(|i| i as f32).collect();
        let mut out = [0.0; 34];
        let polls = Cell::new(0);
        let at = Cell::new(usize::MAX);
        let cancel = || {
            polls.set(polls.get() + 1);
            polls.get() == at.get()
        };
        let mut fill = |_,
                        g: hybrid::Geometry,
                        _: &[_],
                        _: &[_],
                        ds: &mut [f32],
                        dt: &mut [f32],
                        _: &dyn Fn() -> bool| {
            assert_eq!(g.training_frames(), 13);
            ds.fill(0.0);
            dt.fill(0.0);
            Ok(())
        };
        work.run_cli(&input, &mut out, &mut fill, &cancel).unwrap();
        assert!(out.iter().all(|x| x.is_finite()));
        at.set(polls.get());
        polls.set(0);
        assert_eq!(
            work.run_cli(&input, &mut out, &mut fill, &cancel),
            Err(Error::Cancelled)
        );
        work.run_cli(&input, &mut out, &mut fill, &|| false)
            .unwrap();
    }
}
