# Change: Add Sample Rate Resampling

## Why
Currently, the audio engine requires all loaded audio files to match the output device's sample rate exactly, returning an error if there's a mismatch. This limits users to only using audio files that match their system's sample rate, which is inconvenient and unexpected behavior for a modern audio application.

## What Changes
- Add automatic sample rate conversion (resampling) when loading audio files
- Use the `rubato` crate to perform high-quality resampling
- Maintain real-time safety by performing resampling on the Python thread during loading

## Requirements
The system SHALL automatically resample audio files to match the output device's sample rate during loading.

### Scenario: Load succeeds with different sample rate
- **WHEN** `AudioEngine.load_sample(id, path)` is called with an audio file that has a different sample rate than the output device
- **THEN** the file is automatically resampled to match the output device's sample rate
- **AND** the sample is successfully loaded and can be played back

The system SHALL NOT return an error when loading audio files with sample rates that differ from the output device's sample rate.

## Requirements
The system SHALL automatically resample audio files to match the output device's sample rate during loading.

### Scenario: Load succeeds with different sample rate
- **WHEN** `AudioEngine.load_sample(id, path)` is called with an audio file that has a different sample rate than the output device
- **THEN** the file is automatically resampled to match the output device's sample rate
- **AND** the sample is successfully loaded and can be played back

The system SHALL NOT return an error when loading audio files with sample rates that differ from the output device's sample rate.

## Impact
- Affected specs: `load-audio-files`
- Affected code: `rust/src/audio_engine/sample_loader.rs`, `rust/src/audio_engine/errors.rs`
- New dependency: `rubato` crate
