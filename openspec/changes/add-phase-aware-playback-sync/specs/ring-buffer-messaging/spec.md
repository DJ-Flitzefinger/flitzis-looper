## ADDED Requirements

### Requirement: Phase Sync Uses Fixed-Size Control Messages
The system SHALL use the existing fixed-capacity control-to-audio ring buffer for
phase-aware playback sync requests.

Any new request for BPM-lock transport phase anchoring SHALL be represented as a fixed-size
control message. The message SHALL contain bounded identifiers or scalar values only, such
as a selected pad id, and SHALL NOT contain file paths, heap-owned beat-grid vectors, or
Python objects.

#### Scenario: BPM-lock phase-anchor request is fixed-size
- **WHEN** Python/control code requests transport phase anchoring from the selected BPM-lock pad
- **THEN** the request is enqueued as a fixed-size control message
- **AND** the audio thread uses audio-thread-owned mixer and transport state to process it
- **AND** Python does not directly access transport, scheduler, or mixer storage

### Requirement: Phase Sync Message Failures Are Non-Blocking
Phase sync message failures SHALL be handled without blocking the audio callback.

If the control ring buffer is full before a request reaches the audio thread, the
Python-facing producer-side error/drop behavior SHALL apply. If phase anchoring cannot be
computed after a request reaches the audio thread, the request SHALL be ignored safely and
existing playback SHALL continue.

#### Scenario: Ring buffer full does not affect audio callback
- **GIVEN** the control-to-audio ring buffer is full
- **WHEN** Python/control code attempts to enqueue a phase-anchor request
- **THEN** the request is rejected or dropped according to the Python API contract
- **AND** the audio callback continues unaffected

#### Scenario: Phase anchor request cannot be computed
- **GIVEN** a phase-anchor request reaches the audio callback
- **AND** the selected pad is inactive or missing required metadata
- **WHEN** the callback handles the request
- **THEN** the callback ignores the request safely
- **AND** it does not block, allocate, log, touch disk, acquire the GIL, or panic
