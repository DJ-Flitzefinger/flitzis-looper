# Task Breakdown: Async Sample Loading Implementation

## 1. Define LoaderEvent Enum
- [ ] Create `LoaderEvent` enum in `openspec/messages.rs` (or new `events.rs`) with variants: Started, Progress, Success, Error.
- [ ] Ensure the enum is `#[derive(Debug, Clone, PartialEq)]`.
- [ ] Provide documentation comments for each variant.

## 2. Extend AudioEngine Struct
- [ ] Add `loader_tx: Sender<LoaderEvent>` and `loader_rx: Receiver<LoaderEvent>` fields to `AudioEngine` in `rust/src/lib.rs`.
- [ ] Update `AudioEngine::new()` to initialize the channel and store the receiver.
- [ ] Keep the `stream_handle` initialization unchanged.

## 3. Implement load_sample_async Method
- [ ] Add `pub fn load_sample_async(&self, id: usize, path: String) -> PyResult<()>` to `AudioEngine`.
- [ ] Clone necessary handles (`loader_tx`, `stream_handle`) for the worker thread.
- [ ] Spawn a new `std::thread` that executes the loading logic.
- [ ] Send `LoaderEvent::Started { id }` on thread start.

## 4. Worker Thread Logic
- [ ] Inside the spawned thread:
  - Perform disk read of the audio file.
  - Decode audio using `decode_audio_file_to_sample_buffer` (or equivalent).
  - Apply FFT resampling to the device rate.
  - Push the resulting sample to the audio ring buffer via `ControlMessage::LoadSample`.
  - On success, send `LoaderEvent::Success { id, duration_sec }`.
  - On any error, send `LoaderEvent::Error { id, error: e.to_string() }`.

## 5. Add poll_loader_events Method
- [ ] Implement `pub fn poll_loader_events(&self) -> Option<PyObject>` that:
  - Attempts to receive a `LoaderEvent` from `loader_rx` without blocking.
  - Converts the event into a Python dictionary (type, id, optional duration/error message).
  - Returns `None` if no event is available.

## 6. Update Python Bindings
- [ ] Expose `load_sample_async` and `poll_loader_events` as PyO3 methods in the Python wrapper.
- [ ] Ensure the methods follow the existing naming conventions and error handling patterns.

## 7. Update UI Layer (Python)
- [ ] Modify the ImGui UI code to call `poll_loader_events()` each frame.
- [ ] Handle the different event types to update UI elements (e.g., enable Play button, show spinner, display error messages).

## 8. Documentation Updates
- [ ] Add documentation for the new async loading API in `docs/audio-engine.md` and `docs/ui-toolkit.md`.
- [ ] Update any relevant README or developer guides to mention the background loading mechanism.

## 9. Testing Strategy
- [ ] Write unit tests for `AudioEngine::load_sample_async` to verify that it spawns a thread and returns immediately.
- [ ] Add integration tests that simulate loading events and assert that `poll_loader_events` returns the expected dictionaries.
- [ ] Ensure existing tests still pass after changes.

## 10. Verification Checklist
- [ ] Code compiles with `cargo check --manifest-path rust/Cargo.toml`.
- [ ] Linting (`cargo clippy`) passes.
- [ ] Formatting (`cargo fmt --check`) passes.
- [ ] Python unit tests related to sample loading continue to pass.
- [ ] Manual UI test: loading a sample now shows progress updates without freezing the UI.

## Dependencies
- `std::sync::mpsc` for channel creation.
- `std::thread` for spawning worker threads.
- `serde` (if needed for event serialization) – add to `pyproject.toml` if used.

## Risks & Mitigations
- **Thread Safety**: Ensure all data moved into the worker thread implements `Send`. Use `Arc` for shared references.
- **Error Propagation**: Capture and forward errors via `LoaderEvent::Error` to avoid silent failures.
- **Ring Buffer Overflow**: Handle cases where the audio ring buffer is full; fallback to error event.