//! Raw f32 conversion bridge; regular file/process/readers belong to its caller.
//! Output contains the complete phase-major kernel followed by planar PCM.
use flitzis_stem_dsp::demucs::{
    Error,
    audio::{Channels, ConversionGeometry, Converter, Options},
};
use std::{
    io::{self, Read, Write},
    process::ExitCode,
};

fn payload(len: usize) -> Result<Vec<f32>, Error> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| Error::Allocation {
            phase: "demucs.audio.trial",
            requested_bytes: len * 4,
        })?;
    values.resize(len, 0.0);
    Ok(values)
}

fn execute() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 10 {
        return Err("usage: demucs_audio FRAMES BATCHES SOURCE_CHANNELS TARGET_CHANNELS OLD_RATE NEW_RATE ZEROS ROLLOFF FULL OUTPUT_LENGTH|none".into());
    }
    let channels = Channels::new(
        args[0].parse()?,
        args[1].parse()?,
        args[2].parse()?,
        args[3].parse()?,
    )?;
    let options = Options {
        zeros: args[6].parse()?,
        rolloff: args[7].parse()?,
        full: args[8].parse()?,
        output_length: if args[9] == "none" {
            None
        } else {
            Some(args[9].parse()?)
        },
    };
    let geometry = ConversionGeometry::new(channels, args[4].parse()?, args[5].parse()?, options)?;
    let mut work = Converter::new(geometry, &|| false)?;
    let mut input = payload(geometry.input_len())?;
    let mut output = payload(geometry.output_len())?;
    let mut reader = io::stdin().lock();
    let mut bytes = [0u8; 8192];
    for block in input.chunks_mut(bytes.len() / 4) {
        reader.read_exact(&mut bytes[..block.len() * 4])?;
        for (value, raw) in block.iter_mut().zip(bytes.chunks_exact(4)) {
            *value = f32::from_le_bytes(raw.try_into().unwrap());
        }
    }
    if reader.read(&mut bytes[..1])? != 0 {
        return Err("unexpected trailing f32 input".into());
    }
    work.convert_into(&input, &mut output, &|| false)?;
    let mut writer = io::stdout().lock();
    for values in [work.kernel(), &output] {
        for block in values.chunks(bytes.len() / 4) {
            for (value, raw) in block.iter().zip(bytes.chunks_exact_mut(4)) {
                raw.copy_from_slice(&value.to_le_bytes());
            }
            writer.write_all(&bytes[..block.len() * 4])?;
        }
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
