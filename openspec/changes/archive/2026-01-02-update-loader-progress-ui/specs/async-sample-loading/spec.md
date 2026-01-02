## MODIFIED Requirements
### Requirement: LoaderEvent Communication
The system **SHALL** define a `LoaderEvent` enum to communicate loading status from Rust to Python, including at least:
- `Started { id: usize }`
- `Success { id: usize, duration_sec: f32 }`
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
- For `type == "success"`: `duration_sec`
- For `type == "error"`: `msg`

#### Scenario: UI polls and receives progress event
- **WHEN** Python calls `audio_engine.poll_loader_events()`
- **THEN** it may receive a dictionary with `type: "progress"`
- **AND** the dictionary contains `id`, `percent`, and `stage`

#### Scenario: UI polls and receives success event
- **WHEN** Python calls `audio_engine.poll_loader_events()`
- **THEN** it receives a dictionary with `type: "success"` and can update the UI accordingly