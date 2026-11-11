//! Raw float32 arithmetic bridge. The supervisor owns stdin/stdout files, their
//! private publication, complete process lifetime and final readers. No model or
//! productive execution receipt; std/native startup/stdio/exit remain external.
use flitzis_stem_dsp::demucs::{Error, Geometry, RATE, Workspace};
use std::{
    io::{self, Read, Write},
    process::ExitCode,
};

fn payload(len: usize) -> Result<Vec<f32>, Error> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| Error::Allocation {
            phase: "demucs-trial.payload",
            requested_bytes: len * 4,
        })?;
    values.resize(len, 0.0);
    Ok(values)
}

fn execute() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 || !matches!(args[0].as_str(), "analyze" | "reconstruct") {
        return Err(
            "usage: demucs_spectral analyze|reconstruct FRAMES LANES < INPUT_F32 > OUTPUT_F32"
                .into(),
        );
    }
    let g = Geometry::new(args[1].parse()?, args[2].parse()?, RATE)?;
    let analyze = args[0] == "analyze";
    let (input_len, output_len) = if analyze {
        (g.pcm_len(), g.feature_len())
    } else {
        (g.feature_len(), g.pcm_len())
    };
    let mut input = payload(input_len)?;
    let mut output = payload(output_len)?;
    let mut reader = io::stdin().lock();
    let mut bytes = [0u8; 8192];
    for block in input.chunks_mut(bytes.len() / 4) {
        reader.read_exact(&mut bytes[..block.len() * 4])?;
        for (value, raw) in block.iter_mut().zip(bytes.chunks_exact(4)) {
            *value = f32::from_le_bytes(raw.try_into().unwrap());
        }
    }
    if reader.read(&mut bytes[..1])? != 0 {
        return Err("unexpected trailing float32 input".into());
    }
    let mut workspace = Workspace::new(&|| false)?;
    if analyze {
        workspace.analyze_into(g, &input, &mut output, &|| false)?;
    } else {
        workspace.reconstruct_into(g, &input, &mut output, &|| false)?;
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
