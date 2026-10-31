//! Complete-chunk arithmetic diagnostic, not an application execution receipt.
use flitzis_stem_dsp::{CHANNELS, Geometry, RATE, Spectrum};
use std::{
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
    process::ExitCode,
};

fn read(path: &Path, count: usize) -> Result<Vec<f32>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() != (count * 4) as u64 {
        return Err("incomplete diagnostic float32 payload".into());
    }
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(file);
    let mut raw = [0u8; 4];
    for _ in 0..count {
        reader.read_exact(&mut raw).map_err(|e| e.to_string())?;
        values.push(f32::from_le_bytes(raw));
    }
    if reader.read(&mut raw).map_err(|e| e.to_string())? != 0 {
        return Err("diagnostic payload grew during read".into());
    }
    Ok(values)
}
fn write(path: &Path, values: &[f32]) -> Result<(), String> {
    let file = File::create_new(path).map_err(|e| e.to_string())?;
    let mut writer = BufWriter::new(file);
    for value in values {
        writer
            .write_all(&value.to_le_bytes())
            .map_err(|e| e.to_string())?;
    }
    writer.flush().map_err(|e| e.to_string())?;
    writer.get_ref().sync_all().map_err(|e| e.to_string())
}
fn execute() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 5 {
        return Err(
            "usage: bs_spectral PCM_F32 MASK_F32 FRAMES NEW_FEATURES_F32 NEW_PCM_F32".into(),
        );
    }
    let frames = args[2]
        .to_str()
        .ok_or("invalid frames")?
        .parse()
        .map_err(|_| "invalid frames")?;
    let geometry = Geometry::new(frames, RATE).map_err(|e| e.to_string())?;
    for output in [&args[3], &args[4]] {
        if Path::new(output).try_exists().map_err(|e| e.to_string())? {
            return Err("diagnostic output already exists".into());
        }
    }
    let pcm = read(Path::new(&args[0]), CHANNELS * frames)?;
    let masks = read(Path::new(&args[1]), geometry.mask_len())?;
    let spectrum = Spectrum::analyze(&pcm, RATE, &|| false).map_err(|e| e.to_string())?;
    let output = spectrum
        .reconstruct(&masks, &|| false)
        .map_err(|e| e.to_string())?;
    write(Path::new(&args[3]), spectrum.features())?;
    write(Path::new(&args[4]), &output)?;
    println!(
        "BS spectral arithmetic only: frames={} steps={} payload_envelope_bytes={}",
        frames,
        geometry.steps(),
        geometry.transient_bytes()
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
