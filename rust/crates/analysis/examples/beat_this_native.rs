//! Developer-only one-chunk CLI; output is diagnostic logits, never an App receipt.
use flitzis_looper_analysis::beat_inference::{
    BINS, BeatModel, FRAMES, MAX_GRAPH_BYTES, Provider, validate_graph_checked,
};
use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};
#[path = "beat_this_audio/invocation.rs"]
mod invocation;
#[path = "beat_this_audio/runtime.rs"]
mod runtime;

fn execute(own: &mut invocation::Invocation) -> Result<serde_json::Value, String> {
    let context = own.context.clone();
    context.checkpoint("startup")?;
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 5 {
        return Err(
            "usage: beat_this_native GRAPH SHA256 SPECTROGRAM OUTPUT PROFILE_PREFIX".into(),
        );
    }
    if Path::new(&args[3])
        .try_exists()
        .map_err(|e| e.to_string())?
    {
        return Err("trial output already exists".into());
    }
    let (graph, _) = own.read(Path::new(&args[0]), MAX_GRAPH_BYTES)?;
    let (raw, _) = own.read(Path::new(&args[2]), FRAMES * BINS * 4)?;
    if raw.len() != FRAMES * BINS * 4 {
        return Err("incomplete trial spectrogram".into());
    }
    let spect = own.floats(&raw)?;
    drop(raw);
    let digest = args[1].to_str().ok_or("invalid digest encoding")?;
    validate_graph_checked(&graph, digest, &|| context.cancelled())?;
    let path = std::env::var_os(runtime::MANIFEST_ENV);
    let sha = std::env::var(runtime::DIGEST_ENV);
    let requested = if path.is_some() || std::env::var_os(runtime::DIGEST_ENV).is_some() {
        Some((
            PathBuf::from(path.ok_or("missing runtime manifest path")?),
            sha.map_err(|e| e.to_string())?,
        ))
    } else {
        None
    };
    let (admitted, environment) = runtime::configure(
        requested.as_ref().map(|(p, d)| (p.as_path(), d.as_str())),
        &context,
    )?;
    runtime::commit(environment)?;
    let profile = own.profile(Path::new(&args[4]))?;
    context.checkpoint("model.prepare")?;
    let mut model = BeatModel::load_on_owned(&graph, digest, &profile, Provider::Cpu, &context)?;
    drop(graph);
    let mut result = invocation::run(&mut model, &spect, &context, None);
    let profile = own.finish_profile(&mut model, &mut result);
    drop(model);
    let logits = result?;
    let receipt = admitted
        .as_ref()
        .map(runtime::Admission::receipt)
        .transpose()?;
    let values: Vec<_> = logits.beat.into_iter().chain(logits.downbeat).collect();
    own.write_floats(Path::new(&args[3]), &values)?;
    Ok(
        serde_json::json!({"trial_only":true,"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "frames":FRAMES,"profile":profile,"runtime_admission":receipt}),
    )
}

fn main() -> ExitCode {
    let mut own = invocation::Invocation::new();
    let result = execute(&mut own);
    own.finish(
        result,
        None,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    )
}
