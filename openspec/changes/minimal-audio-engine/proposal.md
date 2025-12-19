# Change: Minimal AudioEngine with cpal

## Change ID
minimal-audio-engine

## Why
The Flitzis Looper requires real-time audio output capability. Currently, audio playback is handled externally. Integrating a native Rust audio engine with cpal will reduce dependency complexity and improve latency predictability.

## What Changes
- Add cpal as a dependency in rust/Cargo.toml
- Implement basic AudioEngine struct in rust/src/audio_engine.rs
- Expose minimal API: `new()`, `play()`, `stop()`
- Add PyO3 bindings to instantiate AudioEngine from Python
- No effects, no input, no MIDI

## Impact
- Affected specs: minimal-audio-engine
- Affected code: rust/src/audio_engine.rs, rust/Cargo.toml, rust/src/lib.rs
- Python API: New `AudioEngine` class available in `flitzis_looper_rs`

## Dependencies
- cpal v0.17 (stable)
- PyO3 for Python bindings

## Risks
- Platform-specific audio backend issues (Linux ALSA/PulseAudio)
- Real-time thread safety concerns
- FFI boundary complexity between Python and Rust

## Success Criteria
- Audio plays without distortion on Linux
- Latency < 20ms
- No crashes on shutdown
- AudioEngine can be instantiated from Python

## Reviewers
@audio-team

## Related Issues
None