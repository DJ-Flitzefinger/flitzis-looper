//! Offline complete-source diagnostic CLI, never an application execution receipt.
use flitzis_looper_analysis::{
    beat_audio::{self, MAX_AUDIO_BYTES, Soxr, validate_audio_shape},
    beat_inference::{BeatModel, MAX_GRAPH_BYTES, Provider, validate_graph},
};
use ort::session::RunOptions;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{Arc, Mutex},
};
#[path = "beat_this_audio/fault.rs"]
mod fault;
#[path = "beat_this_audio/runtime.rs"]
mod runtime;

fn bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let mut raw = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|e| e.to_string())?;
    if raw.is_empty() || raw.len() > limit {
        return Err("input exceeds complete trial byte bound".into());
    }
    Ok(raw)
}
fn pcm(path: &Path, rate: u32) -> Result<(Vec<f32>, String), String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let bytes = file.metadata().map_err(|e| e.to_string())?.len();
    if bytes == 0 || bytes > MAX_AUDIO_BYTES as u64 || bytes % 4 != 0 {
        return Err("invalid complete float32 PCM byte extent".into());
    }
    validate_audio_shape(bytes as usize / 4, rate)?;
    let mut raw = Vec::new();
    raw.try_reserve_exact(bytes as usize + 1)
        .map_err(|e| e.to_string())?;
    file.take(bytes + 1)
        .read_to_end(&mut raw)
        .map_err(|e| e.to_string())?;
    if raw.len() as u64 != bytes {
        return Err("complete PCM changed during read".into());
    }
    let digest = format!("{:x}", Sha256::digest(&raw));
    let signal: Vec<_> = raw
        .chunks_exact(4)
        .map(|x| f32::from_le_bytes(x.try_into().unwrap()))
        .collect();
    if !signal.iter().all(|x| x.is_finite()) {
        return Err("nonfinite complete mono PCM".into());
    }
    Ok((signal, digest))
}
fn float_file(path: &Path, values: &[f32]) -> Result<(), String> {
    let mut file = File::create_new(path).map_err(|e| e.to_string())?;
    for v in values {
        file.write_all(&v.to_le_bytes())
            .map_err(|e| e.to_string())?;
    }
    file.sync_all().map_err(|e| e.to_string())
}
fn side(path: &str, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{path}.{suffix}"))
}

fn execute(
    a: &[String],
    evidence: &Arc<Mutex<fault::Evidence>>,
    arena: bool,
) -> Result<(), String> {
    let infer = a.first().is_some_and(|s| s == "infer");
    if (!infer && (a.len() != 6 || a[0] != "frontend")) || (infer && a.len() != 11) {
        return Err("usage: beat_this_audio frontend SOXR SHA PCM RATE NEW_JSON | infer GRAPH SHA SOXR SHA PCM RATE NEW_JSON PROFILE cpu|cuda GPU_MEMORY_LIMIT".into());
    }
    let offset = if infer { 3 } else { 1 };
    let dest = &a[offset + 4];
    for path in [
        PathBuf::from(dest),
        side(dest, "resampled.f32"),
        side(dest, "spect.f32"),
        side(dest, "logits.f32"),
    ] {
        if path.try_exists().map_err(|e| e.to_string())? {
            return Err("trial output already exists".into());
        }
    }
    let rate: u32 = a[offset + 3].parse().map_err(|_| "invalid sample rate")?;
    // Complete geometry is admitted before any PCM allocation or native load.
    let (signal, source_digest) = pcm(Path::new(&a[offset + 2]), rate)?;
    let source_frames = signal.len();
    let source_duration = source_frames as f64 / rate as f64;
    let soxr = Soxr::load(Path::new(&a[offset]), &a[offset + 1])?;
    let prepared = soxr.resample(&signal, rate, &|| false)?;
    // Resampling has finished and owns its complete output, including the tail.
    drop(signal);
    let spect = beat_audio::log_mel(&prepared, &|| false)?;
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
        let graph = bounded(Path::new(&a[1]), MAX_GRAPH_BYTES)?;
        validate_graph(&graph, &a[2])?;
        let admitted = configure(a, evidence, arena)?;
        let mut model = BeatModel::load_on(&graph, &a[2], Path::new(&a[8]), provider, &|| false)?;
        // The copying session owns its model; no run/profile/output reads graph.
        drop(graph);
        let mut marked = false;
        let cancel_ms = std::env::var("FLITZIS_TRIAL_CANCEL_MS")
            .ok()
            .map(|s| s.parse::<u64>())
            .transpose()
            .map_err(|_| "invalid diagnostic cancellation interval")?;
        if cancel_ms.is_some_and(|n| !(1..=1000).contains(&n)) {
            return Err("invalid diagnostic cancellation interval".into());
        }
        let result = beat_audio::predict(
            &spect,
            |chunk| {
                // This diagnostic marker precedes ORT invocation. It has no product
                // lifecycle authority and alone cannot prove execution has begun.
                eprintln!("native_ort_run_start frames={}", chunk.len() / 128);
                if !marked && let Some(path) = std::env::var_os("FLITZIS_TRIAL_RUN_MARKER") {
                    let mut marker = File::create_new(path).map_err(|e| e.to_string())?;
                    marker
                        .write_all(b"ORT invocation follows\n")
                        .map_err(|e| e.to_string())?;
                    marked = true;
                }
                if let Some(ms) = cancel_ms {
                    let options = RunOptions::new().map_err(|e| e.to_string())?;
                    std::thread::scope(|scope| {
                        let timer = scope.spawn(|| {
                            std::thread::sleep(std::time::Duration::from_millis(ms));
                            options.terminate()
                        });
                        let result = model.run_with_options(chunk, &options, &|| false);
                        timer
                            .join()
                            .map_err(|_| "diagnostic cancellation thread failed")?
                            .map_err(|e| e.to_string())?;
                        if result.is_ok() {
                            return Err("diagnostic run finished before termination".into());
                        }
                        result
                    })
                } else {
                    model.run(chunk, &|| false)
                }
            },
            &|| false,
        );
        let profile = model.finish_profile()?;
        drop(model);
        let logits = match result {
            Ok(logits) => logits,
            Err(error) => {
                if cancel_ms.is_some() {
                    println!(
                        "{}",
                        serde_json::json!({"trial_cancel_diagnostic":true,"profile":profile,"error":error})
                    );
                }
                return Err(error);
            }
        };
        let (beats, downbeats) = beat_audio::postprocess(&logits, source_duration)?;
        metadata["beat_seconds"] = serde_json::json!(beats);
        metadata["downbeat_seconds"] = serde_json::json!(downbeats);
        metadata["profile"] = serde_json::json!(profile);
        metadata["requested_provider"] = serde_json::json!(a[9]);
        metadata["graph_sha256"] = serde_json::json!(a[2]);
        if let Some(runtime) = &admitted {
            metadata["runtime_admission"] = runtime.receipt()?;
        }
        let values: Vec<_> = logits.beat.into_iter().chain(logits.downbeat).collect();
        float_file(&side(dest, "logits.f32"), &values)?;
        admitted
    } else {
        None
    };
    float_file(&side(dest, "resampled.f32"), &prepared)?;
    float_file(&side(dest, "spect.f32"), &spect)?;
    // Metadata is exclusive and last; failed partial scratch never proves success.
    let mut file = File::create_new(dest).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut file, &metadata).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    println!("{metadata}");
    Ok(())
}
fn main() -> ExitCode {
    let a: Vec<_> = std::env::args().skip(1).collect();
    let diagnostic = std::env::var_os("FLITZIS_TRIAL_CUDA_ARENA_DIAGNOSTIC");
    let arena_diagnostic = diagnostic.as_deref() == Some(std::ffi::OsStr::new("1"))
        && a.len() == 11
        && a[0] == "infer"
        && a[9] == "cuda"
        && a[10] == "1"
        && std::env::var_os("FLITZIS_TRIAL_CANCEL_MS").is_none();
    if diagnostic.is_some() && !arena_diagnostic {
        eprintln!("invalid explicit CUDA arena diagnostic request");
        return ExitCode::FAILURE;
    }
    if !a.first().is_some_and(|s| s == "infer" || s == "runtime")
        && (std::env::var_os(runtime::MANIFEST_ENV).is_some()
            || std::env::var_os(runtime::DIGEST_ENV).is_some()
            || runtime::package_requested())
    {
        eprintln!("runtime configuration requires an inference or runtime probe command");
        return ExitCode::FAILURE;
    }
    let evidence = Arc::new(Mutex::new(fault::Evidence::default()));
    if a.first().is_some_and(|s| s == "runtime") {
        let admitted = match configure(&a, &evidence, arena_diagnostic) {
            Ok(admitted) => admitted,
            Err(error) => {
                eprintln!("Error: {error}");
                return ExitCode::FAILURE;
            }
        };
        let build = ort::info();
        let receipt = match admitted
            .as_ref()
            .map(runtime::Admission::receipt)
            .transpose()
        {
            Ok(receipt) => receipt,
            Err(error) => {
                eprintln!("Error: {error}");
                return ExitCode::FAILURE;
            }
        };
        println!(
            "{}",
            serde_json::json!({"trial_only":true,"trial_runtime_probe":true,
            "runtime_admission":receipt,
            "ort_build":build})
        );
        return ExitCode::SUCCESS;
    }
    match execute(&a, &evidence, arena_diagnostic) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let thread = format!("{:?}", std::thread::current().id());
            let rounded = arena_diagnostic
                .then(|| evidence.lock().ok()?.fault(&thread, &error))
                .flatten();
            if let Some(rounded) = rounded {
                // The parent independently checks actual ORT stderr and waits for
                // tree AND reader settlement before any same-model CPU process.
                println!(
                    "{}",
                    serde_json::json!({"trial_only":true,"trial_cuda_arena_fault":true,
                    "graph_sha256":a[2],"gpu_memory_limit":1,"thread":thread,
                    "rounded_bytes":rounded,"error":error})
                );
                ExitCode::from(70)
            } else {
                eprintln!("Error: {error}");
                ExitCode::FAILURE
            }
        }
    }
}

fn configure(
    a: &[String],
    evidence: &Arc<Mutex<fault::Evidence>>,
    arena: bool,
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
    if !probe && !(a.len() == 11 && a[0] == "infer") {
        if requested.is_some() {
            return Err(
                "runtime configuration requires a valid inference or runtime probe command".into(),
            );
        }
        return Ok(None);
    }
    // This is the first ORT access in this standalone process. Retain the checked
    // files until every session/run/profile operation in execute has returned.
    let (admitted, mut environment) =
        runtime::configure(requested.as_ref().map(|(p, d)| (p.as_path(), d.as_str())))?;
    if arena || std::env::var_os("FLITZIS_TRIAL_ORT_LOG").is_some() {
        // Process-local standalone diagnostics; never used by the App/core.
        let observed = Arc::clone(&evidence);
        environment = environment.with_logger(Arc::new(move |level,category,id,location,message| {
            if message.contains("BFCArena") || level >= ort::logging::LogLevel::Error {
                let thread = format!("{:?}",std::thread::current().id());
                if let Ok(mut log) = observed.lock() {
                    log.observe(&thread,location,message);
                }
                eprintln!("ORT_TRIAL_LOG {}",serde_json::json!({"thread":thread,"level":format!("{level:?}"),"category":category,"id":id,"location":location,"message":message}));
            }
        }));
    }
    runtime::commit(environment)?;
    Ok(admitted)
}
