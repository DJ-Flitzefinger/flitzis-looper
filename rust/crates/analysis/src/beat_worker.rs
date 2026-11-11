//! One complete BeatThis CPU request in the App's supervised offline child.
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
    Err("native BeatThis worker requires the Windows x64 CPU package".into())
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    let mut own = invocation::Invocation::new();
    let result = windows::run(&mut own);
    own.finish(
        result,
        None,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    )
}

#[cfg(windows)]
mod windows {
    use super::{invocation, runtime};
    use flitzis_looper_analysis::{
        beat_audio::{self, Soxr},
        beat_inference::BeatModel,
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

    pub fn run(own: &mut invocation::Invocation) -> Result<serde_json::Value, String> {
        let context = own.context.clone();
        context.checkpoint("worker.startup")?;
        if !context.bound() || env::consts::ARCH != "x86_64" || ort::MINOR_VERSION != 24 {
            return Err("native BeatThis worker requires its own cancellation and API 24".into());
        }
        let args: Vec<_> = env::args_os().skip(1).collect();
        let [pcm, rate, frames, mono_sha, out] = args.as_slice() else {
            return Err("expected PCM RATE FRAMES MONO_SHA256 OUTPUT_DIRECTORY".into());
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
        let ort_dir = root.join("ort");
        let manifest = ort_dir.join("step23-runtime.json");
        let package = ort_dir.join("step23-package.json");
        if env::var_os(runtime::PACKAGE_ENV).as_deref() != Some(package.as_os_str())
            || env::var(runtime::PACKAGE_DIGEST_ENV).as_deref() != Ok(ORT_PACKAGE_SHA)
            || env::var(runtime::LOAD_POLICY_ENV).as_deref() != Ok("package-system32")
        {
            return Err("native BeatThis worker requires its pinned ORT CPU package".into());
        }

        let (raw, sha) = own.read_checked(pcm, beat_audio::MAX_AUDIO_BYTES, |len| {
            if len != (frames as u64) * 4 {
                return Err("native BeatThis PCM extent differs".into());
            }
            Ok(())
        })?;
        if mono_sha.to_str() != Some(sha.as_str()) {
            return Err("native BeatThis staged PCM digest differs".into());
        }
        let signal = own.floats(&raw)?;
        drop(raw);
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
            runtime::configure(Some((&manifest, ORT_MANIFEST_SHA)), &context)?;
        let admission = admission.ok_or("missing pinned ORT admission")?;
        runtime::commit(environment)?;
        context.checkpoint("model.prepare")?;
        let mut model = BeatModel::load_worker_cpu(&graph, GRAPH_SHA, &context)?;
        drop(graph);
        let logits = beat_audio::predict(
            &spect,
            |chunk| invocation::run(&mut model, chunk, &context, None),
            &|| context.cancelled(),
        )?;
        drop(model);
        drop(spect);
        let duration = frames as f64 / f64::from(rate);
        let (beats, downbeats) =
            beat_audio::postprocess_checked(&logits, duration, &|| context.cancelled())?;
        own.write_floats(&out.join("beat.f32"), &logits.beat)?;
        own.write_floats(&out.join("downbeat.f32"), &logits.downbeat)?;
        let runtime = admission.receipt()?;
        Ok(serde_json::json!({
            "schema_version": 1,
            "mono_sha256": sha,
            "frame_count": frames,
            "sample_rate_hz": rate,
            "graph_sha256": graph_sha,
            "ort_version": runtime["runtime_version"],
            "ort_api": runtime["ort_api"],
            "provider": "CPUExecutionProvider",
            "logit_count": logits.beat.len(),
            "beat_seconds": beats,
            "downbeat_seconds": downbeats,
        }))
    }
}
