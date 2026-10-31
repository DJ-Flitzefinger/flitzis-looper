//! Opt-in BeatThis experiment, never selected by the application.
//!
//! This core has no Python, PyO3 or LibTorch dependency.
//! The separate beat_audio core implements the complete waveform math.
//! The caller must admit a disposable offline process before loading/running it.

use ort::{
    ep,
    session::{RunOptions, Session, builder::AutoDevicePolicy},
    value::{TensorElementType, TensorRef, ValueType},
};
use sha2::{Digest, Sha256};
use std::path::Path;

pub const FRAMES: usize = 1500;
pub const BINS: usize = 128;
pub const MAX_GRAPH_BYTES: usize = 128 * 1024 * 1024;

/// One original model's two raw logit streams; no inferred beat/grid acceptance.
pub struct Logits {
    pub beat: Vec<f32>,
    pub downbeat: Vec<f32>,
}

/// Single-owner, session-local, bounded-thread candidate inference.
pub struct BeatModel {
    session: Session,
    fixed_frames: Option<usize>,
}

/// Requested trial policy. Effective execution is proved separately by profiling.
#[derive(Clone, Copy)]
pub enum Provider {
    Cpu,
    #[cfg(feature = "native-inference-cuda")]
    Cuda {
        memory_limit: usize,
    },
}

impl BeatModel {
    /// Verify the actual immutable graph bytes before ORT parses them.
    /// Only self-contained graphs emitted by the checked exporter are supported.
    /// The profiling path is caller-owned scratch, not product provenance.
    pub fn load(
        graph: &[u8],
        expected_sha256: &str,
        profile: &Path,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, String> {
        Self::load_on(graph, expected_sha256, profile, Provider::Cpu, cancelled)
    }

    /// A failed CUDA request is never silently reinterpreted as successful CPU.
    pub fn load_on(
        graph: &[u8],
        expected_sha256: &str,
        profile: &Path,
        provider: Provider,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, String> {
        checkpoint(cancelled)?;
        validate_graph(graph, expected_sha256)?;
        let providers = match provider {
            Provider::Cpu => vec![ep::CPU::default().build().error_on_failure()],
            #[cfg(feature = "native-inference-cuda")]
            Provider::Cuda { memory_limit } => vec![
                ep::CUDA::default()
                    .with_tf32(false)
                    .with_memory_limit(memory_limit)
                    .build()
                    .error_on_failure(),
                // ORT otherwise inserts its default CPU provider with a host
                // arena. Keep its shape/operator support, using the same host
                // allocator policy as our explicit CPU route. This is not a
                // retry on CPU when required CUDA registration fails.
                ep::CPU::default()
                    .with_arena_allocator(false)
                    .build()
                    .error_on_failure(),
            ],
        };
        let session = Session::builder()
            .map_err(|e| e.to_string())?
            .with_auto_device(AutoDevicePolicy::Default)
            .and_then(|s| s.with_execution_providers(providers))
            .and_then(|s| s.with_intra_threads(1))
            .and_then(|s| s.with_inter_threads(1))
            .and_then(|s| s.with_parallel_execution(false))
            .and_then(|s| s.with_intra_op_spinning(false))
            .and_then(|s| s.with_inter_op_spinning(false))
            .and_then(|s| s.with_profiling(profile))
            .map_err(|e| e.to_string())?
            .commit_from_memory(graph)
            .map_err(|e| e.to_string())?;
        checkpoint(cancelled)?;
        let inputs = session.inputs();
        if inputs.len() != 1 || inputs[0].name() != "spectrogram" {
            return Err("unexpected BeatThis input names".into());
        }
        let fixed_frames = input_signature(inputs[0].dtype())?;
        let outputs = session.outputs();
        if outputs.len() != 2 {
            return Err("expected exactly two BeatThis outputs".into());
        }
        for name in ["beat", "downbeat"] {
            let output = outputs
                .iter()
                .find(|o| o.name() == name)
                .ok_or_else(|| format!("missing BeatThis {name} output"))?;
            signature(output.dtype(), &[1, fixed_frames.map_or(-1, |n| n as i64)])?;
        }
        Ok(Self {
            session,
            fixed_frames,
        })
    }

    /// Execute one exact chunk. Cancellation surrounds synchronous ORT work;
    /// immediate interruption requires retiring the caller's owned process.
    pub fn run(
        &mut self,
        spectrogram: &[f32],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Logits, String> {
        self.run_with_options(
            spectrogram,
            &RunOptions::new().map_err(|e| e.to_string())?,
            cancelled,
        )
    }

    /// A shared RunOptions can request real mid-ORT termination. The caller must
    /// still join the run and retire the entire owned process and its readers.
    pub fn run_with_options(
        &mut self,
        spectrogram: &[f32],
        options: &RunOptions,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Logits, String> {
        checkpoint(cancelled)?;
        let frames = spectrogram.len() / BINS;
        if spectrogram.len() % BINS != 0
            || !(14..=FRAMES).contains(&frames)
            || self.fixed_frames.is_some_and(|n| n != frames)
            || !spectrogram.iter().all(|x| x.is_finite())
        {
            return Err("expected finite supported BeatThis chunk by 128 spectrogram".into());
        }
        let input = TensorRef::from_array_view(([1, frames, BINS], spectrogram))
            .map_err(|e| e.to_string())?;
        let outputs = self
            .session
            .run_with_options(ort::inputs!["spectrogram" => input], options)
            .map_err(|e| e.to_string())?;
        checkpoint(cancelled)?;
        let read = |name| -> Result<Vec<f32>, String> {
            let output = outputs.get(name).ok_or_else(|| format!("missing {name}"))?;
            let (shape, data) = output
                .try_extract_tensor::<f32>()
                .map_err(|e| e.to_string())?;
            if shape.as_ref() != [1, frames as i64] || !data.iter().all(|x| x.is_finite()) {
                return Err(format!("invalid {name} logits"));
            }
            Ok(data.to_vec())
        };
        Ok(Logits {
            beat: read("beat")?,
            downbeat: read("downbeat")?,
        })
    }

    /// Finalize actual executed-node evidence before the process retires.
    pub fn finish_profile(&mut self) -> Result<String, String> {
        self.session.end_profiling().map_err(|e| e.to_string())
    }
}

pub(crate) fn checkpoint(cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    if cancelled() {
        Err("native BeatThis trial cancelled".into())
    } else {
        Ok(())
    }
}

/// Admit immutable graph bytes before a standalone caller loads native code.
/// Session construction repeats this check for callers without that bootstrap.
pub fn validate_graph(graph: &[u8], expected: &str) -> Result<(), String> {
    if graph.is_empty() || graph.len() > MAX_GRAPH_BYTES {
        return Err("BeatThis graph exceeds trial bound".into());
    }
    if expected.len() != 64 || format!("{:x}", Sha256::digest(graph)) != expected {
        return Err("BeatThis graph digest mismatch".into());
    }
    Ok(())
}

fn input_signature(value: &ValueType) -> Result<Option<usize>, String> {
    if let ValueType::Tensor {
        ty: TensorElementType::Float32,
        shape,
        ..
    } = value
    {
        let dims = shape.as_ref();
        if dims.len() == 3 && dims[0] == 1 && dims[2] == BINS as i64 {
            return match dims[1] {
                -1 => Ok(None),
                n if n == FRAMES as i64 => Ok(Some(FRAMES)),
                _ => Err("unsupported BeatThis time extent".into()),
            };
        }
    }
    Err("unexpected BeatThis input signature".into())
}

fn signature(value: &ValueType, expected: &[i64]) -> Result<(), String> {
    match value {
        ValueType::Tensor {
            ty: TensorElementType::Float32,
            shape,
            ..
        } if shape.as_ref() == expected
            || (expected.len() == 2
                && shape.len() == 2
                && shape[0] == -1
                && expected[0] == 1
                && shape[1] == expected[1]) =>
        {
            Ok(())
        }
        // Dynamic export may leave the output batch inferred/unknown. Input
        // batch remains exactly one; every actual output still must be [1,T].
        _ => Err(format!("unexpected BeatThis tensor signature: {value:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_graph_is_rejected_before_runtime_load() {
        let digest = format!("{:x}", Sha256::digest(b"original"));
        assert!(
            validate_graph(b"changed", &digest)
                .unwrap_err()
                .contains("digest")
        );
        assert!(validate_graph(&[], &digest).unwrap_err().contains("bound"));
    }

    #[test]
    fn cancellation_precedes_graph_parsing() {
        let result = BeatModel::load(&[], "", Path::new("unused"), &|| true);
        assert!(result.err().unwrap().contains("cancelled"));
    }
}
