## ADDED Requirements

### Requirement: Launch Inputs Share One Engine Timestamp Epoch
The system SHALL capture launch input timestamps as integer nanoseconds in one Rust engine-owned
monotonic epoch established before stream startup and shared by native MIDI and UI capture.

Native MIDI SHALL capture at backend callback entry. Keyboard and mouse launch gestures SHALL
capture at their earliest accepted observable UI condition, before launch preparation. One global
START/STOP restart gesture SHALL retain one timestamp for every pad in its batch. Timestamps SHALL
remain transient and SHALL NOT be persisted as project intent.

#### Scenario: Native and UI capture share an epoch
- **WHEN** native MIDI and a UI launch input are captured in a running engine
- **THEN** their timestamps use the same Rust monotonic epoch
- **AND** Python does not substitute its own monotonic clock value

#### Scenario: Global restart uses one input observation
- **WHEN** one accepted START/STOP gesture restarts multiple pads
- **THEN** all resulting play requests retain that gesture's single captured timestamp

### Requirement: Launch Timestamp Survives Dispatch And Fallback
The system SHALL preserve a valid captured launch timestamp through normal and exclusive pad
dispatch, mapped keyboard actions, mouse launches, waveform restarts and Python MIDI fallback.

Delayed processing SHALL NOT recapture input time. A successful directly dispatched MIDI event
SHALL remain deduplicated in Python. A failed direct dispatch SHALL retain the original timestamp
in its fallback event. Zero SHALL be a valid timestamp. Malformed event timestamps SHALL use the
documented unstamped fallback and expose an input error; negative, overflowing, boolean or
non-integer public API timestamp arguments SHALL be rejected. Missing or semantically future
timestamps SHALL use the unstamped compatibility path without changing legacy launch execution.

#### Scenario: Delayed direct failure falls back with original time
- **GIVEN** a native MIDI launch could not enter the command queue
- **WHEN** Python processes its fallback event after a delay
- **THEN** the play request retains the original captured timestamp
- **AND** Python does not capture a new time for that event

#### Scenario: Successful direct event is not replayed
- **WHEN** Python observes a directly dispatched MIDI event marked successful
- **THEN** it does not issue a second playback request

#### Scenario: Zero and invalid timestamps are distinguished
- **WHEN** the API receives a timestamp of zero
- **THEN** it preserves zero as a valid engine-epoch timestamp
- **WHEN** the API receives a boolean or negative timestamp
- **THEN** it reports an argument error before publication

#### Scenario: Legacy caller remains compatible
- **WHEN** an existing caller sends a two-argument play request with no timestamp
- **THEN** the request retains existing playback and queue-error behavior
