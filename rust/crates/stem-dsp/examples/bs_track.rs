//! Non-neural wholetrack arithmetic diagnostic, not an application receipt.
use flitzis_stem_dsp::{
    Error, RATE,
    track::{CHUNK, TrackPlan, process_track},
};
use std::{
    fs::File,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
    process::ExitCode,
};

fn execute() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: bs_track INTERLEAVED_PCM_F32 FRAMES NEW_PRIVATE_OUTPUT_F32".into());
    }
    let frames = args[1]
        .to_str()
        .ok_or("invalid frames")?
        .parse()
        .map_err(|_| "invalid frames")?;
    let plan = TrackPlan::new(frames, RATE).map_err(|e| e.to_string())?;
    let mut source = BufReader::new(File::open(&args[0]).map_err(|e| e.to_string())?);
    if source
        .get_ref()
        .metadata()
        .map_err(|e| e.to_string())?
        .len()
        != (frames * 8) as u64
    {
        return Err("incomplete diagnostic stereo source".into());
    }
    // Exclusive private diagnostic sink may contain partial bytes on failure;
    // only exit0/full length is success. Caller owns its cleanup, never the core.
    let file = File::create_new(Path::new(&args[2])).map_err(|e| e.to_string())?;
    let mut output = BufWriter::new(file);
    let mut ordinal = 0;
    let mut written = 0;
    process_track(
        plan,
        |start, count, pcm| {
            source
                .seek(SeekFrom::Start((start * 8) as u64))
                .map_err(|_| Error::Source)?;
            for i in 0..count {
                for channel in 0..2 {
                    let mut raw = [0; 4];
                    source.read_exact(&mut raw).map_err(|_| Error::Source)?;
                    pcm[channel * count + i] = f32::from_le_bytes(raw);
                }
            }
            Ok(count)
        },
        |input, batch| {
            let mut prediction = Vec::new();
            prediction
                .try_reserve_exact(batch * 8 * CHUNK)
                .map_err(|_| Error::Allocation)?;
            for b in 0..batch {
                for stem in 0..4 {
                    for channel in 0..2 {
                        for i in 0..CHUNK {
                            let gain = [0.25, -0.5, 0.75, 1.25][stem];
                            let code = (i * 3 + (ordinal + b) * 7 + stem * 11 + channel * 13) % 101;
                            prediction.push(
                                input[(b * 2 + channel) * CHUNK + i] * gain
                                    + (code as f32 - 50.0) * 0.0001,
                            );
                        }
                    }
                }
            }
            ordinal += batch;
            Ok(prediction)
        },
        |start, count, pcm| {
            if start != written {
                return Err(Error::Output);
            }
            // Frame-major [physical frame, stem, stereo], for bounded disk comparison.
            for i in 0..count {
                for lane in 0..8 {
                    output
                        .write_all(&pcm[lane * count + i].to_le_bytes())
                        .map_err(|_| Error::Output)?;
                }
            }
            written += count;
            Ok(())
        },
        &|| false,
    )
    .map_err(|e| e.to_string())?;
    if written != frames {
        return Err("incomplete diagnostic output".into());
    }
    output.flush().map_err(|e| e.to_string())?;
    output.get_ref().sync_all().map_err(|e| e.to_string())?;
    println!(
        "BS track arithmetic only: frames={} chunks={} pcm_envelope_bytes={}",
        frames,
        ordinal,
        plan.pcm_transient_bytes()
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
