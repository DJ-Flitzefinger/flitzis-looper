# Tasks: Audio File Loading and Sample Playback

## 1. Protocol & Data Model
- [x] 1.1 Define a minimal immutable sample buffer type (e.g., `Arc<[f32]>` + channel count)
- [x] 1.2 Update `ControlMessage::LoadSample` to publish a loaded sample handle + `id`
- [x] 1.3 Add/confirm `ControlMessage::PlaySample { id, velocity }` semantics (id range 0..32, velocity range, missing id behavior)
- [x] 1.4 Define fixed v1 limits: `MAX_SAMPLE_SLOTS = 32` and `MAX_VOICES = 32` (later derived from UI)

## 2. Non-Real-Time File Loading
- [x] 2.1 Add audio decoding via `symphonia` to produce normalized interleaved `f32` samples
- [x] 2.2 Implement `AudioEngine.load_sample(id, path)` (runs on Python thread; returns Python exception on failure)
- [x] 2.3 Implement basic channel mapping rules (mono↔stereo) and clear errors for unsupported layouts
- [x] 2.4 Decide and implement v1 sample-rate handling (match output rate or error); add explicit error messages

## 3. Real-Time Playback
- [x] 3.1 Add a fixed-size sample bank (32 slots) in the audio callback
- [x] 3.2 Implement a fixed-capacity voice list (max 32 voices) and deterministic overflow behavior (drop or replace)
- [x] 3.3 Render voices into the output buffer (interleaved channels) with per-trigger velocity scaling
- [x] 3.4 Remove/avoid logging and other non-RT-safe operations inside the audio callback

## 4. Python API Surface
- [x] 4.1 Expose `play_sample(id, velocity)` to Python and wire it to the control message queue
- [x] 4.2 Update `src/flitzis_looper_rs/__init__.pyi` with the new public methods

## 5. Tests
- [x] 5.1 Add Rust unit tests for decoding (generate a small WAV file in the test and load it via `symphonia`)
- [x] 5.2 Add Rust unit tests for mixing that do not require an audio device (factor mixing into a pure helper and test it)
- [x] 5.3 Add Python smoke test that calls `load_sample` and `play_sample` without raising (skip if no audio device is available)

## 6. Documentation & Validation
- [x] 6.1 Update `docs/architecture.md` and `docs/message-passing.md` to reflect the implemented loading/playback flow
- [x] 6.2 Run Rust checks: `cargo fmt --manifest-path rust/Cargo.toml` and `cargo test --manifest-path rust/Cargo.toml`
- [x] 6.3 Run Python tests: `uv run pytest`