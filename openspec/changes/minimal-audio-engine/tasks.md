# Tasks: Minimal AudioEngine Implementation

## Phase 1: Setup
- [ ] Add cpal to rust/Cargo.toml
- [ ] Add PyO3 dependency to rust/Cargo.toml
- [ ] Create rust/src/audio_engine.rs
- [ ] Expose AudioEngine in rust/src/lib.rs with PyO3 bindings

## Phase 2: Core Implementation
- [ ] Implement `AudioEngine::new()` with default device
- [ ] Implement `AudioEngine::play()` with silent buffer
- [ ] Implement `AudioEngine::stop()` with graceful shutdown
- [ ] Add PyO3 `#[pyclass]` and `#[pymethods]` annotations

## Phase 3: Validation
- [ ] Add test in rust/tests/test_audio_engine.rs
- [ ] Verify build with `cargo check --manifest-path rust/Cargo.toml`
- [ ] Test audio output manually on Linux
- [ ] Test Python instantiation with `python -c "from flitzis_looper_rs import AudioEngine; engine = AudioEngine()"`

## Phase 4: Documentation
- [ ] Update architecture.md with AudioEngine diagram
- [ ] Add note in message-passing.md about audio buffer flow
- [ ] Document Python API in code comments

## Dependencies
- None

## Reviewers
@audio-team

## Estimated Effort
3 days