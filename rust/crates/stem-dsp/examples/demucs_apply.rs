//! Supplied-offset/bag arithmetic control bridge, never a productive runtime.
use flitzis_stem_dsp::demucs::{
    Error, MAX_PAYLOAD_BYTES, RATE,
    host::apply::{Model, Workspace},
};
use std::{
    io::{self, Read, Write},
    process::ExitCode,
};

struct Config {
    segment: usize,
    valid: usize,
    training: usize,
    split: bool,
    overlap: f64,
    power: f32,
    offsets: Vec<usize>,
    weights: Vec<f64>,
}

fn config(text: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let parts: Vec<_> = text.split('/').collect();
    if parts.len() != 3 {
        return Err("model requires geometry/offsets/weights".into());
    }
    let p: Vec<_> = parts[0].split(',').collect();
    if p.len() != 6 {
        return Err("geometry requires segment,valid,training,split,overlap,power".into());
    }
    Ok(Config {
        segment: p[0].parse()?,
        valid: p[1].parse()?,
        training: p[2].parse()?,
        split: p[3].parse()?,
        overlap: p[4].parse()?,
        power: p[5].parse()?,
        offsets: if parts[1] == "-" {
            vec![]
        } else {
            parts[1]
                .split(',')
                .map(str::parse)
                .collect::<Result<_, _>>()?
        },
        weights: parts[2]
            .split(',')
            .map(str::parse)
            .collect::<Result<_, _>>()?,
    })
}

fn execute() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        return Err("demucs_apply FRAMES CHANNELS SOURCES geometry/offsets/weights ...".into());
    }
    let frames: usize = args[0].parse()?;
    let channels: usize = args[1].parse()?;
    let sources: usize = args[2].parse()?;
    let configs = args[3..]
        .iter()
        .map(|s| config(s))
        .collect::<Result<Vec<_>, _>>()?;
    let names: Vec<_> = (0..sources).map(|i| i.to_string()).collect();
    let names: Vec<_> = names.iter().map(String::as_str).collect();
    let models: Vec<_> = configs
        .iter()
        .map(|c| Model {
            sources: &names,
            channels,
            samplerate: RATE,
            segment: c.segment,
            overlap: c.overlap,
            transition_power: c.power,
            valid_frames: c.valid,
            training_frames: c.training,
            split: c.split,
            offsets: &c.offsets,
            weights: &c.weights,
        })
        .collect();
    let mut work = Workspace::new(frames, &models, &|| false)?;
    // The workspace guard includes the following caller PCM owners. Stream I/O
    // through a bounded stack buffer, with no whole-byte duplicate allocation.
    let len = frames.checked_mul(channels).ok_or(Error::Geometry)?;
    let outlen = len.checked_mul(sources).ok_or(Error::Geometry)?;
    let allocate = |n: usize| -> Result<Vec<f32>, Error> {
        let bytes = n.checked_mul(4).ok_or(Error::Geometry)?;
        if bytes > MAX_PAYLOAD_BYTES {
            return Err(Error::Geometry);
        }
        let mut v = Vec::new();
        v.try_reserve_exact(n).map_err(|_| Error::Allocation {
            phase: "demucs.apply.bridge",
            requested_bytes: bytes,
        })?;
        v.resize(n, 0.0);
        Ok(v)
    };
    let mut input = allocate(len)?;
    let mut output = allocate(outlen)?;
    let mut bytes = [0_u8; 8192];
    let mut reader = io::stdin().lock();
    for block in input.chunks_mut(bytes.len() / 4) {
        reader.read_exact(&mut bytes[..block.len() * 4])?;
        for (x, b) in block.iter_mut().zip(bytes.chunks_exact(4)) {
            *x = f32::from_le_bytes(b.try_into()?);
        }
    }
    if reader.read(&mut bytes[..1])? != 0 {
        return Err("trailing input".into());
    }
    drop(reader);
    work.run(
        &input,
        &mut output,
        &mut |index, chunk, input, output| {
            eprintln!(
                "CALL {index} {} {}",
                chunk.valid_frames, chunk.training_frames
            );
            let train = chunk.training_frames;
            for source in 0..sources {
                for channel in 0..channels {
                    let row = &input[channel * train..(channel + 1) * train];
                    for i in 0..train {
                        let base = (row[i] * ((source + 1) as f32 * 0.25)
                            + row[(i + train - 1) % train] * 0.125)
                            + (row[0] * 0.0625 + i as f32 * 0.000001);
                        output[(source * channels + channel) * train + i] =
                            base * (1.0 + index as f32 * 0.125) + index as f32 * 0.007;
                    }
                }
            }
            Ok(())
        },
        &|| false,
    )?;
    let mut writer = io::stdout().lock();
    for block in output.chunks(bytes.len() / 4) {
        for (x, b) in block.iter().zip(bytes.chunks_exact_mut(4)) {
            b.copy_from_slice(&x.to_le_bytes());
        }
        writer.write_all(&bytes[..block.len() * 4])?;
    }
    writer.flush()?;
    Ok(())
}

fn main() -> ExitCode {
    match execute() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
