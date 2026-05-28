## ADDED Requirements

### Requirement: Audio Telemetry Dispatch Is Controller-Owned
The system SHALL route audio-to-control telemetry through controller-owned dispatch before it
mutates Python session projections.

The UI layer MAY request runtime polling during rendering, but the controller SHALL own the
message-type dispatch for `SampleStarted`, `SampleStopped`, `PadPeak`, and `PadPlayhead` telemetry.

#### Scenario: Audio message updates session through controller dispatch
- **GIVEN** the audio-to-control queue contains pad peak, playhead, started, or stopped telemetry
- **WHEN** runtime events are polled
- **THEN** the controller dispatches each recognized message to the appropriate controller handler
- **AND** the resulting `SessionState` changes happen through controller-owned code

### Requirement: Python Session Playback State Is A Projection Of Rust Audio State
The system SHALL treat Rust audio-thread state as the live authority for active voices, source
playheads, pause/render state, transport, scheduler, loaded buffers, prepared stems, and future
smoothed DSP parameter state.

Python `SessionState` playback fields SHALL be transient projections updated by controller-owned
telemetry handling and explicit controller actions such as unload, pause, and resume. Audio
telemetry remains best-effort; a dropped telemetry message MUST NOT make `ProjectState` durable
intent change silently.

#### Scenario: Dropped telemetry does not persist false live state
- **GIVEN** audio-to-control telemetry is delayed or dropped
- **WHEN** the project is saved or restored
- **THEN** `ProjectState` still persists only durable performer intent
- **AND** live playback indicators remain a transient `SessionState` projection
