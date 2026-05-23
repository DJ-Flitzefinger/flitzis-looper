## ADDED Requirements

### Requirement: Stem Mix Control Messages Are Fixed Size
The system SHALL update audio-thread stem mix state using fixed-size bounded control
messages through the existing control-to-audio ring buffer.

Stem mix control messages SHALL contain bounded scalar fields such as pad id, source-version
hash or token, mix mode, enabled stem mask, mute mask, and solo mask. They SHALL NOT contain
file paths, Python objects, unbounded metadata vectors, or copied stem audio payloads.

#### Scenario: All-stems mode sends bounded control state
- **GIVEN** the performer selects all-stems mode for a pad with current prepared stems
- **WHEN** the controller publishes the update to Rust
- **THEN** the control-to-audio message contains only bounded stem mix metadata
- **AND** the audio thread does not receive file paths or full stem audio payloads

#### Scenario: Future per-stem mask update is bounded
- **GIVEN** a future per-stem mute or solo control changes for a pad
- **WHEN** the control layer publishes the update to Rust
- **THEN** the message represents the state as bounded masks over known stem kinds
- **AND** the update does not allocate, block, log, touch disk, run inference, or acquire the Python GIL in the audio callback

### Requirement: Stem Mix Message Failure Preserves Playback
The system SHALL preserve current playback when a stem mix control message cannot be
enqueued or is rejected by the audio thread.

Producer-side ring-buffer-full failure SHALL be reported or deferred outside the audio
callback. Audio-thread stale-source or unavailable-stem rejection SHALL leave existing
full-mix or stem playback state unchanged.

#### Scenario: Ring buffer full leaves current mix unchanged
- **GIVEN** a stem mix update is ready
- **AND** the control-to-audio ring buffer is full
- **WHEN** the producer tries to enqueue the update
- **THEN** the request is rejected or deferred outside the audio callback
- **AND** current playback continues with the previous mix state

#### Scenario: Stale stem mix update is rejected safely
- **GIVEN** a stem mix update targets source version A
- **AND** the audio thread currently has source version B loaded for that pad
- **WHEN** the update reaches the audio callback
- **THEN** the callback rejects the stale update
- **AND** current playback continues with the previous mix state
