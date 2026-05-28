## ADDED Requirements

### Requirement: BPM Lock Master Tempo Shares Transport Tempo Without Implicit Pad Phase Sync
The system SHALL use accepted BPM-lock performance master BPM updates as the shared Rust tempo for
both BPM-ratio matching and transport-grid timing.

When BPM lock is enabled, existing BPM-ratio tempo matching SHALL remain the active voice behavior.
When a valid performance master BPM is recomputed and accepted by the audio thread, Rust SHALL
apply that BPM to both mixer tempo matching and the permanent transport grid while preserving the
transport's current bar phase at the current output frame. This update SHALL NOT reset the
transport output-frame clock, stop, restart, retrigger, or time-slip active voices.

Enabling BPM lock, setting per-pad BPM, enabling Key Lock, or changing pitch/speed controls SHALL
NOT anchor the transport downbeat to a pad by side effect. An explicit transport phase-anchor
request MAY align the transport downbeat to a selected active pad when valid metadata is available.
That request SHALL remain a controlled sync operation, not a side effect of whichever pad happens
to be playing.

#### Scenario: BPM lock master tempo updates transport tempo without pad phase sync
- **GIVEN** BPM lock is enabled
- **AND** the permanent Rust transport has an existing master BPM and downbeat anchor
- **WHEN** the valid performance master BPM is accepted by the audio thread
- **THEN** existing active voices continue using BPM-ratio tempo matching
- **AND** the transport grid uses the same master BPM
- **AND** the transport preserves its current bar phase without anchoring to a pad

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
