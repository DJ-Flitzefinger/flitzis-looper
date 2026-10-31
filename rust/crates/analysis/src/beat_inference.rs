//! Opt-in fixed-chunk BeatThis experiment, never selected by the application.
//!
//! This core has no Python, PyO3 or LibTorch dependency. It deliberately does not
//! implement the audio frontend, short chunks, aggregation or postprocessing.
//! The caller must admit a disposable offline process before loading/running it.

use ort::{
    ep,
    session::{Session, builder::AutoDevicePolicy},
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
        checkpoint(cancelled)?;
        validate_graph(graph, expected_sha256)?;
        let session = Session::builder()
            .map_err(|e| e.to_string())?
            .with_auto_device(AutoDevicePolicy::Default)
            .and_then(|s| {
                s.with_execution_providers([ep::CPU::default().build().error_on_failure()])
            })
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
        signature(inputs[0].dtype(), &[1, FRAMES as i64, BINS as i64])?;
        let outputs = session.outputs();
        if outputs.len() != 2 {
            return Err("expected exactly two BeatThis outputs".into());
        }
        for name in ["beat", "downbeat"] {
            let output = outputs
                .iter()
                .find(|o| o.name() == name)
                .ok_or_else(|| format!("missing BeatThis {name} output"))?;
            signature(output.dtype(), &[1, FRAMES as i64])?;
        }
        Ok(Self { session })
    }

    /// Execute one exact chunk. Cancellation surrounds synchronous ORT work;
    /// immediate interruption requires retiring the caller's owned process.
    pub fn run(
        &mut self,
        spectrogram: &[f32],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Logits, String> {
        checkpoint(cancelled)?;
        if spectrogram.len() != FRAMES * BINS || !spectrogram.iter().all(|x| x.is_finite()) {
            return Err("expected finite 1500 by 128 spectrogram".into());
        }
        let input = TensorRef::from_array_view(([1, FRAMES, BINS], spectrogram))
            .map_err(|e| e.to_string())?;
        let outputs = self
            .session
            .run(ort::inputs!["spectrogram" => input])
            .map_err(|e| e.to_string())?;
        checkpoint(cancelled)?;
        let read = |name| -> Result<Vec<f32>, String> {
            let output = outputs.get(name).ok_or_else(|| format!("missing {name}"))?;
            let (shape, data) = output
                .try_extract_tensor::<f32>()
                .map_err(|e| e.to_string())?;
            if shape.as_ref() != [1, FRAMES as i64] || !data.iter().all(|x| x.is_finite()) {
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

fn checkpoint(cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    if cancelled() {
        Err("native BeatThis trial cancelled".into())
    } else {
        Ok(())
    }
}

fn validate_graph(graph: &[u8], expected: &str) -> Result<(), String> {
    if graph.is_empty() || graph.len() > MAX_GRAPH_BYTES {
        return Err("BeatThis graph exceeds trial bound".into());
    }
    if expected.len() != 64 || format!("{:x}", Sha256::digest(graph)) != expected {
        return Err("BeatThis graph digest mismatch".into());
    }
    Ok(())
}

fn signature(value: &ValueType, expected: &[i64]) -> Result<(), String> {
    match value {
        ValueType::Tensor {
            ty: TensorElementType::Float32,
            shape,
            ..
        } if shape.as_ref() == expected => Ok(()),
        _ => Err("unexpected fixed-chunk BeatThis tensor signature".into()),
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
