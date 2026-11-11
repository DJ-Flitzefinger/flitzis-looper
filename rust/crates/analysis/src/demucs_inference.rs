//! Opt-in, synchronous CPU inference for the original HTDemucs release core.
//! The owner provides a fresh disposable process, admitted ORT and readers.

use crate::{
    trial_lifetime::{Context, retain_first, terminate_during},
    trial_ort,
};
use flitzis_stem_dsp::demucs::{
    BINS, Error, MAX_PAYLOAD_BYTES, RATE,
    host::apply::Model,
    hybrid::{self, Geometry},
    offline,
    reduction::CpuProfile,
};
use ort::{
    ep,
    session::{RunOptions, Session, builder::GraphOptimizationLevel},
    value::{Tensor, TensorElementType, TensorRef, ValueType},
};
use std::{cell::Cell, path::Path};

pub const FRAMES: usize = 343_980;
pub const STEPS: usize = 336;
pub const SOURCES: [&str; 4] = ["drums", "bass", "other", "vocals"];
pub const CONTRACT: &str = "htdemucs-4.0.1-955717e8-stereo-b1-v1";
pub const CHECKPOINT: &str = "8726e21a993978c7ba086d3872e7608d7d5bfca646ca4aca459ffda844faa8b4";
pub const MAX_GRAPH_BYTES: usize = 192 * 1024 * 1024;
const SPEC_IN: [usize; 5] = [1, 2, 2, BINS, STEPS];
const TIME_IN: [usize; 3] = [1, 2, FRAMES];
const SPEC_OUT: [usize; 6] = [1, SOURCES.len(), 2, 2, BINS, STEPS];
const TIME_OUT: [usize; 4] = [1, SOURCES.len(), 2, FRAMES];
pub const SPEC_INPUT_LEN: usize = 2 * 2 * BINS * STEPS;
pub const TIME_INPUT_LEN: usize = 2 * FRAMES;
pub const SPEC_OUTPUT_LEN: usize = SOURCES.len() * SPEC_INPUT_LEN;
pub const TIME_OUTPUT_LEN: usize = SOURCES.len() * TIME_INPUT_LEN;
const OUTPUT_BYTES: usize = (SPEC_OUTPUT_LEN + TIME_OUTPUT_LEN) * size_of::<f32>();
const INPUT_BYTES: usize = (SPEC_INPUT_LEN + TIME_INPUT_LEN) * size_of::<f32>();
const WEIGHT_BYTES: usize = 167_937_824;
const CANCELLED: &str = "native HTDemucs trial cancelled";
// The current opset-18 recipe lowers attention to MatMul/Softmax/MatMul. Charge
// both live score owners until reuse or a bounded lowering is actually proved.
const DENSE_SCORE_BYTES: usize = 8 * (8 * STEPS) * (8 * STEPS) * size_of::<f32>();
#[cfg(all(windows, feature = "native-demucs-worker"))]
pub const SCAN64_LOWERING: &str = "torchscript-opset18-scan64";
// Two 8 x 64 x 2688 float32 tile owners. All other operator, runtime and
// transformation costs must be covered by the approved release allowances.
#[cfg(all(windows, feature = "native-demucs-worker"))]
const SCAN64_SCORE_BYTES: usize = 2 * 8 * 64 * (8 * STEPS) * size_of::<f32>();
const TORCH: &str = "2.12.0+cu130";
const TORCH_COMMIT: &str = "7661cd9c6b841b62b7f411aa52ec51f05457263b";
const SOURCE_CLOSURE: &str = "7457feb86c729a70906beabc70afccb4e4e49bf63a639e87c280f85ed1be4113";

/// Prospective logical payload for this exact graph and the admitted CPU policy.
/// The owner establishes each phase's additional live payload, including ORT
/// intermediates, and retains its independent allocator/process reservation.
pub struct Payload {
    pub graph_bytes: usize,
    /// Other logical bytes live during load, including parser/copies and readers.
    pub load_extra: usize,
    /// Other logical bytes during execution, including operator tensors and readers.
    pub run_extra: usize,
}

impl Payload {
    pub fn load_bytes(&self) -> Result<usize, Error> {
        if !(1..=MAX_GRAPH_BYTES).contains(&self.graph_bytes) {
            return Err(Error::Geometry);
        }
        bounded(&[self.graph_bytes, WEIGHT_BYTES, self.load_extra])
    }

    fn run_bytes(&self) -> Result<usize, Error> {
        bounded(&[
            WEIGHT_BYTES,
            2 * DENSE_SCORE_BYTES,
            INPUT_BYTES, // potential provider copies during I/O binding
            OUTPUT_BYTES,
            self.run_extra,
        ])
    }

    /// Complete normalized-call payload after releasing graph input bytes.
    pub fn normalized_bytes(&self) -> Result<usize, Error> {
        bounded(&[self.run_bytes()?, INPUT_BYTES, OUTPUT_BYTES])
    }

    /// Complete offline invocation. Workspace already includes callback I/O.
    pub fn offline_bytes(
        &self,
        frames: usize,
        model: &Model<'_>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<usize, Error> {
        check_model(model)?;
        offline::Workspace::planned_payload_bytes(
            frames,
            std::slice::from_ref(model),
            self.run_bytes()?,
            cancelled,
        )
    }
}

/// Frozen liveness allowances reviewed against the exact learned graph and
/// ORT package. No allowance is derived from a successful allocation or peak.
#[cfg(all(windows, feature = "native-demucs-worker"))]
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scan64Release {
    pub graph_bytes: usize,
    pub graph_sha256: String,
    pub load_extra_bytes: usize,
    pub run_extra_bytes: usize,
    pub trace_sha256: String,
    pub attention_sha256: String,
}

#[cfg(all(windows, feature = "native-demucs-worker"))]
impl Scan64Release {
    pub fn load_bytes(&self) -> Result<usize, Error> {
        if !(1..=MAX_GRAPH_BYTES).contains(&self.graph_bytes) || self.load_extra_bytes == 0 {
            return Err(Error::Geometry);
        }
        // The mapped serialization consumes physical headroom. Conservatively
        // include it in the admission union with both possible weight owners.
        bounded(&[self.graph_bytes, 2 * WEIGHT_BYTES, self.load_extra_bytes])
    }

    pub fn run_bytes(&self) -> Result<usize, Error> {
        if self.run_extra_bytes == 0 {
            return Err(Error::Geometry);
        }
        bounded(&[
            WEIGHT_BYTES,
            SCAN64_SCORE_BYTES,
            INPUT_BYTES,
            OUTPUT_BYTES,
            self.run_extra_bytes,
        ])
    }

    pub fn offline_bytes(
        &self,
        frames: usize,
        model: &Model<'_>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<usize, Error> {
        check_model(model)?;
        offline::Workspace::planned_payload_bytes(
            frames,
            std::slice::from_ref(model),
            self.run_bytes()?,
            cancelled,
        )
    }
}

fn bounded(parts: &[usize]) -> Result<usize, Error> {
    parts
        .iter()
        .try_fold(0usize, |sum, &part| sum.checked_add(part))
        .filter(|&sum| sum <= MAX_PAYLOAD_BYTES)
        .ok_or(Error::Geometry)
}

fn check_model(model: &Model<'_>) -> Result<(), Error> {
    if model.sources == SOURCES
        && model.channels == 2
        && model.samplerate == RATE
        && model.training_frames == FRAMES
    {
        Ok(())
    } else {
        Err(Error::Geometry)
    }
}

/// One exclusive, single-owner ORT session. Construct it after the owner has
/// verified the runtime package/ABI and admitted both complete phase budgets.
pub struct CpuModel {
    session: Session,
    context: Context,
    run_extra: usize,
    run_limit: usize,
}

impl CpuModel {
    /// The expected graph SHA comes from the checked exporter via independent
    /// custody. This is the copying, inline-only model loader. Release the
    /// caller's graph bytes before invoking or charge them to the run payload.
    /// Obtain the run limit from the corresponding payload planner.
    /// The admission callback reserves the independently bounded process and
    /// configures the verified CPU-only ORT runtime before any ORT API here.
    /// The enclosing standalone owner holds its runtime guard through exit.
    pub fn load_owned(
        graph: &[u8],
        expected_sha256: &str,
        profile: &Path,
        payload: &Payload,
        run_limit: usize,
        context: &Context,
        admit: impl FnOnce(usize, usize) -> Result<(), String>,
    ) -> Result<Self, String> {
        let context = &context.with_cancel_message(CANCELLED);
        context.checkpoint("demucs.model.prepare")?;
        let load = payload.load_bytes().map_err(|e| e.to_string())?;
        let run_min = payload.normalized_bytes().map_err(
            |_| "HTDemucs dense attention exceeds the conservative 512 MiB payload bound",
        )?;
        if !(run_min..=MAX_PAYLOAD_BYTES).contains(&run_limit) {
            return Err("unsupported HTDemucs admitted run payload".into());
        }
        if payload.graph_bytes != graph.len()
            || trial_ort::sha256(graph, || context.check())? != expected_sha256
        {
            return Err("HTDemucs inline graph digest or extent mismatch".into());
        }
        admit(load, run_limit)?;
        context.checkpoint("demucs.model.load")?;
        let providers = [ep::CPU::default()
            .with_arena_allocator(false)
            .build()
            .error_on_failure()];
        let builder = trial_ort::session_builder(&providers)?
            .with_no_environment_execution_providers()
            .map_err(|e| e.to_string())?
            .with_memory_pattern(false)
            .map_err(|e| e.to_string())?
            .with_optimization_level(GraphOptimizationLevel::Disable)
            .map_err(|e| e.to_string())?
            .with_profiling(profile)
            .map_err(|e| e.to_string())?;
        let mut session = trial_ort::commit(builder, graph, Some(context))?;
        let validation = validate_session(&session, "torchscript-opset18-dense")
            .and_then(|()| context.cleanup())
            .and_then(|()| context.checkpoint("demucs.model.loaded"));
        if let Err(primary) = validation {
            context.record(
                session
                    .end_profiling()
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
            );
            return Err(primary);
        }
        Ok(Self {
            session,
            context: context.clone(),
            run_extra: payload.run_bytes().map_err(|e| e.to_string())?,
            run_limit,
        })
    }

    /// Map the same immutable bytes used for digest verification into the
    /// synchronous ONNX loader. The graph and load-only owners retire before Run.
    #[cfg(all(windows, feature = "native-demucs-worker"))]
    pub fn load_scan64(
        path: &Path,
        release: &Scan64Release,
        ceiling: usize,
        run_limit: usize,
        context: &Context,
        admit_runtime: impl FnOnce() -> Result<(), String>,
    ) -> Result<Self, String> {
        let context = context.with_cancel_message(CANCELLED);
        context.checkpoint("demucs.scan64.prepare")?;
        let load = release.load_bytes().map_err(|e| e.to_string())?;
        let run_extra = release.run_bytes().map_err(|e| e.to_string())?;
        let run_min =
            bounded(&[run_extra, INPUT_BYTES, OUTPUT_BYTES]).map_err(|e| e.to_string())?;
        if ceiling > MAX_PAYLOAD_BYTES
            || load > ceiling
            || !(run_min..=ceiling).contains(&run_limit)
        {
            return Err("HTDemucs Scan64 complete phase bounds exceed the admitted ceiling".into());
        }
        let session =
            super::inline_model::with_read_only(path, release.graph_bytes, &context, |graph| {
                if trial_ort::sha256(graph, || context.check())? != release.graph_sha256 {
                    return Err("HTDemucs Scan64 graph digest mismatch".into());
                }
                admit_runtime()?;
                context.checkpoint("demucs.scan64.load")?;
                let providers = [ep::CPU::default()
                    .with_arena_allocator(false)
                    .build()
                    .error_on_failure()];
                let builder = trial_ort::session_builder(&providers)?
                    .with_no_environment_execution_providers()
                    .map_err(|e| e.to_string())?
                    .with_memory_pattern(false)
                    .map_err(|e| e.to_string())?
                    .with_optimization_level(GraphOptimizationLevel::Disable)
                    .map_err(|e| e.to_string())?
                    .with_config_entry("session.disable_prepacking", "1")
                    .map_err(|e| e.to_string())?
                    .with_config_entry("session.load_model_format", "ONNX")
                    .map_err(|e| e.to_string())?;
                trial_ort::commit(builder, graph, Some(&context))
            })?;
        validate_session(&session, SCAN64_LOWERING)?;
        let metadata = session.metadata().map_err(|e| e.to_string())?;
        if metadata.custom("attention_query_block").as_deref() != Some("64")
            || metadata.custom("trace_sha256").as_deref() != Some(release.trace_sha256.as_str())
            || metadata.custom("attention_sha256").as_deref()
                != Some(release.attention_sha256.as_str())
        {
            return Err("HTDemucs Scan64 learned trace provenance mismatch".into());
        }
        context.cleanup()?;
        drop(metadata);
        context.checkpoint("demucs.scan64.loaded")?;
        Ok(Self {
            session,
            context,
            run_extra,
            run_limit,
        })
    }

    /// Invoke the learned branches on identical original normalized inputs.
    /// Any error invalidates both outputs. Buffers are borrowed through the
    /// synchronous call and ORT terminator join. No FFI reference escapes.
    pub fn run_normalized(
        &mut self,
        spectral: &[f32],
        time: &[f32],
        decoded_spectral: &mut [f32],
        decoded_time: &mut [f32],
    ) -> Result<(), String> {
        let context = &self.context;
        context.checkpoint("demucs.model.input")?;
        finite(spectral, SPEC_INPUT_LEN, context)?;
        finite(time, TIME_INPUT_LEN, context)?;
        if decoded_spectral.len() != SPEC_OUTPUT_LEN || decoded_time.len() != TIME_OUTPUT_LEN {
            return Err("HTDemucs caller output extent mismatch".into());
        }
        let spectral =
            TensorRef::from_array_view((SPEC_IN, spectral)).map_err(|e| e.to_string())?;
        let time = TensorRef::from_array_view((TIME_IN, time)).map_err(|e| e.to_string())?;
        // Drop binding and all native input references before the borrowed
        // input views and their caller storage leave scope.
        let mut binding = self.session.create_binding().map_err(|e| e.to_string())?;
        binding
            .bind_input("spectral", &spectral)
            .map_err(|e| e.to_string())?;
        binding
            .bind_input("waveform", &time)
            .map_err(|e| e.to_string())?;
        let spectral_output = sentinel(SPEC_OUT, SPEC_OUTPUT_LEN, context)?;
        let time_output = sentinel(TIME_OUT, TIME_OUTPUT_LEN, context)?;
        let spectral_ptr = spectral_output.data_ptr();
        let time_ptr = time_output.data_ptr();
        binding
            .bind_output("decoded_spectral", spectral_output)
            .map_err(|e| e.to_string())?;
        binding
            .bind_output("decoded_waveform", time_output)
            .map_err(|e| e.to_string())?;
        let options = RunOptions::new().map_err(|e| e.to_string())?;
        let terminated = Cell::new(false);
        context.checkpoint("demucs.model.run")?;
        let (mut result, requested) = terminate_during(
            context,
            None,
            || options.terminate().map_err(|e| e.to_string()),
            || {
                let outputs = self
                    .session
                    .run_binding_with_options(&binding, &options)
                    .map_err(|e| {
                        terminated.set(trial_ort::is_run_termination(e.code(), e.message()));
                        e.to_string()
                    })?;
                context.checkpoint("demucs.model.output")?;
                let spectral = outputs
                    .get("decoded_spectral")
                    .ok_or("missing decoded_spectral")?;
                let time = outputs
                    .get("decoded_waveform")
                    .ok_or("missing decoded_waveform")?;
                let (spectral_shape, spectral) = spectral
                    .try_extract_tensor::<f32>()
                    .map_err(|e| e.to_string())?;
                let (time_shape, time) = time
                    .try_extract_tensor::<f32>()
                    .map_err(|e| e.to_string())?;
                if spectral.as_ptr().cast() != spectral_ptr
                    || time.as_ptr().cast() != time_ptr
                    || !shape_matches(spectral_shape.as_ref(), &SPEC_OUT)
                    || !shape_matches(time_shape.as_ref(), &TIME_OUT)
                {
                    return Err("HTDemucs actual decoder shape mismatch".into());
                }
                finite(spectral, SPEC_OUTPUT_LEN, context)?;
                finite(time, TIME_OUTPUT_LEN, context)?;
                decoded_spectral.copy_from_slice(spectral);
                decoded_time.copy_from_slice(time);
                Ok(())
            },
        );
        if requested && terminated.get() && context.cancelled() {
            result = result.map_err(|primary| format!("{CANCELLED}; native run: {primary}"));
        }
        drop(binding);
        drop((spectral, time));
        retain_first(&mut result, context.cleanup());
        if result.is_ok() {
            context.checkpoint("demucs.model.completed")?;
        }
        result
    }

    /// Compose the same learned predictor with original-compatible offline
    /// normalization, context, inverse/add/crop and supplied offsets. The owner
    /// preflights this exact invocation before model load, with all live readers.
    pub fn run_offline(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        model: &Model<'_>,
        profile: CpuProfile,
        cli_normalization: bool,
    ) -> Result<(), String> {
        check_model(model).map_err(|e| e.to_string())?;
        if !input.len().is_multiple_of(2) {
            return Err(Error::Geometry.to_string());
        }
        let context = self.context.clone();
        let cancelled = || context.cancelled();
        let planned = offline::Workspace::planned_payload_bytes(
            input.len() / 2,
            std::slice::from_ref(model),
            self.run_extra,
            &cancelled,
        )
        .map_err(|e| e.to_string())?;
        if planned > self.run_limit {
            return Err("offline HTDemucs payload exceeds its admitted run extent".into());
        }
        let mut work = offline::Workspace::new(
            input.len() / 2,
            std::slice::from_ref(model),
            profile,
            self.run_extra,
            &cancelled,
        )
        .map_err(|e| e.to_string())?;
        let mut failure = None;
        let mut predict = |_: usize,
                           g: hybrid::Geometry,
                           spectral: &[f32],
                           time: &[f32],
                           decoded_spectral: &mut [f32],
                           decoded_time: &mut [f32],
                           cancel: &dyn Fn() -> bool| {
            verify_geometry(g)?;
            if cancel() {
                return Err(Error::Cancelled);
            }
            self.run_normalized(spectral, time, decoded_spectral, decoded_time)
                .map_err(|error| {
                    failure.get_or_insert(error);
                    Error::Predictor
                })
        };
        let result = if cli_normalization {
            work.run_cli(input, output, &mut predict, &cancelled)
        } else {
            work.run(input, output, &mut predict, &cancelled)
        };
        result.map_err(|error| {
            failure.unwrap_or_else(|| {
                if error == Error::Cancelled {
                    CANCELLED.into()
                } else {
                    error.to_string()
                }
            })
        })?;
        context.checkpoint("demucs.offline.completed")
    }

    /// Close ORT's execution evidence before releasing the session.
    pub fn finish_profile(&mut self) -> Result<String, String> {
        self.session.end_profiling().map_err(|e| e.to_string())
    }
}

fn verify_geometry(g: Geometry) -> Result<(), Error> {
    if g.batches() == 1
        && g.channels() == 2
        && g.sources() == SOURCES.len()
        && g.training_frames() == FRAMES
        && g.spectral_steps() == STEPS
    {
        Ok(())
    } else {
        Err(Error::Geometry)
    }
}

fn finite(values: &[f32], len: usize, context: &Context) -> Result<(), String> {
    if values.len() != len {
        return Err("HTDemucs tensor extent mismatch".into());
    }
    for block in values.chunks(4096) {
        context.check()?;
        if block.iter().any(|value| !value.is_finite()) {
            return Err("HTDemucs nonfinite or unwritten tensor".into());
        }
    }
    Ok(())
}

fn sentinel<const N: usize>(
    shape: [usize; N],
    len: usize,
    context: &Context,
) -> Result<Tensor<f32>, String> {
    context.check()?;
    let mut values = Vec::new();
    values.try_reserve_exact(len).map_err(|e| e.to_string())?;
    values.resize(len, f32::NAN);
    context.check()?;
    Tensor::from_array((shape, values)).map_err(|e| e.to_string())
}

fn shape_matches(actual: &[i64], expected: &[usize]) -> bool {
    actual.len() == expected.len() && actual.iter().zip(expected).all(|(&a, &b)| a == b as i64)
}

fn validate_session(session: &Session, lowering: &str) -> Result<(), String> {
    let metadata = session.metadata().map_err(|e| e.to_string())?;
    if metadata.custom("flitzis_htdemucs_contract").as_deref() != Some(CONTRACT)
        || metadata.custom("checkpoint_sha256").as_deref() != Some(CHECKPOINT)
        || metadata.custom("torch_version").as_deref() != Some(TORCH)
        || metadata.custom("torch_commit").as_deref() != Some(TORCH_COMMIT)
        || metadata.custom("source_closure_sha256").as_deref() != Some(SOURCE_CLOSURE)
        || metadata.custom("lowering").as_deref() != Some(lowering)
        || metadata.custom("sources").as_deref() != Some("drums,bass,other,vocals")
    {
        return Err("unexpected HTDemucs release or checkpoint identity".into());
    }
    for (actual, expected) in [
        (
            session.inputs(),
            [
                ("spectral", SPEC_IN.as_slice()),
                ("waveform", TIME_IN.as_slice()),
            ],
        ),
        (
            session.outputs(),
            [
                ("decoded_spectral", SPEC_OUT.as_slice()),
                ("decoded_waveform", TIME_OUT.as_slice()),
            ],
        ),
    ] {
        if actual.len() != expected.len() {
            return Err("unexpected HTDemucs tensor count".into());
        }
        for (actual, (name, dims)) in actual.iter().zip(expected) {
            if actual.name() != name
                || !matches!(actual.dtype(), ValueType::Tensor { ty: TensorElementType::Float32, shape, .. }
                    if shape_matches(shape.as_ref(), dims))
            {
                return Err(format!("unexpected HTDemucs float32 signature for {name}"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_unproved_dense_liveness_and_overflow() {
        assert_eq!(OUTPUT_BYTES, 55_047_552);
        assert_eq!(INPUT_BYTES, 13_761_888);
        assert_eq!(DENSE_SCORE_BYTES, 231_211_008);
        let payload = Payload {
            graph_bytes: WEIGHT_BYTES,
            load_extra: 0,
            run_extra: 0,
        };
        assert_eq!(payload.load_bytes().unwrap(), WEIGHT_BYTES * 2);
        assert_eq!(payload.normalized_bytes(), Err(Error::Geometry));
        let overflow = Payload {
            load_extra: usize::MAX,
            ..payload
        };
        assert_eq!(overflow.load_bytes(), Err(Error::Geometry));
    }

    #[test]
    fn exact_release_shape_and_finite_refusals() {
        let context = Context::new(None);
        assert!(shape_matches(&[1, 2, 343980], &TIME_IN));
        assert!(!shape_matches(&[-1, 2, 343980], &TIME_IN));
        assert!(!shape_matches(&[1, 2, 343980, 1], &TIME_IN));
        assert!(finite(&[0.0], 1, &context).is_ok());
        assert!(finite(&[0.0], 2, &context).is_err());
        assert!(finite(&[f32::NAN], 1, &context).is_err());
        assert!(verify_geometry(Geometry::new(1, 2, 4, FRAMES - 1, FRAMES, 0).unwrap()).is_ok());
        assert_eq!(
            verify_geometry(Geometry::new(2, 2, 4, FRAMES, FRAMES, 0).unwrap()),
            Err(Error::Geometry)
        );
    }

    #[test]
    fn dense_candidate_refuses_before_runtime_bootstrap() {
        let result = CpuModel::load_owned(
            &[0],
            "",
            Path::new("unused"),
            &Payload {
                graph_bytes: 1,
                load_extra: 0,
                run_extra: 0,
            },
            MAX_PAYLOAD_BYTES,
            &Context::new(None),
            |_, _| panic!("unadmitted candidate reached ORT bootstrap"),
        );
        assert_eq!(
            result.err().as_deref(),
            Some("HTDemucs dense attention exceeds the conservative 512 MiB payload bound"),
        );
    }
}
