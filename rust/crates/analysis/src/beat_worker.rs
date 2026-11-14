//! One complete Native BeatThis request in the App's supervised offline child.
#[cfg(windows)]
#[path = "beat_worker_cuda.rs"]
mod cuda;
#[cfg(windows)]
#[allow(dead_code)]
#[path = "../examples/beat_this_audio/invocation.rs"]
mod invocation;
#[cfg(windows)]
#[allow(dead_code)]
#[path = "../examples/beat_this_audio/runtime.rs"]
mod runtime;

#[cfg(not(windows))]
fn main() -> Result<(), String> {
    Err("native BeatThis worker requires a Windows x64 package".into())
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    let mut own = invocation::Invocation::new();
    let mut fault = None;
    let result = windows::run(&mut own, &mut fault);
    own.finish(
        result,
        fault,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    )
}

#[cfg(windows)]
mod windows {
    use super::{cuda, invocation, runtime};
    use flitzis_looper_analysis::{
        beat_audio::{self, Soxr},
        beat_inference::{BeatModel, Provider},
        trial_lifetime::retain_first,
    };
    use std::{env, path::Path};

    const GRAPH_BYTES: usize = 82_104_250;
    const GRAPH_SHA: &str = "46bfb2c1a1f1e5c1d8786d0d9462f285971ff7fa8a592ac5f4ab609698bd44ca";
    const SOXR_SHA: &str = "b2e3a0ff4806540250fa45e9522c3a3549663888397fce4708249369b25db37e";
    const ORT_MANIFEST_SHA: &str =
        "4929eb4f6a3aebe2c117850a2e1354c48bc004bfa5ff4d805c2ce0ecbef2915d";
    const ORT_PACKAGE_SHA: &str =
        "7a5e216d9cc35418e8c38afa293ece04e86abc63153553fe56e81aa901cf2319";

    pub fn run(
        own: &mut invocation::Invocation,
        fault: &mut Option<serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
        let context = own.context.clone();
        context.checkpoint("worker.startup")?;
        if !context.bound() || env::consts::ARCH != "x86_64" || ort::MINOR_VERSION != 24 {
            return Err("native BeatThis worker requires its own cancellation and API 24".into());
        }
        let args: Vec<_> = env::args_os().skip(1).collect();
        let (args, on_cuda) = match args.as_slice() {
            [mode, rest @ ..] if mode == "--cuda" => (rest, true),
            _ => (args.as_slice(), false),
        };
        let [pcm, rate, frames, mono_sha, out] = args else {
            return Err("expected [--cuda] PCM RATE FRAMES MONO_SHA256 OUTPUT_DIRECTORY".into());
        };
        let pcm = Path::new(pcm);
        let out = Path::new(out);
        if !pcm.is_absolute() || !out.is_absolute() {
            return Err("native BeatThis worker paths must be absolute".into());
        }
        let rate: u32 = rate
            .to_str()
            .and_then(|s| s.parse().ok())
            .ok_or("invalid sample rate")?;
        let frames: usize = frames
            .to_str()
            .and_then(|s| s.parse().ok())
            .ok_or("invalid frame count")?;
        beat_audio::validate_audio_shape(frames, rate)?;
        let executable = env::current_exe().map_err(|error| error.to_string())?;
        let root = executable.parent().ok_or("missing worker installation")?;
        let (directory, manifest_sha, package_sha) = if on_cuda {
            ("cuda", cuda::ORT_MANIFEST_SHA, cuda::ORT_PACKAGE_SHA)
        } else {
            ("ort", ORT_MANIFEST_SHA, ORT_PACKAGE_SHA)
        };
        let ort_dir = root.join(directory);
        let manifest = ort_dir.join("step23-runtime.json");
        let package = ort_dir.join("step23-package.json");
        if env::var_os(runtime::PACKAGE_ENV).as_deref() != Some(package.as_os_str())
            || env::var(runtime::PACKAGE_DIGEST_ENV).as_deref() != Ok(package_sha)
            || env::var(runtime::LOAD_POLICY_ENV).as_deref() != Ok("package-system32")
        {
            return Err("native BeatThis worker requires its pinned ORT package".into());
        }

        let pcm_bytes = beat_audio::pcm_byte_extent(frames)?;
        let (raw, sha) = own.read_checked(pcm, pcm_bytes, |len| {
            if len != pcm_bytes as u64 {
                return Err("native BeatThis PCM extent differs".into());
            }
            Ok(())
        })?;
        if mono_sha.to_str() != Some(sha.as_str()) {
            return Err("native BeatThis staged PCM digest differs".into());
        }
        let signal = own.floats(&raw)?;
        drop(raw);
        let _crt = runtime::load_worker_crt(root, &context)?;
        let soxr = Soxr::load_checked(&root.join("soxr.dll"), SOXR_SHA, &context)?;
        context.checkpoint("frontend.resample")?;
        let mut prepared = soxr.resample(&signal, rate, &|| context.cancelled());
        let close = soxr.complete();
        context.record(close.clone());
        retain_first(&mut prepared, close);
        drop(signal);
        let prepared = prepared?;
        context.checkpoint("frontend.log_mel")?;
        let spect = beat_audio::log_mel(&prepared, &|| context.cancelled())?;
        drop(prepared);

        let (graph, graph_sha) = own.read_checked(
            &root.join("beat-this-dynamic.onnx"),
            GRAPH_BYTES,
            |extent| {
                if extent != GRAPH_BYTES as u64 {
                    return Err("native BeatThis graph extent differs".into());
                }
                Ok(())
            },
        )?;
        if graph_sha != GRAPH_SHA {
            return Err("native BeatThis graph digest differs".into());
        }
        let (admission, environment) =
            runtime::configure(Some((&manifest, manifest_sha)), &context)?;
        let admission = admission.ok_or("missing pinned ORT admission")?;
        runtime::commit(environment)?;
        context.checkpoint("model.prepare")?;
        let mut evidence = serde_json::json!({
            "schema_version": 1,
            "mono_sha256": sha,
            "frame_count": frames,
            "sample_rate_hz": rate,
            "graph_sha256": graph_sha,
        });
        let mut startup_failure = None;
        let device = on_cuda
            .then(|| cuda::device(&context, &mut startup_failure))
            .transpose();
        let device = settle_startup(device, startup_failure, &mut evidence, fault, &context)?;
        let mut profile = on_cuda
            .then(|| own.profile(&out.join("ort-profile")))
            .transpose()?;
        let provider = if on_cuda {
            cuda::provider()
        } else {
            Provider::Cpu
        };
        let mut startup_failure = None;
        let model = BeatModel::load_worker(
            &graph,
            GRAPH_SHA,
            profile.as_deref(),
            provider,
            &context,
            |stage, code, message| {
                if on_cuda {
                    startup_failure = cuda::model_startup_error(stage, code, message);
                }
            },
        );
        let mut model = settle_startup(model, startup_failure, &mut evidence, fault, &context)?;
        drop(graph);
        let mut device_error = None;
        let mut cuda_kernel_events = 0;
        let logits = beat_audio::predict(
            &spect,
            |chunk| {
                let mut result =
                    invocation::run_observed(&mut model, chunk, &context, None, |code, message| {
                        if on_cuda {
                            device_error = cuda::device_error(code, message);
                        }
                    });
                if let Some(prefix) = profile.take() {
                    let path = own.finish_profile(&mut model, &mut result);
                    if result.is_ok() {
                        cuda_kernel_events = cuda::kernel_events(own, &prefix, path.as_deref())?;
                    }
                }
                result
            },
            &|| context.cancelled(),
        );
        drop(spect);
        let logits = match logits {
            Ok(logits) => logits,
            Err(error) => {
                if let (Some(device_error), Some(device)) = (device_error, &device) {
                    context.checkpoint("cuda.failure")?;
                    context.cleanup()?;
                    let receipt = admission.receipt()?;
                    evidence["cuda"] = cuda::runtime_evidence(device, &receipt)?;
                    evidence["cuda_error"] = device_error.into();
                    *fault = Some(evidence);
                }
                return Err(error);
            }
        };
        let runtime = admission.receipt()?;
        drop(model);
        evidence["ort_version"] = runtime["runtime_version"].clone();
        evidence["ort_api"] = runtime["ort_api"].clone();
        evidence["provider"] = if let Some(device) = &device {
            evidence["cuda"] = cuda::runtime_evidence(device, &runtime)?;
            evidence["cuda_kernel_events"] = cuda_kernel_events.into();
            "CUDAExecutionProvider".into()
        } else {
            "CPUExecutionProvider".into()
        };
        let duration = frames as f64 / f64::from(rate);
        let (beats, downbeats) =
            beat_audio::postprocess_checked(&logits, duration, &|| context.cancelled())?;
        own.write_floats(&out.join("beat.f32"), &logits.beat)?;
        own.write_floats(&out.join("downbeat.f32"), &logits.downbeat)?;
        evidence["logit_count"] = logits.beat.len().into();
        evidence["beat_seconds"] = serde_json::json!(beats);
        evidence["downbeat_seconds"] = serde_json::json!(downbeats);
        Ok(evidence)
    }

    fn settle_startup<T>(
        result: Result<T, String>,
        failure: Option<cuda::StartupError>,
        evidence: &mut serde_json::Value,
        fault: &mut Option<serde_json::Value>,
        context: &flitzis_looper_analysis::trial_lifetime::Context,
    ) -> Result<T, String> {
        if result.is_err()
            && let Some(failure) = failure
        {
            context.checkpoint("cuda.startup_failure")?;
            context.cleanup()?;
            let mut receipt = std::mem::take(evidence);
            receipt["cuda_startup_error"] = serde_json::json!(failure);
            *fault = Some(receipt);
        }
        result
    }
}
