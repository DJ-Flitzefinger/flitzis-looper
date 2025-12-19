# Change: Audio File Loading and Sample Playback

## Change ID
`add-audio-file-playback`

## Status
Proposed

## Summary
Implement a minimal, real-time-safe path to load short audio files (decoded via `symphonia`) on a non-real-time thread and trigger playback from the audio callback using ID-based commands.

## Why
We currently have a running CPAL output stream and ring-buffer messaging, but the audio callback only outputs silence. To support real sample triggering, we need file I/O and decoding to happen outside the audio thread while still making sample data available to the real-time callback without locks or large data copies.

## What Changes
1. Add a non-real-time Python API `AudioEngine.load_sample(id, path)` that reads/decodes an audio file and prepares an immutable sample buffer.
2. Update the control message protocol so the audio thread can receive a lightweight handle to the prepared sample buffer (shared memory), keyed by `id`.
3. Add a small, fixed-capacity voice mixer in the audio callback that can trigger playback via `AudioEngine.play_sample(id, velocity)`.
4. Use fixed v1 limits for predictable behavior: 32 sample slots and 32 voices (later tied to UI sample banks).
5. Ensure real-time safety: no disk I/O, no blocking, and no allocations in the audio callback.
6. Update architecture docs to reflect the file-loading and playback data flow.

## Affected Components
- Rust audio engine (`rust/src/audio_engine.rs`): sample bank, voice playback, message handling
- Message protocol (`rust/src/messages.rs`): sample loading and triggering messages
- Python API surface (`src/flitzis_looper_rs/__init__.pyi`) and tests
- OpenSpec: new capabilities `load-audio-files` and `play-samples`

## Dependencies
- `symphonia` crate for decoding audio files

## Validation Plan
- Rust unit tests for `symphonia` decode → f32 buffer conversion (use a WAV fixture for determinism)
- Rust unit tests for deterministic rendering/mixing (without requiring real audio hardware)
- Python smoke tests for the new public methods (load + trigger)