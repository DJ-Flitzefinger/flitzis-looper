# Task Breakdown: Async Sample Loading Implementation

## 1. Define LoaderEvent Enum
- [x] Create `LoaderEvent` enum in `openspec/messages.rs` (or new `events.rs`) with variants: Started, Progress, Success, Error.
- [x] Ensure the enum is `#[derive(Debug, Clone, PartialEq)]`.
- [x] Provide documentation comments for each variant.

## 2. Extend AudioEngine Struct
- [x] Add `loader_tx: Sender<LoaderEvent>` and `loader_rx: Receiver<LoaderEvent>` fields to `AudioEngine` in `rust/src/lib.rs`.
- [x] Update `AudioEngine::new()` to initialize the channel and store the receiver.
- [x] Keep the `stream_handle` initialization unchanged.

## 3. Implement load_sample_async Method
- [x] Add `pub fn load_sample_async(&self, id: usize, path: String) -> PyResult<()>` to `AudioEngine`.
- [x] Clone necessary handles (`loader_tx`, `stream_handle`) for the worker thread.
- [x] Spawn a new `std::thread` that executes the loading logic.
- [x] Send `LoaderEvent::Started { id }` on thread start.

## 4. Worker Thread Logic
- [x] Inside the spawned thread:
  - Perform disk read of the audio file.
  - Decode audio using `decode_audio_file_to_sample_buffer` (or equivalent).
  - Apply FFT resampling to the device rate.
  - Push the resulting sample to the audio ring buffer via `ControlMessage::LoadSample`.
  - On success, send `LoaderEvent::Success { id, duration_sec }`.
  - On any error, send `LoaderEvent::Error { id, error: e.to_string() }`.

## 5. Add poll_loader_events Method
- [x] Implement `pub fn poll_loader_events(&self) -> Option<PyObject>` that:
  - Attempts to receive a `LoaderEvent` from `loader_rx` without blocking.
  - Converts the event into a Python dictionary (type, id, optional duration/error message).
  - Returns `None` if no event is available.

## 6. Update Python Bindings
- [x] Expose `load_sample_async` and `poll_loader_events` as PyO3 methods in the Python wrapper.
- [x] Ensure the methods follow the existing naming conventions and error handling patterns.

## 7. Update UI Layer (Python)
- [x] Modify the ImGui UI code to call `poll_loader_events()` each frame.
- [x] Handle the different event types to update UI elements (e.g., enable Play button, show spinner, display error messages).

## 8. Documentation Updates
- [x] Add documentation for the new async loading API in `docs/audio-engine.md` and `docs/ui-toolkit.md`.
- [x] Update any relevant README or developer guides to mention the background loading mechanism.

## 9. Testing Strategy
- [x] Write unit tests for `AudioEngine::load_sample_async` to verify that it spawns a thread and returns immediately.
- [x] Add integration tests that simulate loading events and assert that `poll_loader_events` returns the expected dictionaries.
- [x] Ensure existing tests still pass after changes.

## 10. Verification Checklist
- [x] Code compiles with `cargo check --manifest-path rust/Cargo.toml`.
- [x] Linting (`cargo clippy`) passes.
- [x] Formatting (`cargo fmt --check`) passes.
- [x] Python unit tests related to sample loading continue to pass.
- [x] Manual UI test: loading a sample now shows progress updates without freezing the UI.

## Dependencies
- `std::sync::mpsc` for channel creation.
- `std::thread` for spawning worker threads.
- `serde` (if needed for event serialization) – add to `pyproject.toml` if used.

## Risks & Mitigations
- **Thread Safety**: Ensure all data moved into the worker thread implements `Send`. Use `Arc` for shared references.
- **Error Propagation**: Capture and forward errors via `LoaderEvent::Error` to avoid silent failures.
- **Ring Buffer Overflow**: Handle cases where the audio ring buffer is full; fallback to error event.