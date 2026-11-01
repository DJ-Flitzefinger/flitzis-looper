//! Exclusive private diagnostic output, never an application publication receipt.
#[path = "support/workspace.rs"]
mod workspace;

#[global_allocator]
static WORKSPACE: workspace::WorkspaceAllocator = workspace::WorkspaceAllocator::new();

use flitzis_stem_dsp::{
    Error, Geometry, RATE, Spectrum,
    learned::{Model, activation_bytes, weight_bytes},
    track::{TrackPlan, process_track},
};
use std::{
    fs::File,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
    process::ExitCode,
};

fn write(path: &Path, values: &[f32]) -> Result<(), String> {
    let mut file = BufWriter::new(File::create_new(path).map_err(|e| e.to_string())?);
    for value in values {
        file.write_all(&value.to_le_bytes())
            .map_err(|e| e.to_string())?;
    }
    file.flush().map_err(|e| e.to_string())?;
    file.get_ref().sync_all().map_err(|e| e.to_string())
}

fn reserved(len: usize, phase: &'static str) -> Result<Vec<f32>, String> {
    let requested_bytes = len
        .checked_mul(size_of::<f32>())
        .ok_or("source extent overflow")?;
    let mut values = Vec::new();
    values.try_reserve_exact(len).map_err(|_| {
        Error::Allocation {
            phase,
            requested_bytes,
        }
        .to_string()
    })?;
    Ok(values)
}

fn execute() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--workspace-control" {
        return workspace_control();
    }
    if !(6..=7).contains(&args.len()) {
        return Err("usage: bs_learned WEIGHTS SHA256 chunk|track PCM_F32 FRAMES NEW_PRIVATE_DIRECTORY [ORIGINAL_FEATURES_F32]".into());
    }
    let digest = args[1].to_str().ok_or("invalid digest")?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("invalid digest".into());
    }
    let mut expected = [0u8; 32];
    for (i, value) in expected.iter_mut().enumerate() {
        *value = u8::from_str_radix(&digest[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string())?;
    }
    let frames: usize = args[4]
        .to_str()
        .ok_or("invalid frame count")?
        .parse()
        .map_err(|_| "invalid frame count")?;
    let mode = args[2].to_str().ok_or("invalid mode")?;
    if !["chunk", "track"].contains(&mode) {
        return Err("invalid mode".into());
    }
    if args.len() == 7 && mode != "chunk" {
        return Err("original features require chunk mode".into());
    }
    let source_bytes = frames.checked_mul(8).ok_or("source extent overflow")?;
    if mode == "chunk" {
        Geometry::new(frames, RATE).map_err(|e| e.to_string())?;
    } else {
        TrackPlan::new(frames, RATE).map_err(|e| e.to_string())?;
    }
    let mut source = BufReader::new(File::open(&args[3]).map_err(|e| e.to_string())?);
    if source
        .get_ref()
        .metadata()
        .map_err(|e| e.to_string())?
        .len()
        != source_bytes as u64
    {
        return Err("incomplete source".into());
    }
    let mut original_features = if args.len() == 7 {
        let file = File::open(&args[6]).map_err(|e| e.to_string())?;
        if file.metadata().map_err(|e| e.to_string())?.len()
            != Geometry::new(frames, RATE)
                .map_err(|e| e.to_string())?
                .feature_len() as u64
                * 4
        {
            return Err("incomplete original features".into());
        }
        Some(BufReader::new(file))
    } else {
        None
    };
    let mut weights = BufReader::new(File::open(&args[0]).map_err(|e| e.to_string())?);
    if weights
        .get_ref()
        .metadata()
        .map_err(|e| e.to_string())?
        .len()
        != weight_bytes() as u64
    {
        return Err("incomplete fixed-schema weights".into());
    }
    // External ORIGINAL admission must precede this process. The caller chooses
    // a source-backed extent, including allocation metadata/fragmentation; this
    // executable never derives a process budget from free RAM or reduces one.
    // Requiring it avoids accidentally running the old unbounded Rust heap path.
    let capacity = std::env::var("FLITZIS_BS_WORKSPACE_BYTES")
        .map_err(|_| "FLITZIS_BS_WORKSPACE_BYTES requires complete external original admission")?
        .parse::<usize>()
        .map_err(|_| "invalid FLITZIS_BS_WORKSPACE_BYTES")?;
    WORKSPACE.seal(capacity)?;
    // Existing private directory is never overwritten, even when later load or
    // arithmetic fails. Every earlier partial file is invalid without exit0.
    let output = Path::new(&args[5]);
    std::fs::create_dir(output).map_err(|e| e.to_string())?;
    let model = Model::load(&mut weights, &expected, &|| false).map_err(|e| e.to_string())?;
    if mode == "chunk" {
        let mut pcm = reserved(frames * 2, "learned-trial.pcm")?;
        for _ in 0..frames * 2 {
            let mut value = [0; 4];
            source.read_exact(&mut value).map_err(|e| e.to_string())?;
            pcm.push(f32::from_le_bytes(value));
        }
        let spectral = Spectrum::analyze(&pcm, RATE, &|| false).map_err(|e| e.to_string())?;
        drop(pcm);
        let geometry = spectral.geometry();
        let masks = model
            .masks(spectral.features(), geometry.steps(), &|| false)
            .map_err(|e| e.to_string())?;
        let prediction = spectral
            .reconstruct(&masks, &|| false)
            .map_err(|e| e.to_string())?;
        write(&output.join("features.f32"), spectral.features())?;
        write(&output.join("masks.f32"), &masks)?;
        write(&output.join("output.f32"), &prediction)?;
        // These private diagnostic buffers have no readers after the writes.
        // Do not retain the whole first forward during the independent oracle-
        // feature forward. A later failure still invalidates ALL private files.
        drop(prediction);
        drop(masks);
        drop(spectral);
        if let Some(reader) = &mut original_features {
            let mut features = reserved(geometry.feature_len(), "learned-trial.original-features")?;
            for _ in 0..geometry.feature_len() {
                let mut raw = [0; 4];
                reader.read_exact(&mut raw).map_err(|e| e.to_string())?;
                features.push(f32::from_le_bytes(raw));
            }
            let original_masks = model
                .masks(&features, geometry.steps(), &|| false)
                .map_err(|e| e.to_string())?;
            write(&output.join("original_features_masks.f32"), &original_masks)?;
        }
        println!(
            "BS learned CPU chunk trial: frames={frames} weights={} learned_payload_allowance={}",
            weight_bytes(),
            activation_bytes(geometry.steps()).map_err(|e| e.to_string())?
        );
    } else {
        let plan = TrackPlan::new(frames, RATE).map_err(|e| e.to_string())?;
        let mut sink =
            BufWriter::new(File::create_new(output.join("output.f32")).map_err(|e| e.to_string())?);
        let mut written = 0;
        process_track(
            plan,
            |start, count, pcm| {
                source
                    .seek(SeekFrom::Start((start * 8) as u64))
                    .map_err(|_| Error::Source)?;
                for i in 0..count {
                    for c in 0..2 {
                        let mut raw = [0; 4];
                        source.read_exact(&mut raw).map_err(|_| Error::Source)?;
                        pcm[c * count + i] = f32::from_le_bytes(raw);
                    }
                }
                Ok(count)
            },
            |input, batch| model.track_batch(input, batch, &|| false),
            |start, count, pcm| {
                if start != written {
                    return Err(Error::Output);
                }
                for i in 0..count {
                    for lane in 0..8 {
                        sink.write_all(&pcm[lane * count + i].to_le_bytes())
                            .map_err(|_| Error::Output)?;
                    }
                }
                written += count;
                Ok(())
            },
            &|| false,
        )
        .map_err(|e| e.to_string())?;
        if written != frames {
            return Err("incomplete private track".into());
        }
        sink.flush().map_err(|e| e.to_string())?;
        sink.get_ref().sync_all().map_err(|e| e.to_string())?;
        println!(
            "BS learned CPU track trial: frames={frames} chunks={}",
            plan.chunks()
        );
    }
    Ok(())
}

fn workspace_control() -> Result<(), String> {
    // Implementation control ONLY: no weights, learned network or admission
    // claim. Use actual pinned SGEMM packing and actual FFT/spectral ownership.
    WORKSPACE.seal(1 << 20)?;
    let before = WORKSPACE.stats();
    {
        let a = vec![0.25f32; 32 * 384];
        let b = vec![0.5f32; 384 * 64];
        let mut c = vec![0.0f32; 32 * 64];
        let baseline = WORKSPACE.stats();
        for _ in 0..64 {
            // SAFETY: exact contiguous nonaliasing initialized array extents.
            unsafe {
                matrixmultiply::sgemm(
                    32,
                    384,
                    64,
                    1.0,
                    a.as_ptr(),
                    384,
                    1,
                    b.as_ptr(),
                    64,
                    1,
                    0.0,
                    c.as_mut_ptr(),
                    64,
                    1,
                );
            }
            if c.iter().any(|&value| value != 48.0) {
                return Err("workspace SGEMM arithmetic mismatch".into());
            }
        }
        let packed = WORKSPACE.stats();
        if packed.allocations <= baseline.allocations || packed.live != baseline.live {
            return Err("SGEMM packing did not allocate and retire inside workspace".into());
        }
    }
    let before_fft = WORKSPACE.stats();
    for _ in 0..8 {
        let pcm: Vec<_> = (0..2050).map(|i| (i % 17) as f32 * 0.001 - 0.008).collect();
        let spectrum = Spectrum::analyze(&pcm, RATE, &|| false).map_err(|e| e.to_string())?;
        let mut mask = vec![0.0; spectrum.geometry().mask_len()];
        for pair in mask.chunks_exact_mut(2) {
            pair[0] = 1.0;
        }
        let output = spectrum
            .reconstruct(&mask, &|| false)
            .map_err(|e| e.to_string())?;
        for stem in output.chunks_exact(2050) {
            if stem.iter().zip(&pcm).any(|(a, b)| (a - b).abs() > 1e-7) {
                return Err("workspace FFT identity mismatch".into());
            }
        }
    }
    let after = WORKSPACE.stats();
    if after.allocations <= before_fft.allocations
        || after.live != before.live
        || after.failures != 0
    {
        return Err("workspace FFT ownership did not settle".into());
    }
    println!(
        "BS workspace control: SGEMM64 FFT8 capacity={} peak={} live={} allocations={} failures={}",
        after.capacity, after.peak, after.live, after.allocations, after.failures
    );
    Ok(())
}

fn main() -> ExitCode {
    match execute() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
