//! Developer-only one-chunk CLI; output is diagnostic logits, never an App receipt.
use flitzis_looper_analysis::beat_inference::{BINS, BeatModel, FRAMES, MAX_GRAPH_BYTES};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

fn bounded(path: &Path, maximum: usize) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(maximum as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() > maximum {
        return Err("trial input exceeds byte bound".into());
    }
    Ok(data)
}

fn main() -> Result<(), String> {
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
    let graph = bounded(Path::new(&args[0]), MAX_GRAPH_BYTES)?;
    let raw = bounded(Path::new(&args[2]), FRAMES * BINS * 4)?;
    if raw.len() != FRAMES * BINS * 4 {
        return Err("incomplete trial spectrogram".into());
    }
    let spect: Vec<_> = raw
        .chunks_exact(4)
        .map(|x| f32::from_le_bytes(x.try_into().unwrap()))
        .collect();
    if !spect.iter().all(|x| x.is_finite()) {
        return Err("nonfinite trial spectrogram".into());
    }
    let digest = args[1].to_str().ok_or("invalid digest encoding")?;
    let mut model = BeatModel::load(&graph, digest, Path::new(&args[4]), &|| false)?;
    let logits = model.run(&spect, &|| false)?;
    let profile = model.finish_profile()?;
    // Exclusive creation prevents overwriting an earlier result or an input.
    let mut output = File::create_new(&args[3]).map_err(|e| e.to_string())?;
    for value in logits.beat.iter().chain(&logits.downbeat) {
        output
            .write_all(&value.to_le_bytes())
            .map_err(|e| e.to_string())?;
    }
    output.sync_all().map_err(|e| e.to_string())?;
    println!(
        "{}",
        serde_json::json!({"trial_only":true,"os":std::env::consts::OS,
        "arch":std::env::consts::ARCH,"frames":FRAMES,"profile":profile})
    );
    Ok(())
}
