//! Parameter-free decoder fixture for original HTDemucs forward comparisons.
//! The supervising caller owns the fixture files, process and last readers.

use flitzis_stem_dsp::demucs::{
    Error,
    host::Statistics,
    hybrid::{BranchStatistics, Geometry, Workspace},
    reduction::CpuProfile,
};
use std::{
    io::{self, Read, Write},
    process::ExitCode,
};

#[path = "support/demucs_fixture.rs"]
mod fixture;

const IO_BUFFER_BYTES: usize = 8192;

fn payload<T: Copy>(len: usize, value: T) -> Result<Vec<T>, Error> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| Error::Allocation {
            phase: "demucs.hybrid.trial",
            requested_bytes: len * size_of::<T>(),
        })?;
    values.resize(len, value);
    Ok(values)
}

fn read_float(reader: &mut impl Read) -> io::Result<f32> {
    let mut bytes = [0u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(f32::from_le_bytes(bytes))
}

fn write_floats(values: &[f32]) -> io::Result<()> {
    let mut writer = io::stdout().lock();
    let mut bytes = [0u8; IO_BUFFER_BYTES];
    for block in values.chunks(bytes.len() / 4) {
        for (value, raw) in block.iter().zip(bytes.chunks_exact_mut(4)) {
            raw.copy_from_slice(&value.to_le_bytes());
        }
        writer.write_all(&bytes[..block.len() * 4])?;
    }
    writer.flush()
}

fn execute() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    let independent = args.first().is_some_and(|arg| arg == "independent");
    if independent {
        args.remove(0);
    }
    if args.len() != 5 {
        return Err("usage: demucs_hybrid BATCH CHANNELS SOURCES FRAMES TRAINING < STATS_AND_PCM_F32 > PCM_F32".into());
    }
    let g = Geometry::new(
        args[0].parse()?,
        args[1].parse()?,
        args[2].parse()?,
        args[3].parse()?,
        args[4].parse()?,
        2 * IO_BUFFER_BYTES,
    )?;
    let zero = Statistics::from_reference(0.0, 0.0)?;
    let mut stats = payload(
        g.batches(),
        BranchStatistics {
            spectral: zero,
            time: zero,
        },
    )?;
    let mut reader = io::stdin().lock();
    if !independent {
        for stats in &mut stats {
            stats.spectral =
                Statistics::from_reference(read_float(&mut reader)?, read_float(&mut reader)?)?;
            stats.time =
                Statistics::from_reference(read_float(&mut reader)?, read_float(&mut reader)?)?;
        }
    }
    let mut pcm = payload(g.input_len(), 0.0)?;
    let mut bytes = [0u8; IO_BUFFER_BYTES];
    for block in pcm.chunks_mut(bytes.len() / 4) {
        reader.read_exact(&mut bytes[..block.len() * 4])?;
        for (value, raw) in block.iter_mut().zip(bytes.chunks_exact(4)) {
            *value = f32::from_le_bytes(raw.try_into().unwrap());
        }
    }
    if reader.read(&mut bytes[..1])? != 0 {
        return Err("unexpected trailing float32 input".into());
    }
    let mut output = payload(g.output_len(), 0.0)?;
    let mut work = Workspace::new(g, &|| false)?;
    if independent {
        let profile = CpuProfile::admit(
            "2.12.0+cu130",
            "7661cd9c6b841b62b7f411aa52ec51f05457263b",
            "AVX512",
            1,
        )
        .ok_or(Error::Geometry)?;
        work.run_independent(
            profile,
            &pcm,
            &mut output,
            &mut |g, spectral, time, ds, dt, cancel| {
                write_floats(spectral).map_err(|_| Error::Envelope)?;
                write_floats(time).map_err(|_| Error::Envelope)?;
                fixture::predict(g, spectral, time, ds, dt, cancel)
            },
            &|| false,
        )?;
        let (spectral, time) = work.independent_statistics();
        for (s, t) in spectral.iter().zip(time) {
            write_floats(&[s.mean(), s.std(), t.mean(), t.std()])?;
        }
    } else {
        work.run(&pcm, &stats, &mut output, &mut fixture::predict, &|| false)?;
    }
    let mut writer = io::stdout().lock();
    for block in output.chunks(bytes.len() / 4) {
        for (value, raw) in block.iter().zip(bytes.chunks_exact_mut(4)) {
            raw.copy_from_slice(&value.to_le_bytes());
        }
        writer.write_all(&bytes[..block.len() * 4])?;
    }
    writer.flush()?;
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
