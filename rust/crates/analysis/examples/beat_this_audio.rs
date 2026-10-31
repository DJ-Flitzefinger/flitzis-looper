//! Offline complete-source diagnostic CLI, never an application execution receipt.
use flitzis_looper_analysis::{
    beat_audio::{self, MAX_AUDIO_BYTES, Soxr},
    beat_inference::{BeatModel, MAX_GRAPH_BYTES, Provider},
};
use ort::session::RunOptions;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

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

fn execute() -> Result<(), String> {
    let a: Vec<_> = std::env::args().skip(1).collect();
    let infer = a.first().is_some_and(|s| s == "infer");
    if (!infer && (a.len() != 6 || a[0] != "frontend")) || (infer && a.len() != 11) {
        return Err("usage: beat_this_audio frontend SOXR SHA PCM RATE NEW_JSON | infer GRAPH SHA SOXR SHA PCM RATE NEW_JSON PROFILE cpu|cuda GPU_MEMORY_LIMIT".into());
    }
    if infer && std::env::var_os("FLITZIS_TRIAL_ORT_LOG").is_some() {
        // Standalone diagnostic process only: never mutate the shared core's
        // process environment. These logs do not themselves authorize fallback.
        ort::init().with_logger(std::sync::Arc::new(|level,category,id,location,message| {
            if message.contains("BFCArena") || level >= ort::logging::LogLevel::Error {
                eprintln!("ORT_TRIAL_LOG {}",serde_json::json!({"thread":format!("{:?}",std::thread::current().id()),"level":format!("{level:?}"),"category":category,"id":id,"location":location,"message":message}));
            }
        })).commit();
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
    let raw = bounded(Path::new(&a[offset + 2]), MAX_AUDIO_BYTES)?;
    if raw.len() % 4 != 0 {
        return Err("partial float32 PCM".into());
    }
    let signal: Vec<_> = raw
        .chunks_exact(4)
        .map(|x| f32::from_le_bytes(x.try_into().unwrap()))
        .collect();
    let rate: u32 = a[offset + 3].parse().map_err(|_| "invalid sample rate")?;
    let soxr = Soxr::load(Path::new(&a[offset]), &a[offset + 1])?;
    let prepared = soxr.resample(&signal, rate, &|| false)?;
    let spect = beat_audio::log_mel(&prepared, &|| false)?;
    let mut metadata = serde_json::json!({"trial_only":true,"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "source_sha256":format!("{:x}",Sha256::digest(&raw)),"source_frames":signal.len(),"rate":rate,
        "resampled_frames":prepared.len(),"spectrogram_frames":spect.len()/128,
        "chunk_starts":beat_audio::chunk_starts(spect.len()/128)?,"soxr_sha256":a[offset+1]});
    if infer {
        let provider = match a[9].as_str() {
            "cpu" => Provider::Cpu,
            #[cfg(feature = "native-inference-cuda")]
            "cuda" => Provider::Cuda {
                memory_limit: a[10].parse().map_err(|_| "invalid GPU limit")?,
            },
            _ => return Err("unsupported explicit provider".into()),
        };
        let graph = bounded(Path::new(&a[1]), MAX_GRAPH_BYTES)?;
        let mut model = BeatModel::load_on(&graph, &a[2], Path::new(&a[8]), provider, &|| false)?;
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
        let (beats, downbeats) =
            beat_audio::postprocess(&logits, signal.len() as f64 / rate as f64)?;
        metadata["beat_seconds"] = serde_json::json!(beats);
        metadata["downbeat_seconds"] = serde_json::json!(downbeats);
        metadata["profile"] = serde_json::json!(profile);
        metadata["requested_provider"] = serde_json::json!(a[9]);
        metadata["graph_sha256"] = serde_json::json!(a[2]);
        let values: Vec<_> = logits.beat.into_iter().chain(logits.downbeat).collect();
        float_file(&side(dest, "logits.f32"), &values)?;
    }
    float_file(&side(dest, "resampled.f32"), &prepared)?;
    float_file(&side(dest, "spect.f32"), &spect)?;
    // Metadata is exclusive and last; failed partial scratch never proves success.
    let mut file = File::create_new(dest).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut file, &metadata).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    println!("{metadata}");
    Ok(())
}
fn main() -> Result<(), String> {
    execute()
}
