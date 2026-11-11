//! Nonneural host-arithmetic bridge. Supervisor owns regular files, complete
//! process settlement and last readers. No productive execution receipt.
use flitzis_stem_dsp::demucs::{
    Error, MAX_PAYLOAD_BYTES,
    host::{Chunk, Statistics, Track, TrackWorkspace},
};
use std::{
    io::{self, Read, Write},
    process::ExitCode,
};

fn payload(len: usize) -> Result<Vec<f32>, Error> {
    let bytes = len.checked_mul(4).ok_or(Error::Geometry)?;
    if bytes > MAX_PAYLOAD_BYTES {
        return Err(Error::Geometry);
    }
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| Error::Allocation {
            phase: "demucs-host-trial",
            requested_bytes: bytes,
        })?;
    values.resize(len, 0.0);
    Ok(values)
}

fn read(len: usize) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let mut values = payload(len)?;
    let mut reader = io::stdin().lock();
    let mut bytes = [0_u8; 8192];
    for block in values.chunks_mut(bytes.len() / 4) {
        reader.read_exact(&mut bytes[..block.len() * 4])?;
        for (value, raw) in block.iter_mut().zip(bytes.chunks_exact(4)) {
            *value = f32::from_le_bytes(raw.try_into()?);
        }
    }
    if reader.read(&mut bytes[..1])? != 0 {
        return Err("trailing float32 input".into());
    }
    Ok(values)
}

fn write(values: &[f32]) -> io::Result<()> {
    let mut writer = io::stdout().lock();
    let mut bytes = [0_u8; 8192];
    for block in values.chunks(bytes.len() / 4) {
        for (value, raw) in block.iter().zip(bytes.chunks_exact_mut(4)) {
            raw.copy_from_slice(&value.to_le_bytes());
        }
        writer.write_all(&bytes[..block.len() * 4])?;
    }
    writer.flush()
}

fn execute() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = std::env::args().skip(1).collect();
    if a.len() < 3 {
        return Err("demucs_host branch|cli|prepare|track FRAMES CHANNELS [geometry]".into());
    }
    let frames: usize = a[1].parse()?;
    let channels: usize = a[2].parse()?;
    let len = frames.checked_mul(channels).ok_or(Error::Geometry)?;
    match a[0].as_str() {
        "branch" | "cli" | "branch-reference" | "cli-reference"
            if (matches!(a[0].as_str(), "branch" | "cli") && a.len() == 3)
                || (matches!(a[0].as_str(), "branch-reference" | "cli-reference")
                    && a.len() == 5) =>
        {
            // Bound the simultaneous bridge input, normalized and restored copies.
            if len.checked_mul(12).is_none_or(|v| v > MAX_PAYLOAD_BYTES) {
                return Err(Error::Geometry.into());
            }
            let input = read(len)?;
            let stats = if a.len() == 5 {
                Statistics::from_reference(a[3].parse()?, a[4].parse()?)?
            } else if a[0] == "cli" {
                Statistics::channel_reference_independent(&input, channels, &|| false)?
            } else {
                Statistics::measure_independent(&input, &|| false)?
            };
            let mut out = payload(len)?;
            stats.normalize_into(
                &input,
                &mut out,
                if a[0].starts_with("cli") { 0.0 } else { 1e-5 },
                &|| false,
            )?;
            write(&[stats.mean(), stats.std()])?;
            write(&out)?;
            stats.restore_in_place(&mut out, &|| false)?;
            write(&out)?;
        }
        "prepare" if a.len() == 7 => {
            let chunk = Chunk {
                offset: a[3].parse()?,
                frames: a[4].parse()?,
                valid_frames: a[5].parse()?,
                training_frames: a[6].parse()?,
            };
            let out_len = channels
                .checked_mul(chunk.training_frames)
                .ok_or(Error::Geometry)?;
            if len
                .checked_add(out_len)
                .and_then(|v| v.checked_mul(4))
                .is_none_or(|v| v > MAX_PAYLOAD_BYTES)
            {
                return Err(Error::Geometry.into());
            }
            let input = read(len)?;
            let mut out = payload(out_len)?;
            chunk.prepare_into(&input, channels, &mut out, &|| false)?;
            write(&out)?;
        }
        "track" if a.len() == 9 => {
            let sources: usize = a[3].parse()?;
            let t = Track::new(
                frames,
                channels,
                sources,
                a[4].parse()?,
                a[5].parse()?,
                a[6].parse()?,
                a[7].parse()?,
                a[8].parse()?,
            )?;
            let input = read(t.input_len())?;
            let mut out = payload(t.output_len())?;
            let mut work = TrackWorkspace::new(t, &|| false)?;
            work.run(
                &input,
                &mut out,
                &mut |chunk, input, output| {
                    let train = chunk.training_frames;
                    for source in 0..sources {
                        let gain = (source + 1) as f32 * 0.25;
                        for channel in 0..channels {
                            let row = &input[channel * train..(channel + 1) * train];
                            for i in 0..train {
                                output[(source * channels + channel) * train + i] = (row[i] * gain
                                    + row[(i + train - 1) % train] * 0.125)
                                    + (row[0] * 0.0625 + i as f32 * 0.000001);
                            }
                        }
                    }
                    Ok(())
                },
                &|| false,
            )?;
            write(&out)?;
        }
        _ => return Err("invalid host arithmetic mode/arguments".into()),
    }
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
