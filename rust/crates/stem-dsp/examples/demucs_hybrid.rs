//! Parameter-free decoder fixture for original HTDemucs forward comparisons.
//! The supervising caller owns the fixture files, process and last readers.

use flitzis_stem_dsp::demucs::{
    BINS, Error,
    host::Statistics,
    hybrid::{BranchStatistics, Geometry, Workspace},
};
use std::{
    io::{self, Read, Write},
    process::ExitCode,
};

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

fn predict(
    g: Geometry,
    spectral: &[f32],
    time: &[f32],
    decoded_spectral: &mut [f32],
    decoded_time: &mut [f32],
    cancelled: &dyn Fn() -> bool,
) -> Result<(), Error> {
    let steps = g.spectral_steps();
    let spectrum_len = 2 * BINS * steps;
    let frames = g.training_frames();
    for batch in 0..g.batches() {
        for source in 0..g.sources() {
            for channel in 0..g.channels() {
                let input_lane = batch * g.channels() + channel;
                let output_lane = (batch * g.sources() + source) * g.channels() + channel;
                let gain = (source + 1) as f32;
                let label = (1 + batch * 16 + source * 3 + channel) as f32;
                for part in 0..2 {
                    for bin in 0..BINS {
                        let offset = (part * BINS + bin) * steps;
                        let mark = match (part, bin) {
                            (0, 3) => 8.0,
                            (1, 7) => -6.0,
                            (0, bin) if bin == BINS - 1 => 3.0,
                            _ => 0.0,
                        };
                        if cancelled() {
                            return Err(Error::Cancelled);
                        }
                        for step in 0..steps {
                            let i = offset + step;
                            let patterned = (step % 5) as f32 * 0.125 + label * 0.0625;
                            decoded_spectral[output_lane * spectrum_len + i] =
                                spectral[input_lane * spectrum_len + i] * (gain * 0.25)
                                    + mark * patterned;
                        }
                    }
                }
                for i in 0..frames {
                    if i.is_multiple_of(4096) && cancelled() {
                        return Err(Error::Cancelled);
                    }
                    decoded_time[output_lane * frames + i] = time[input_lane * frames + i]
                        * (gain * -0.125)
                        + ((i % 17) as f32 * 0.03125 + label * 0.0625);
                }
            }
        }
    }
    Ok(())
}

fn read_float(reader: &mut impl Read) -> io::Result<f32> {
    let mut bytes = [0u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(f32::from_le_bytes(bytes))
}

fn execute() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 5 {
        return Err("usage: demucs_hybrid BATCH CHANNELS SOURCES FRAMES TRAINING < STATS_AND_PCM_F32 > PCM_F32".into());
    }
    let g = Geometry::new(
        args[0].parse()?,
        args[1].parse()?,
        args[2].parse()?,
        args[3].parse()?,
        args[4].parse()?,
        IO_BUFFER_BYTES,
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
    for stats in &mut stats {
        stats.spectral =
            Statistics::from_reference(read_float(&mut reader)?, read_float(&mut reader)?)?;
        stats.time =
            Statistics::from_reference(read_float(&mut reader)?, read_float(&mut reader)?)?;
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
    work.run(&pcm, &stats, &mut output, &mut predict, &|| false)?;
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
