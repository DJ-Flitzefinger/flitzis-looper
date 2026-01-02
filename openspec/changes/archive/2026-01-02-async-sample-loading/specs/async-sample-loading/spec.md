## ADDED Requirements

### Requirement: Background Loading of Audio Samples
The system **SHALL** provide a non‑blocking API `load_sample_async(id: usize, path: str) -> None` that returns immediately after scheduling the sample load operation on a background worker thread.  
#### Scenario: Non‑blocking call returns immediately
- **WHEN** Python calls `audio_engine.load_sample_async(1, "/samples/test.flac")`
- **THEN** the call returns immediately without waiting for the load to finish

### Requirement: LoaderEvent Communication
The system **SHALL** define a `LoaderEvent` enum to communicate loading status from Rust to Python, including at least:
- `Started { id: usize }`
- `Success { id: usize, duration_sec: f32 }`
- `Error { id: usize, error: String }`
- `Progress { id: usize, percent: f32 }` (future‑proof)  
#### Scenario: Event sent on load start
- **WHEN** a background thread begins loading a sample
- **THEN** it sends `LoaderEvent::Started { id }` to the UI queue

### Requirement: Event Queue for UI Updates
`AudioEngine` **SHALL** expose a thread‑safe method `poll_loader_events() -> Optional[Dict]` that retrieves pending `LoaderEvent` messages without blocking the Python UI thread. The returned dictionary must contain at least:
- `type`: one of "started", "success", "error"
- `id`: the sample identifier
- `duration_sec` (on success)
- `msg` (on error)  
#### Scenario: UI polls and receives success event
- **WHEN** Python calls `audio_engine.poll_loader_events()`
- **THEN** it receives a dictionary with `type: "success"` and can update the UI accordingly

### Requirement: Integration with Audio Ring Buffer
The system **SHALL** integrate with the audio ring buffer by pushing the decoded and resampled sample into the buffer via a `ControlMessage::LoadSample` command upon successful load.  
#### Scenario: Sample pushed to ring buffer
- **WHEN** decoding finishes successfully
- **THEN** the sample is pushed to the audio ring buffer using `ControlMessage::LoadSample`

### Requirement: Thread Safety
The system **SHALL** ensure all data transferred to the worker thread is `Send + 'static`. The `AudioStreamHandle` used to push samples into the ring buffer must be safely shareable across threads (e.g., wrapped in `Arc<Mutex<_>>`). The channel for `LoaderEvent` communication must be thread‑safe.  
#### Scenario: Safe transfer of handle to worker thread
- **WHEN** spawning the worker thread
- **THEN** all necessary data (path, id, sender) are moved into the thread and are `Send`

### Requirement: Error Propagation
The system **SHALL** capture any failure during disk I/O, decoding, resampling, or ring buffer insertion and report it back to Python as a `LoaderEvent::Error` containing a descriptive error message.  
#### Scenario: Error during file read
- **WHEN** the sample file cannot be found
- **THEN** the worker thread sends `LoaderEvent::Error { id, error: "File not found" }`

### Requirement: Non‑blocking UI Polling
The system **SHALL** allow the Python UI layer to call `poll_loader_events()` each frame (e.g., within the ImGui render loop) to update UI widgets such as spinners, progress bars, and Play buttons based on loading status.  
#### Scenario: UI updates based on poll
- **WHEN** the UI loop calls `poll_loader_events()` each frame
- **THEN** it can enable/disable controls based on received events