//! Exclusive private diagnostic output, never an application publication receipt.
#[path = "support/trial_io.rs"]
mod trial_io;
#[path = "support/workspace.rs"]
mod workspace;

#[global_allocator]
static WORKSPACE: workspace::WorkspaceAllocator = workspace::WorkspaceAllocator::new();

use flitzis_stem_dsp::{
    Error, Geometry, RATE, SpectralPlan, SpectralWorkspace, Spectrum,
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
    if args.len() == 1 && args[0] == "--spectral-workspace-control" {
        return spectral_workspace_control(status);
    }
    if args.len() == 2 && args[0] == "--spectral-workspace-reject-control" {
        let prefix = args[1]
            .to_str()
            .ok_or("invalid prefix")?
            .parse()
            .map_err(|_| "invalid prefix")?;
        return spectral_workspace_rejection(status, prefix);
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
    // Retain selected plans/windows before seal; their System/native backing
    // still needs a complete process allowance. Working vectors use the arena.
    let spectral_plan = SpectralPlan::new(
        if mode == "chunk" {
            frames
        } else {
            flitzis_stem_dsp::track::CHUNK
        },
        RATE,
        &|| false,
    )?;
    WORKSPACE.seal(capacity)?;
    let mut spectral_workspace = SpectralWorkspace::from_plan(spectral_plan, &|| false)?;
    let model = Model::load(&mut weights, &expected, &|| false)?;
    if let Outputs::Chunk(output) = &mut outputs {
        let mut pcm = reserved(frames * 2, "learned-trial.pcm")?;
        for _ in 0..frames * 2 {
            let mut value = [0; 4];
            source.read_exact(&mut value).at("source.read")?;
            pcm.push(f32::from_le_bytes(value));
        }
        let spectral = spectral_workspace.analyze(&pcm, RATE, &|| false)?;
        drop(pcm);
        let geometry = spectral.geometry();
        let masks = model.masks(spectral.features(), geometry.steps(), &|| false)?;
        let prediction = spectral_workspace.reconstruct(&spectral, &masks, &|| false)?;
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
            |input, batch| {
                model.track_batch_with_workspace(input, batch, &mut spectral_workspace, &|| false)
            },
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
        "BS workspace control: SGEMM64 FFT8 capacity={} peak={} live={} allocations={} failures={} mapped_bytes={:?}",
        after.capacity, after.peak, after.live, after.allocations, after.failures,
        WORKSPACE.mapped_bytes(),
    )
    .at("status.write")?;
    Ok(())
}

fn spectral_fixture_capacity(layouts: &[std::alloc::Layout; 6]) -> Result<usize> {
    if layouts.iter().any(|layout| layout.align() > 16) {
        return Err("unsupported spectral fixture alignment".into());
    }
    let geometry = Geometry::new(4099, RATE)?;
    // Fixture source sequence: persistent working prefix, then features/all4
    // masks/output. Every cycle releases all three back to the exact free tail.
    // Count16B headers/rounding and one structural32B minimum splittable tail.
    let working: usize = layouts
        .iter()
        .filter(|layout| layout.size() != 0)
        .map(|layout| (layout.size() + 16).div_ceil(16) * 16)
        .sum();
    Ok(working
        + [geometry.feature_len(), geometry.mask_len(), 8 * 4099]
            .into_iter()
            .map(|floats| (floats * 4 + 16).div_ceil(16) * 16)
            .sum::<usize>()
        + 32)
}

fn spectral_workspace_control(status: &mut impl Write) -> Result<()> {
    // FIXTURE ONLY: original process admission still belongs to the caller.
    // Three later requests per cycle are features, all4 masks and output;
    // the actual retained FFT/inverse path requests NO other Rust backing.
    let plan = SpectralPlan::new(4099, RATE, &|| false)?;
    let layouts = plan.allocation_layouts();
    let capacity = spectral_fixture_capacity(&layouts)?;
    let pcm: Vec<_> = (0..2 * 4099)
        .map(|i| (i % 17) as f32 * 0.001 - 0.008)
        .collect();
    WORKSPACE.seal(capacity)?;
    let prefix = WORKSPACE.stats();
    let mut workspace = SpectralWorkspace::from_plan(plan, &|| false)?;
    let before = WORKSPACE.stats();
    let requests = layouts.iter().filter(|layout| layout.size() != 0).count();
    if before.allocations - prefix.allocations != requests || before.failures != 0 {
        return Err("spectral vector preparation changed its source recipe".into());
    }
    for frames in [4099, 1025, 2501, 4099, 1025] {
        let spectrum = workspace.analyze(&pcm[..2 * frames], RATE, &|| false)?;
        let mut mask = vec![0.0; spectrum.geometry().mask_len()];
        for pair in mask.chunks_exact_mut(2) {
            pair[0] = 1.0;
        }
        let output = workspace.reconstruct(&spectrum, &mask, &|| false)?;
        if output.chunks_exact(2 * frames).any(|stem| {
            stem.iter()
                .zip(&pcm[..2 * frames])
                .any(|(a, b)| (a - b).abs() > 1e-7)
        }) {
            return Err("retained spectral identity mismatch".into());
        }
    }
    let after = WORKSPACE.stats();
    if after.allocations - before.allocations != 15
        || after.live != before.live
        || after.failures != 0
    {
        return Err("retained spectral workspace backing changed".into());
    }
    drop(workspace);
    let closed = WORKSPACE.stats();
    if closed.live != prefix.live || closed.failures != 0 {
        return Err("spectral working vectors did not retire".into());
    }
    writeln!(status,
        "BS retained spectral control: cycles=5 payload_requests=15 working_requests={requests} working_bytes={} capacity={} peak={} live={} failures={} mapped_bytes={:?}",
        layouts.iter().map(|layout| layout.size()).sum::<usize>(),
        closed.capacity, closed.peak, closed.live, closed.failures,
        WORKSPACE.mapped_bytes(),
    ).at("status.write")?;
    Ok(())
}

fn spectral_workspace_rejection(status: &mut impl Write, prefix: usize) -> Result<()> {
    // Deliberately insufficient FIXTURE ONLY, never a model/process budget.
    let plan = SpectralPlan::new(4099, RATE, &|| false)?;
    let layouts = plan.allocation_layouts();
    if prefix >= layouts.len() || layouts.iter().any(|layout| layout.size() == 0) {
        return Err("unsupported rejection fixture prefix".into());
    }
    let phases = [
        "analyze.fft",
        "analyze.scratch",
        "inverse.fft",
        "inverse.scratch",
        "inverse.envelope",
        "inverse.accumulation",
    ];
    // These fixture layouts have alignment<=16. Each uses one16B header and
    // rounds its extent to16; keep one minimum32B free tail for the next refusal.
    if layouts.iter().any(|layout| layout.align() > 16) {
        return Err("unsupported rejection fixture alignment".into());
    }
    let capacity = 32
        + layouts[..prefix]
            .iter()
            .map(|layout| (layout.size() + 16).div_ceil(16) * 16)
            .sum::<usize>();
    WORKSPACE.seal(capacity)?;
    let expected = Error::Allocation {
        phase: phases[prefix],
        requested_bytes: layouts[prefix].size(),
    };
    match SpectralWorkspace::from_plan(plan, &|| false) {
        Err(error) if error == expected => {}
        _ => return Err("spectral working-vector rejection changed".into()),
    }
    let after = WORKSPACE.stats();
    if after.live != 0 || after.failures != 1 || after.allocations != prefix {
        return Err("spectral prefix refusal did not retire all arena owners".into());
    }
    writeln!(
        status,
        "BS spectral rejection control: prefix={prefix} error=({expected}) live=0 failures=1"
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
