//! Offline complete-source diagnostic CLI, never an application execution receipt.
use flitzis_looper_analysis::{
    beat_audio::{self, Soxr, validate_audio_shape},
    beat_inference::{BeatModel, MAX_GRAPH_BYTES, Provider, validate_graph_checked},
    trial_lifetime::{CANCELLED, retain_first},
};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{Arc, Mutex},
};
#[path = "beat_this_audio/fault.rs"]
mod fault;
#[path = "beat_this_audio/invocation.rs"]
mod invocation;
#[path = "beat_this_audio/runtime.rs"]
mod runtime;

fn side(path: &str, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{path}.{suffix}"))
}

fn execute(
    a: &[String],
    evidence: &Arc<Mutex<fault::Evidence>>,
    arena: bool,
    own: &mut invocation::Invocation,
    status: &mut impl Write,
) -> Result<serde_json::Value, String> {
    let context = own.context.clone();
    context.checkpoint("startup")?;
    let infer = a.first().is_some_and(|s| s == "infer");
    let control = a.first().is_some_and(|s| s == "--cancellation-control");
    if (!infer && (a.len() != 6 || !(a[0] == "frontend" || control))) || (infer && a.len() != 11) {
        return Err("usage: beat_this_audio frontend SOXR SHA PCM RATE NEW_JSON | infer GRAPH SHA SOXR SHA PCM RATE NEW_JSON PROFILE cpu|cuda GPU_MEMORY_LIMIT".into());
    }
    if control && !context.bound() {
        return Err("control requires its own cancellation marker".into());
    }
    let offset = if infer { 3 } else { 1 };
    let dest = &a[offset + 4];
    for path in [
        PathBuf::from(dest),
        side(dest, "resampled.f32"),
        side(dest, "spect.f32"),
        side(dest, "logits.f32"),
    ] {
        context.check()?;
        if path.try_exists().map_err(|e| e.to_string())? {
            return Err("trial output already exists".into());
        }
    }
    let rate: u32 = a[offset + 3].parse().map_err(|_| "invalid sample rate")?;
    let (raw, source_digest) =
        own.read_checked(Path::new(&a[offset + 2]), isize::MAX as usize, |bytes| {
            if bytes == 0 || bytes % 4 != 0 {
                return Err("invalid complete float32 PCM byte extent".into());
            }
            validate_audio_shape(bytes as usize / 4, rate).map(|_| ())
        })?;
    let signal = own.floats(&raw)?;
    drop(raw);
    let source_frames = signal.len();
    let source_duration = source_frames as f64 / rate as f64;
    let soxr = Soxr::load_checked(Path::new(&a[offset]), &a[offset + 1], &context)?;
    context.checkpoint("frontend.resample")?;
    let mut prepared = soxr.resample(&signal, rate, &|| context.cancelled());
    let close = soxr.complete();
    context.record(close.clone());
    retain_first(&mut prepared, close);
    drop(signal);
    let prepared = prepared?;
    context.checkpoint("frontend.log_mel")?;
    if control {
        // A real exclusive partial file precedes real nonneural frontend work.
        own.write_floats(
            &side(dest, "spect.f32"),
            &prepared[..prepared.len().min(2048)],
        )?;
        writeln!(
            status,
            "BeatThis cancellation control ready: nonneural private-output=1"
        )
        .map_err(|e| e.to_string())?;
        status.flush().map_err(|e| e.to_string())?;
        context.checkpoint("control.frontend")?;
        loop {
            let _ = beat_audio::log_mel(&prepared, &|| context.cancelled())?;
        }
    }
    let spect = beat_audio::log_mel(&prepared, &|| context.cancelled())?;
    let mut metadata = serde_json::json!({"trial_only":true,"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "source_sha256":source_digest,"source_frames":source_frames,"rate":rate,
        "resampled_frames":prepared.len(),"spectrogram_frames":spect.len()/128,
        "chunk_starts":beat_audio::chunk_starts(spect.len()/128)?,"soxr_sha256":a[offset+1]});
    let _admitted = if infer {
        let provider = match a[9].as_str() {
            "cpu" => Provider::Cpu,
            #[cfg(feature = "native-inference-cuda")]
            "cuda" => Provider::Cuda {
                memory_limit: a[10].parse().map_err(|_| "invalid GPU limit")?,
            },
            _ => return Err("unsupported explicit provider".into()),
        };
        let (graph, _) = own.read(Path::new(&a[1]), MAX_GRAPH_BYTES)?;
        validate_graph_checked(&graph, &a[2], &|| context.cancelled())?;
        let admitted = configure(a, evidence, arena, &context)?;
        let cancel_ms = std::env::var("FLITZIS_TRIAL_CANCEL_MS")
            .ok()
            .map(|s| s.parse::<u64>())
            .transpose()
            .map_err(|_| "invalid diagnostic cancellation interval")?;
        if cancel_ms.is_some_and(|n| !(1..=1000).contains(&n)) {
            return Err("invalid diagnostic cancellation interval".into());
        }
        let profile = own.profile(Path::new(&a[8]))?;
        context.checkpoint("model.prepare")?;
        let mut model = BeatModel::load_on_owned(&graph, &a[2], &profile, provider, &context)?;
        drop(graph);
        let mut marked = false;
        let mut result = beat_audio::predict(
            &spect,
            |chunk| {
                writeln!(
                    std::io::stderr().lock(),
                    "native_ort_run_start frames={}",
                    chunk.len() / 128
                )
                .map_err(|e| e.to_string())?;
                if !marked && let Some(path) = std::env::var_os("FLITZIS_TRIAL_RUN_MARKER") {
                    own.write(Path::new(&path), |file, _| {
                        file.write_all(b"ORT invocation follows\n")
                            .map_err(|e| e.to_string())
                    })?;
                    marked = true;
                }
                invocation::run(&mut model, chunk, &context, cancel_ms)
            },
            &|| context.cancelled(),
        );
        let profile = own.finish_profile(&mut model, &mut result);
        drop(model);
        let logits = result?;
        context.checkpoint("postprocess")?;
        let (beats, downbeats) =
            beat_audio::postprocess_checked(&logits, source_duration, &|| context.cancelled())?;
        metadata["beat_seconds"] = serde_json::json!(beats);
        metadata["downbeat_seconds"] = serde_json::json!(downbeats);
        metadata["profile"] = serde_json::json!(profile);
        metadata["requested_provider"] = serde_json::json!(a[9]);
        metadata["graph_sha256"] = serde_json::json!(a[2]);
        if let Some(runtime) = &admitted {
            metadata["runtime_admission"] = runtime.receipt()?;
        }
        let values: Vec<_> = logits.beat.into_iter().chain(logits.downbeat).collect();
        own.write_floats(&side(dest, "logits.f32"), &values)?;
        admitted
    } else {
        None
    };
    own.write_floats(&side(dest, "resampled.f32"), &prepared)?;
    own.write_floats(&side(dest, "spect.f32"), &spect)?;
    own.write(Path::new(dest), |file, context| {
        context.check()?;
        serde_json::to_writer_pretty(file, &metadata).map_err(|e| e.to_string())
    })?;
    Ok(metadata)
}

fn main() -> ExitCode {
    let a: Vec<_> = std::env::args().skip(1).collect();
    let mut own = invocation::Invocation::new();
    let evidence = Arc::new(Mutex::new(fault::Evidence::default()));
    let diagnostic = std::env::var_os("FLITZIS_TRIAL_CUDA_ARENA_DIAGNOSTIC");
    let arena = diagnostic.as_deref() == Some(std::ffi::OsStr::new("1"))
        && a.len() == 11
        && a[0] == "infer"
        && a[9] == "cuda"
        && a[10] == "1"
        && std::env::var_os("FLITZIS_TRIAL_CANCEL_MS").is_none();
    let mut status = std::io::stdout().lock();
    let result = (|| {
        own.context.checkpoint("startup")?;
        if diagnostic.is_some() && !arena {
            return Err("invalid explicit CUDA arena diagnostic request".into());
        }
        if !a.first().is_some_and(|s| s == "infer" || s == "runtime")
            && (std::env::var_os(runtime::MANIFEST_ENV).is_some()
                || std::env::var_os(runtime::DIGEST_ENV).is_some()
                || runtime::package_requested())
        {
            return Err(
                "runtime configuration requires an inference or runtime probe command".into(),
            );
        }
        if a.first().is_some_and(|s| s == "runtime") {
            let admitted = configure(&a, &evidence, arena, &own.context)?;
            let build = ort::info();
            let receipt = admitted
                .as_ref()
                .map(runtime::Admission::receipt)
                .transpose()?;
            Ok(
                serde_json::json!({"trial_only":true,"trial_runtime_probe":true,"runtime_admission":receipt,"ort_build":build}),
            )
        } else {
            execute(&a, &evidence, arena, &mut own, &mut status)
        }
    })();
    let thread = format!("{:?}", std::thread::current().id());
    let fault = result.as_ref().err().filter(|e| arena && !e.starts_with(CANCELLED))
        .and_then(|error| evidence.lock().ok()?.fault(&thread,error).map(|rounded| serde_json::json!({"trial_only":true,
            "trial_cuda_arena_fault":true,"graph_sha256":a[2],"gpu_memory_limit":1,"thread":thread,"rounded_bytes":rounded,"error":error})));
    own.finish(result, fault, &mut status, &mut std::io::stderr().lock())
}

fn configure(
    a: &[String],
    evidence: &Arc<Mutex<fault::Evidence>>,
    arena: bool,
    context: &flitzis_looper_analysis::trial_lifetime::Context,
) -> Result<Option<runtime::Admission>, String> {
    let probe = a.first().is_some_and(|s| s == "runtime");
    let manifest = std::env::var_os(runtime::MANIFEST_ENV);
    let digest = std::env::var(runtime::DIGEST_ENV).map_err(|e| e.to_string());
    let requested = if probe {
        if a.len() != 3 || manifest.is_some() || std::env::var_os(runtime::DIGEST_ENV).is_some() {
            return Err("usage: beat_this_audio runtime MANIFEST SHA256 (without runtime environment overrides)".into());
        }
        Some((PathBuf::from(&a[1]), a[2].clone()))
    } else if manifest.is_some() || std::env::var_os(runtime::DIGEST_ENV).is_some() {
        Some((
            PathBuf::from(manifest.ok_or("missing runtime manifest path")?),
            digest?,
        ))
    } else {
        None
    };
    if !(probe || a.len() == 11 && a[0] == "infer") {
        if requested.is_some() {
            return Err(
                "runtime configuration requires a valid inference or runtime probe command".into(),
            );
        }
        return Ok(None);
    }
    // This is the first ORT access in this standalone process. Retain the checked
    // files until every session/run/profile operation in execute has returned.
    let (admitted, mut environment) = runtime::configure(
        requested.as_ref().map(|(p, d)| (p.as_path(), d.as_str())),
        context,
    )?;
    if arena || std::env::var_os("FLITZIS_TRIAL_ORT_LOG").is_some() {
        // Process-local standalone diagnostics; never used by the App/core.
        let observed = Arc::clone(evidence);
        let completion = context.clone();
        environment = environment.with_logger(Arc::new(move |level,category,id,location,message| {
            if message.contains("BFCArena") || level >= ort::logging::LogLevel::Error {
                let thread = format!("{:?}",std::thread::current().id());
                if let Ok(mut log) = observed.lock() {
                    log.observe(&thread,location,message);
                }
                completion.record(writeln!(std::io::stderr().lock(), "ORT_TRIAL_LOG {}",serde_json::json!({"thread":thread,"level":format!("{level:?}"),"category":category,"id":id,"location":location,"message":message})).map_err(|e| e.to_string()));
            }
        }));
    }
    runtime::commit(environment)?;
    Ok(admitted)
}
