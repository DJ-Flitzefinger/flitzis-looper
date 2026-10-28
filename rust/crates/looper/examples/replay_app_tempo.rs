//! Device-free numeric replay of already acquired detector observations.
//! This does not decode audio, run a model or grant source/timing authority.

#[path = "../src/audio_engine/app_tempo.rs"]
pub(crate) mod app_tempo;

// Reuse the exact existing public-estimator tests through their normal module
// path, without compiling or constructing the audio engine in this thin target.
#[cfg(test)]
mod audio_engine {
    pub(crate) use crate::app_tempo;
}

use serde_json::{Value, json};
use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("Expected an observation JSON file")?;
    let data: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let cases = data["cases"].as_array().ok_or("Expected cases array")?;
    let mut output = std::io::stdout().lock();
    for case in cases {
        // Integer bit patterns preserve cached binary64 observations exactly;
        // this path must not add another decimal parsing approximation.
        let beats: Vec<f64> = case["beat_bits"]
            .as_array()
            .ok_or("Expected detector observations")?
            .iter()
            .map(|value| {
                value
                    .as_u64()
                    .map(f64::from_bits)
                    .ok_or("Expected binary64 observation bits")
            })
            .collect::<Result<_, _>>()?;
        let duration = f64::from_bits(
            case["duration_bits"]
                .as_u64()
                .ok_or("Expected binary64 source duration bits")?,
        );
        let started = std::time::Instant::now();
        let estimate = app_tempo::estimate(&beats, duration);
        let result = json!({
            "id":case["id"],
            "estimate":estimate,
            "elapsed_seconds":started.elapsed().as_secs_f64(),
        });
        serde_json::to_writer(&mut output, &result)?;
        writeln!(&mut output)?;
        output.flush()?;
    }
    Ok(())
}
