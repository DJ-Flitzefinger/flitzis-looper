# Change: Add Sample Rate Resampling

## Why
Currently, the audio engine requires all loaded audio files to match the output device's sample rate exactly, returning an error if there's a mismatch. This limits users to only using audio files that match their system's sample rate, which is inconvenient and unexpected behavior for a modern audio application.

## What Changes
- Add automatic sample rate conversion (resampling) when loading audio files
- Use the `rubato` crate to perform high-quality resampling
- Remove the sample rate mismatch error, making the system more flexible
- Maintain real-time safety by performing resampling on the Python thread during loading

## Impact
- Affected specs: `load-audio-files`
- Affected code: `rust/src/audio_engine/sample_loader.rs`, `rust/src/audio_engine/errors.rs`
- New dependency: `rubato` crate
