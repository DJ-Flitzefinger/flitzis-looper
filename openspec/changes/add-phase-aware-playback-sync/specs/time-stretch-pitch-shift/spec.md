## ADDED Requirements

### Requirement: BPM Lock Does Not Implicitly Redefine The Permanent Transport
The system SHALL keep BPM-lock tempo matching separate from the permanent Rust transport
masterclock unless an explicit transport sync operation is requested.

When BPM lock is enabled, existing BPM-ratio tempo matching SHALL remain the active voice
behavior. Enabling BPM lock, changing global speed, setting per-pad BPM, enabling key lock, or
changing pitch/speed controls SHALL NOT automatically move the transport downbeat anchor, reset
the transport output-frame clock, or redefine the transport master BPM used for trigger
quantization.

An explicit transport phase-anchor request MAY align the transport downbeat to a selected active
pad when valid metadata is available. That request SHALL be a controlled sync operation, not a
side effect of whichever pad happens to be playing.

#### Scenario: BPM lock preserves transport phase without explicit sync
- **GIVEN** BPM lock is enabled
- **AND** the permanent Rust transport has an existing master BPM and downbeat anchor
- **WHEN** the mixer master BPM is recomputed for tempo matching
- **THEN** existing active voices continue using BPM-ratio tempo matching
- **AND** the transport master BPM and downbeat anchor remain unchanged

#### Scenario: Explicit sync may anchor transport phase
- **GIVEN** BPM lock is enabled
- **AND** an explicit transport phase-anchor request selects an active pad with valid BPM and timing metadata
- **WHEN** Rust handles the fixed-size phase-anchor request
- **THEN** Rust may update the transport downbeat anchor from that pad's current musical phase
- **AND** existing active voices are not time-slipped, warped, or retriggered

### Requirement: BPM Lock Phase Degrades Gracefully
The system SHALL preserve existing BPM-lock tempo matching when explicit transport phase anchoring
cannot be established.

If the selected anchor pad is inactive, paused, missing, or lacks valid BPM/timing metadata, Rust
SHALL leave the transport downbeat unchanged and continue existing tempo-ratio matching behavior
where metadata is available.

#### Scenario: Missing anchor metadata preserves tempo matching
- **GIVEN** BPM lock is enabled
- **AND** the selected anchor pad lacks valid BPM or timing-anchor metadata
- **WHEN** Rust handles a phase-anchor request
- **THEN** Rust does not update the transport downbeat from that pad
- **AND** BPM lock continues to use existing tempo-ratio matching
