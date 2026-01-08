# async-sample-loading Specification

## Purpose
TBD - created by archiving change async-sample-loading. Update Purpose after archive.
## Requirements
### Requirement: Background Loading of Audio Samples
The system **SHALL** provide a non‑blocking API `load_sample_async(id: usize, path: str) -> None` that returns immediately after scheduling the sample load operation on a background worker thread.  
#### Scenario: Non‑blocking call returns immediately
- **WHEN** Python calls `audio_engine.load_sample_async(1, "/samples/test.flac")`
- **THEN** the call returns immediately without waiting for the load to finish

### Requirement: LoaderEvent Communication
The system **SHALL** define a `LoaderEvent` enum to communicate loading status from Rust to Python, including at least:
- `Started { id: usize }`
- `Success { id: usize, duration_s: f32 }`
- `Error { id: usize, error: String }`
- `Progress { id: usize, percent: f32, stage: String }`

For `Progress` events:
- `percent` **SHALL** represent best-effort total progress across the complete async load pipeline as a floating-point value in the range 0.0..=1.0.
- `stage` **SHALL** be a human-readable string using a main task with an optional sub-task, formatted as `"<Task>"` or `"<Task> (<Subtask>)"`.
  - Examples: `"Loading (decoding)"`, `"Loading (resampling)"`.
  - This format is intended to remain stable as new tasks are introduced (e.g. `"BPM detection"`).

#### Scenario: Event sent on load start
- **WHEN** a background thread begins loading a sample
- **THEN** it sends `LoaderEvent::Started { id }` to the UI queue

#### Scenario: Progress events include stage and total percentage
- **WHEN** a background thread is processing a sample load
- **THEN** it MAY send one or more `LoaderEvent::Progress { id, percent, stage }` events
- **AND** each event contains a human-readable `stage` string describing the current task/sub-task
- **AND** each event contains a `percent` value in 0.0..=1.0 representing total progress

#### Scenario: Progress increases monotonically and completes
- **WHEN** a sample load completes successfully
- **THEN** the system sends one or more progress updates with non-decreasing `percent`
- **AND** the final progress update (if emitted) is `percent == 1.0` before (or alongside) `LoaderEvent::Success`

### Requirement: Event Queue for UI Updates
`AudioEngine` **SHALL** expose a thread-safe method `poll_loader_events() -> Optional[Dict]` that retrieves pending `LoaderEvent` messages without blocking the Python UI thread.

The returned dictionary **SHALL** contain at least:
- `type`: one of `"started"`, `"progress"`, `"success"`, `"error"`
- `id`: the sample identifier

Additionally:
- For `type == "progress"`: `percent` (float 0.0..=1.0) and `stage` (string)
- For `type == "success"`: `duration_s`
- For `type == "error"`: `msg`

#### Scenario: UI polls and receives progress event
- **WHEN** Python calls `audio_engine.poll_loader_events()`
- **THEN** it may receive a dictionary with `type: "progress"`
- **AND** the dictionary contains `id`, `percent`, and `stage`

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

### Requirement: Sample Loading Includes Audio Analysis Stage
The async sample loading pipeline SHALL include a dedicated analysis stage that detects BPM, key, and beat grid for the sample being loaded.

The analysis stage SHALL run off the audio thread (on the background worker) and SHALL complete before the load operation is considered successful.

#### Scenario: Load progress reports an analyzing stage
- **WHEN** the worker begins analyzing a loaded sample
- **THEN** it sends one or more progress updates whose `stage` indicates analysis (e.g., `"Analyzing (bpm/key/beat grid)"`)

#### Scenario: Analysis happens after resampling
- **GIVEN** a sample load requires resampling
- **WHEN** the worker processes the load pipeline
- **THEN** the worker completes the resampling stage
- **AND** the worker begins the analysis stage after resampling

#### Scenario: Load success includes analysis results
- **WHEN** the worker finishes loading a sample successfully
- **THEN** the corresponding completion event includes the sample duration
- **AND** the completion event includes analysis results (BPM, key, beat grid) for that sample

### Requirement: Analysis Progress Uses Weighted Best-effort Percent
When the analysis stage cannot provide fine-grained progress updates, the system SHALL map analysis to a weighted portion of total progress and SHALL emit a progress update that jumps to completion once analysis finishes.

#### Scenario: Analysis progress completes in a single jump
- **WHEN** analysis starts and the system cannot compute fine-grained analysis progress
- **THEN** the system emits a progress update at the start of analysis
- **AND** the system emits a progress update with `percent == 1.0` once analysis finishes

