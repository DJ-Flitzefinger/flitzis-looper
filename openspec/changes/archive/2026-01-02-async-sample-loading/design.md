# Design Document: Async Sample Loading Architecture

## 1. Overview
The goal is to eliminate UI freezes caused by synchronous sample loading by offloading disk I/O, decoding, and resampling to a background worker thread. The worker thread will communicate progress and completion via a message channel back to the Python layer, which can then update the UI.

## 2. Component Diagram
- **Python UI Thread**: Calls `load_sample_async`, polls `poll_loader_events()` each frame.
- **AudioEngine (PyO3 Wrapper)**: Holds the `Sender<LoaderEvent>` and manages the receiver end of the channel.
- **Worker Thread**: Executes loading steps, pushes decoded sample to the audio ring buffer, sends events.
- **Audio Ring Buffer**: Receives the loaded sample for playback.
- **LoaderEvent Enum**: Encodes Started, Progress, Success, Error states.

## 3. Communication Flow
1. Python invokes `AudioEngine.load_sample_async(id, path)`.
2. AudioEngine spawns a worker thread (or task) with cloned `Sender<LoaderEvent>`.
3. Worker thread:
   - Sends `LoaderEvent::Started { id }`.
   - Performs file read → decode → resample.
   - Pushes resulting sample to the audio ring buffer via `AudioStreamHandle`.
   - Sends `LoaderEvent::Success { id, duration_sec }` or `LoaderEvent::Error { id, error }`.
4. Python periodically calls `AudioEngine.poll_loader_events()` to retrieve pending events and update UI widgets.

## 4. Thread Safety Considerations
- All data passed to the worker thread must implement `Send`.
- The `AudioStreamHandle` used for pushing samples must be thread‑safe (e.g., wrapped in `Arc<Mutex<_>>` or similar).
- Events are sent via `mpsc::channel`, which is itself thread‑safe.

## 5. Error Handling Strategy
- Errors occurring at any stage (file not found, decode failure, ring buffer overflow) are captured and sent as `LoaderEvent::Error`.
- The error message is attached to the event and delivered to Python for UI display.

## 6. Progress Reporting
- For long‑running loads, intermediate progress percentages can be emitted via `LoaderEvent::Progress`.
- Initial implementation may omit granular progress and only emit `Started` and final `Success/Error`, but the enum is designed to support future progress updates.

## 7. Integration Points
- **Existing Audio Engine**: Must expose a means to push the decoded sample into the ring buffer; this is already present via `ControlMessage::LoadSample`.
- **UI Layer**: Must call `poll_loader_events()` each frame and react accordingly (e.g., enable Play button, show loading spinner).
- **Python API**: Adds `load_sample_async` method and `poll_loader_events` method returning a dictionary describing the event.

## 8. Future Extensions
- **Granular Progress**: Attach percentage completed to each progress event.
- **Thread Pool**: Replace `std::thread::spawn` with a configurable thread pool for better resource utilization.
- **BPM Analysis**: After loading, perform BPM detection and emit additional events.