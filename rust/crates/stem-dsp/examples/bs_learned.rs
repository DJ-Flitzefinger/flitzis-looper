//! Exclusive private diagnostic output, never an application publication receipt.
#[path = "support/cancellation.rs"]
mod cancellation;
#[path = "support/terminal.rs"]
mod terminal;
#[path = "support/trial_io.rs"]
mod trial_io;
#[path = "support/workspace.rs"]
mod workspace;

#[global_allocator]
static WORKSPACE: workspace::WorkspaceAllocator = workspace::WorkspaceAllocator::new();

use cancellation::Cancellation;
use flitzis_stem_dsp::{
    Error, Geometry, RATE, SpectralPlan, SpectralWorkspace, Spectrum,
    learned::{Model, activation_bytes, weight_bytes},
    track::{TrackPlan, process_track},
};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    process::ExitCode,
};
use trial_io::{Inputs, IoPhase, Outputs, Result, core_io};

enum TrialReport {
    Chunk { frames: usize, allowance: usize },
    Track { frames: usize, chunks: usize },
}

impl TrialReport {
    fn write(self, status: &mut impl Write) -> Result<()> {
        match self {
            Self::Chunk { frames, allowance } => writeln!(
                status,
                "BS learned CPU chunk trial: frames={frames} weights={} learned_payload_allowance={allowance}",
                weight_bytes(),
            ),
            Self::Track { frames, chunks } => writeln!(
                status,
                "BS learned CPU track trial: frames={frames} chunks={chunks}",
            ),
        }
        .at("status.write")
    }
}

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

fn execute(
    status: &mut impl Write,
    cancel: &Cancellation,
    cleanup: &mut Result<()>,
    private: &mut trial_io::PrivateOutputs,
) -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 2 && args[0] == "--cancellation-control" {
        return cancellation_control(Path::new(&args[1]), status, cancel, cleanup);
    }
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
        return trial_io::control(Path::new(&args[1]), &WORKSPACE, status, private);
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
    let original = if args.len() == 7 {
        Some((
            Path::new(&args[6]),
            Geometry::new(frames, RATE)?.feature_len() as u64 * 4,
        ))
    } else {
        None
    };
    // All ordinary exits after the first open reach consuming completion. Inner
    // numerical borrows and callback readers end before ownership is consumed.
    let mut inputs = Inputs::default();
    let mut outputs = None;
    let mut result = (|| {
        cancel.checkpoint("input.prepare")?;
        inputs.prepare(
            Path::new(&args[3]),
            source_bytes as u64,
            Path::new(&args[0]),
            weight_bytes() as u64,
            original,
        )?;
        // External ORIGINAL admission must precede this process. The caller chooses
        // a source-backed extent, including allocation metadata/fragmentation; this
        // executable never derives a process budget from free RAM or reduces one.
        // Requiring it avoids accidentally running the old unbounded Rust heap path.
        let capacity = std::env::var("FLITZIS_BS_WORKSPACE_BYTES")
            .map_err(
                |_| "FLITZIS_BS_WORKSPACE_BYTES requires complete external original admission",
            )?
            .parse::<usize>()
            .map_err(|_| "invalid FLITZIS_BS_WORKSPACE_BYTES")?;
        workspace::WorkspaceAllocator::validate_extent(capacity)?;
        // Paths, exclusive handles and buffered storage are prepared BEFORE seal.
        // A later failure invalidates ALL new empty/partial private files. Nothing
        // in the sealed phase opens/reopens an output or formats platform error text.
        cancel.checkpoint("output.prepare")?;
        outputs = Some(Outputs::prepare(
            Path::new(&args[5]),
            mode == "chunk",
            args.len() == 7,
        )?);
        // Retain selected plans/windows before seal; their bootstrap/native backing
        // still needs a complete process allowance. Working vectors use the arena.
        cancel.checkpoint("spectral.plan")?;
        let spectral_plan = SpectralPlan::new(
            if mode == "chunk" {
                frames
            } else {
                flitzis_stem_dsp::track::CHUNK
            },
            RATE,
            &|| cancel.cancelled(),
        )?;
        cancel.checkpoint("workspace.seal")?;
        WORKSPACE.seal(capacity)?;
        cancel.checkpoint("spectral.prepare")?;
        let mut spectral_workspace =
            SpectralWorkspace::from_plan(spectral_plan, &|| cancel.cancelled())?;
        cancel.checkpoint("model.load")?;
        let model = Model::load(inputs.weights.as_mut().unwrap(), &expected, &|| {
            cancel.cancelled()
        })?;
        let source = inputs.source.as_mut().unwrap();
        let outputs = outputs.as_mut().unwrap();
        if let Outputs::Chunk(output) = outputs {
            let mut pcm = reserved(frames * 2, "learned-trial.pcm")?;
            cancel.checkpoint("source.read")?;
            read_values(source, frames * 2, "source.read", cancel, |_, value| {
                pcm.push(value)
            })?;
            cancel.checkpoint("spectral.analyze")?;
            let spectral = spectral_workspace.analyze(&pcm, RATE, &|| cancel.cancelled())?;
            drop(pcm);
            let geometry = spectral.geometry();
            cancel.checkpoint("model.masks")?;
            let masks = model.masks(spectral.features(), geometry.steps(), &|| {
                cancel.cancelled()
            })?;
            cancel.checkpoint("spectral.reconstruct")?;
            let prediction =
                spectral_workspace.reconstruct(&spectral, &masks, &|| cancel.cancelled())?;
            cancel.checkpoint("output.features")?;
            output
                .features
                .write_values_checked(spectral.features(), &|| cancel.check())?;
            cancel.checkpoint("output.masks")?;
            output
                .masks
                .write_values_checked(&masks, &|| cancel.check())?;
            cancel.checkpoint("output.pcm")?;
            output
                .prediction
                .write_values_checked(&prediction, &|| cancel.check())?;
            // These private diagnostic buffers have no readers after the writes.
            // Do not retain the whole first forward during the independent oracle-
            // feature forward. A later failure still invalidates ALL private files.
            drop(prediction);
            drop(masks);
            drop(spectral);
            if let Some(reader) = &mut inputs.original {
                let mut features =
                    reserved(geometry.feature_len(), "learned-trial.original-features")?;
                cancel.checkpoint("original-features.read")?;
                read_values(
                    reader,
                    geometry.feature_len(),
                    "original-features.read",
                    cancel,
                    |_, value| features.push(value),
                )?;
                cancel.checkpoint("original-features.masks")?;
                let original_masks =
                    model.masks(&features, geometry.steps(), &|| cancel.cancelled())?;
                cancel.checkpoint("output.original-masks")?;
                output
                    .original_masks
                    .as_mut()
                    .ok_or("missing prepared original-feature output")?
                    .write_values_checked(&original_masks, &|| cancel.check())?;
            }
            Ok(TrialReport::Chunk {
                frames,
                allowance: activation_bytes(geometry.steps())?,
            })
        } else if let Outputs::Track(sink) = outputs {
            let plan = TrackPlan::new(frames, RATE)?;
            let mut written = 0;
            let mut source_error = None;
            let mut sink_error = None;
            let result = process_track(
                plan,
                |start, count, pcm| {
                    cancel
                        .checkpoint("track.source")
                        .map_err(|_| Error::Cancelled)?;
                    core_io(
                        source.seek(SeekFrom::Start((start * 8) as u64)),
                        "source.seek",
                        &mut source_error,
                        Error::Source,
                    )?;
                    if let Err(error) =
                        read_values(source, count * 2, "source.read", cancel, |index, value| {
                            pcm[(index % 2) * count + index / 2] = value
                        })
                    {
                        if matches!(error, trial_io::Failure::Core(Error::Cancelled)) {
                            return Err(Error::Cancelled);
                        }
                        source_error.get_or_insert(error);
                        return Err(Error::Source);
                    }
                    Ok(count)
                },
                |input, batch| {
                    cancel
                        .checkpoint("track.predict")
                        .map_err(|_| Error::Cancelled)?;
                    model.track_batch_with_workspace(input, batch, &mut spectral_workspace, &|| {
                        cancel.cancelled()
                    })
                },
                |start, count, pcm| {
                    cancel
                        .checkpoint("track.output")
                        .map_err(|_| Error::Cancelled)?;
                    if start != written {
                        return Err(Error::Output);
                    }
                    let mut bytes = [0u8; trial_io::BUFFER_BYTES];
                    for base in (0..count).step_by(bytes.len() / 32) {
                        cancel.check().map_err(|_| Error::Cancelled)?;
                        let frames = (count - base).min(bytes.len() / 32);
                        for i in 0..frames {
                            for lane in 0..8 {
                                let offset = (i * 8 + lane) * 4;
                                bytes[offset..offset + 4]
                                    .copy_from_slice(&pcm[lane * count + base + i].to_le_bytes());
                            }
                        }
                        core_io(
                            sink.writer.write_all(&bytes[..frames * 32]),
                            "output.pcm.write",
                            &mut sink_error,
                            Error::Output,
                        )?;
                    }
                    written += count;
                    Ok(())
                },
                &|| cancel.cancelled(),
            );
            if let Err(error) = result {
                return Err(source_error.or(sink_error).unwrap_or_else(|| error.into()));
            }
            if written != frames {
                return Err("incomplete private track".into());
            }
            Ok(TrialReport::Track {
                frames,
                chunks: plan.chunks(),
            })
        } else {
            unreachable!()
        }
    })();
    if result.is_ok() {
        trial_io::retain_first(&mut result, cancel.checkpoint("output.complete"));
    }
    let result = trial_io::complete_retained(
        result,
        inputs,
        outputs,
        cleanup,
        &|| cancel.checkpoint("output.closed"),
        private,
    );
    result?.write(status)
}

/// Decode bounded blocks, retaining original f32 bits and caller-owned layout.
fn read_values(
    reader: &mut impl Read,
    count: usize,
    phase: &'static str,
    cancel: &Cancellation,
    mut accept: impl FnMut(usize, f32),
) -> Result<()> {
    let mut bytes = [0u8; trial_io::BUFFER_BYTES];
    for start in (0..count).step_by(bytes.len() / 4) {
        cancel.check()?;
        let count = (count - start).min(bytes.len() / 4);
        reader.read_exact(&mut bytes[..count * 4]).at(phase)?;
        for (i, raw) in bytes[..count * 4].chunks_exact(4).enumerate() {
            accept(start + i, f32::from_le_bytes(raw.try_into().unwrap()));
        }
    }
    cancel.check()
}

fn cancellation_control(
    directory: &Path,
    status: &mut impl Write,
    cancel: &Cancellation,
    cleanup: &mut Result<()>,
) -> Result<()> {
    if !cancel.bound() {
        return Err("cancellation control requires an own marker channel".into());
    }
    let mut inputs = Inputs::default();
    let mut outputs = None;
    let result = (|| {
        cancel.checkpoint("control.prepare")?;
        outputs = Some(Outputs::prepare(directory, true, true)?);
        let Some(Outputs::Chunk(output)) = &mut outputs else {
            unreachable!()
        };
        output.features.write_values(&[0.25; 8198])?;
        output.features.writer.flush().at("control.flush")?;
        inputs.prepare(
            &directory.join("features.f32"),
            8198 * 4,
            &directory.join("masks.f32"),
            0,
            Some((&directory.join("output.f32"), 0)),
        )?;
        let plan = SpectralPlan::new(4099, RATE, &|| cancel.cancelled())?;
        WORKSPACE.seal(4 << 20)?;
        let mut spectral = SpectralWorkspace::from_plan(plan, &|| cancel.cancelled())?;
        let mut pcm = reserved(8198, "control.pcm")?;
        read_values(
            inputs.source.as_mut().unwrap(),
            8198,
            "control.read",
            cancel,
            |_, value| pcm.push(value),
        )?;
        cancel.checkpoint("control.spectral")?;
        let first = spectral.analyze(&pcm, RATE, &|| cancel.cancelled())?;
        output
            .masks
            .write_values_checked(first.features(), &|| cancel.check())?;
        drop(first);
        writeln!(
            status,
            "BS cancellation control ready: nonneural retained-inputs=3 private-outputs=4"
        )
        .at("status.write")?;
        status.flush().at("status.flush")?;
        loop {
            let _spectrum = spectral.analyze(&pcm, RATE, &|| cancel.cancelled())?;
            // Keep the tiny control bounded in CPU and file growth while waiting.
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    })();
    trial_io::complete(result, inputs, outputs, cleanup, &|| {
        cancel.checkpoint("output.closed")
    })
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
    let bootstrap_before = WORKSPACE.bootstrap_stats();
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
        "BS retained spectral control: cycles=5 payload_requests=15 working_requests={requests} working_bytes={} capacity={} peak={} live={} failures={} mapped_bytes={:?} bootstrap_before={:?} bootstrap_after={:?}",
        layouts.iter().map(|layout| layout.size()).sum::<usize>(),
        closed.capacity, closed.peak, closed.live, closed.failures,
        WORKSPACE.mapped_bytes(),
        bootstrap_before, WORKSPACE.bootstrap_stats(),
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
    // Prepare both own noninheritable duplicates before any arena seal. Even a
    // partial setup failure consumes every acquired owner through completion.
    let (mut status, mut result) = terminal::Native::prepare(-11i32 as u32, "status.prepare");
    let (errors, ready) = terminal::Native::prepare(-12i32 as u32, "error.prepare");
    trial_io::retain_first(&mut result, ready);
    let cancel = Cancellation::from_environment();
    let mut cleanup = Ok(());
    let mut private = trial_io::PrivateOutputs::default();
    if result.is_ok() {
        result = execute(&mut status, &cancel, &mut cleanup, &mut private);
    }
    terminal::complete(result, cleanup, &cancel, status, errors, private)
}

#[cfg(test)]
mod io_blocks {
    use super::*;

    #[test]
    fn source_block_tail_preserves_stereo_lane_order_and_float_bits() {
        let frames = 4099;
        let values: Vec<_> = (0..frames * 2)
            .map(|i| f32::from_bits(0x3f000000 + i as u32))
            .collect();
        let bytes: Vec<_> = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let cancel = Cancellation::from_environment();
        let mut lanes = vec![0.; values.len()];
        read_values(
            &mut bytes.as_slice(),
            values.len(),
            "test.read",
            &cancel,
            |index, value| lanes[(index % 2) * frames + index / 2] = value,
        )
        .unwrap();
        for i in 0..frames {
            for lane in 0..2 {
                assert_eq!(
                    lanes[lane * frames + i].to_bits(),
                    values[i * 2 + lane].to_bits()
                );
            }
        }
        let error = read_values(
            &mut &bytes[..bytes.len() - 1],
            values.len(),
            "test.short",
            &cancel,
            |_, _| {},
        )
        .unwrap_err();
        assert!(matches!(
            error,
            trial_io::Failure::Io {
                phase: "test.short",
                ..
            }
        ));
    }
}
