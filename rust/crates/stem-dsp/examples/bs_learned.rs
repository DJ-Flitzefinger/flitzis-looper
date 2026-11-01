//! Exclusive private diagnostic output, never an application publication receipt.
#[path = "support/trial_io.rs"]
mod trial_io;
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
    io::{BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
    process::ExitCode,
};
use trial_io::{BUFFER_BYTES, IoPhase, Outputs, Result, core_io};

fn reserved(len: usize, phase: &'static str) -> Result<Vec<f32>> {
    let requested_bytes = len
        .checked_mul(size_of::<f32>())
        .ok_or("source extent overflow")?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| Error::Allocation {
            phase,
            requested_bytes,
        })?;
    Ok(values)
}

fn execute(status: &mut impl Write) -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--workspace-control" {
        return workspace_control(status);
    }
    if args.len() == 2 && args[0] == "--io-control" {
        return trial_io::control(Path::new(&args[1]), &WORKSPACE, status);
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
        *value = u8::from_str_radix(&digest[i * 2..i * 2 + 2], 16).map_err(|_| "invalid digest")?;
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
        Geometry::new(frames, RATE)?;
    } else {
        TrackPlan::new(frames, RATE)?;
    }
    let mut source =
        BufReader::with_capacity(BUFFER_BYTES, File::open(&args[3]).at("source.open")?);
    if source.get_ref().metadata().at("source.metadata")?.len() != source_bytes as u64 {
        return Err("incomplete source".into());
    }
    let mut original_features = if args.len() == 7 {
        let file = File::open(&args[6]).at("original-features.open")?;
        if file.metadata().at("original-features.metadata")?.len()
            != Geometry::new(frames, RATE)?.feature_len() as u64 * 4
        {
            return Err("incomplete original features".into());
        }
        Some(BufReader::with_capacity(BUFFER_BYTES, file))
    } else {
        None
    };
    let mut weights =
        BufReader::with_capacity(BUFFER_BYTES, File::open(&args[0]).at("weights.open")?);
    if weights.get_ref().metadata().at("weights.metadata")?.len() != weight_bytes() as u64 {
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
    workspace::WorkspaceAllocator::validate_extent(capacity)?;
    // Paths, exclusive handles and buffered storage are prepared BEFORE seal.
    // A later failure invalidates ALL new empty/partial private files. Nothing
    // in the sealed phase opens/reopens an output or formats platform error text.
    let mut outputs = Outputs::prepare(Path::new(&args[5]), mode == "chunk", args.len() == 7)?;
    WORKSPACE.seal(capacity)?;
    let model = Model::load(&mut weights, &expected, &|| false)?;
    if let Outputs::Chunk(output) = &mut outputs {
        let mut pcm = reserved(frames * 2, "learned-trial.pcm")?;
        for _ in 0..frames * 2 {
            let mut value = [0; 4];
            source.read_exact(&mut value).at("source.read")?;
            pcm.push(f32::from_le_bytes(value));
        }
        let spectral = Spectrum::analyze(&pcm, RATE, &|| false)?;
        drop(pcm);
        let geometry = spectral.geometry();
        let masks = model.masks(spectral.features(), geometry.steps(), &|| false)?;
        let prediction = spectral.reconstruct(&masks, &|| false)?;
        output.features.write_values(spectral.features())?;
        output.masks.write_values(&masks)?;
        output.prediction.write_values(&prediction)?;
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
                reader.read_exact(&mut raw).at("original-features.read")?;
                features.push(f32::from_le_bytes(raw));
            }
            let original_masks = model.masks(&features, geometry.steps(), &|| false)?;
            output
                .original_masks
                .as_mut()
                .ok_or("missing prepared original-feature output")?
                .write_values(&original_masks)?;
        }
        writeln!(
            status,
            "BS learned CPU chunk trial: frames={frames} weights={} learned_payload_allowance={}",
            weight_bytes(),
            activation_bytes(geometry.steps())?
        )
        .at("status.write")?;
    } else if let Outputs::Track(sink) = &mut outputs {
        let plan = TrackPlan::new(frames, RATE)?;
        let mut written = 0;
        let mut source_error = None;
        let mut sink_error = None;
        let result = process_track(
            plan,
            |start, count, pcm| {
                core_io(
                    source.seek(SeekFrom::Start((start * 8) as u64)),
                    "source.seek",
                    &mut source_error,
                    Error::Source,
                )?;
                for i in 0..count {
                    for c in 0..2 {
                        let mut raw = [0; 4];
                        core_io(
                            source.read_exact(&mut raw),
                            "source.read",
                            &mut source_error,
                            Error::Source,
                        )?;
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
                        core_io(
                            sink.writer.write_all(&pcm[lane * count + i].to_le_bytes()),
                            "output.pcm.write",
                            &mut sink_error,
                            Error::Output,
                        )?;
                    }
                }
                written += count;
                Ok(())
            },
            &|| false,
        );
        if let Err(error) = result {
            return Err(source_error.or(sink_error).unwrap_or_else(|| error.into()));
        }
        if written != frames {
            return Err("incomplete private track".into());
        }
        sink.finish()?;
        writeln!(
            status,
            "BS learned CPU track trial: frames={frames} chunks={}",
            plan.chunks()
        )
        .at("status.write")?;
    }
    Ok(())
}

fn workspace_control(status: &mut impl Write) -> Result<()> {
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
        let spectrum = Spectrum::analyze(&pcm, RATE, &|| false)?;
        let mut mask = vec![0.0; spectrum.geometry().mask_len()];
        for pair in mask.chunks_exact_mut(2) {
            pair[0] = 1.0;
        }
        let output = spectrum.reconstruct(&mask, &|| false)?;
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
    writeln!(
        status,
        "BS workspace control: SGEMM64 FFT8 capacity={} peak={} live={} allocations={} failures={}",
        after.capacity, after.peak, after.live, after.allocations, after.failures
    )
    .at("status.write")?;
    Ok(())
}

fn main() -> ExitCode {
    // Initialize and retain the lazy standard streams before any seal, including
    // the error path after execute returns. Formatting uses no OS error text.
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let mut status = stdout.lock();
    let mut errors = stderr.lock();
    let result = execute(&mut status).and_then(|()| status.flush().at("status.flush"));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(errors, "{error}");
            ExitCode::FAILURE
        }
    }
}
