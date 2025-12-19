# Tasks: Minimal AudioEngine Implementation

## Phase 1: Setup
- [x] Add cpal to rust/Cargo.toml
- [x] Add PyO3 dependency to rust/Cargo.toml
- [x] Create rust/src/audio_engine.rs
- [x] Expose AudioEngine in rust/src/lib.rs with PyO3 bindings

## Phase 2: Core Implementation
- [x] Implement `AudioEngine::new()` with default device
- [x] Implement `AudioEngine::play()` with silent buffer
- [x] Implement `AudioEngine::stop()` with graceful shutdown
- [x] Add PyO3 `#[pyclass]` and `#[pymethods]` annotations

## Phase 3: Validation
- [x] Add test in rust/tests/test_audio_engine.rs
- [x] Verify build with `cargo check --manifest-path rust/Cargo.toml`
- [x] Test audio output manually on Linux
- [x] Test Python instantiation with `python -c "from flitzis_looper_rs import AudioEngine; engine = AudioEngine()"`

## Phase 4: Documentation
- [x] Update architecture.md with AudioEngine diagram
- [x] Add note in message-passing.md about audio buffer flow
- [x] Document Python API in code comments

## Dependencies
- None

## Reviewers
- @audio-team

## Estimated Effort
3 days