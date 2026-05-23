## ADDED Requirements

### Requirement: BPM Lock Establishes Shared Phase For Future Quantized Starts
The system SHALL allow BPM lock to establish shared transport phase for future quantized
starts when a valid active anchor pad is available.

When BPM lock is enabled and the selected anchor pad is active with valid BPM and phase
metadata, Rust SHALL align the transport downbeat to the anchor pad's current musical
phase.

The initial phase-aware BPM-lock behavior SHALL NOT continuously slip, warp, or retrigger
already playing non-anchor voices. Existing BPM-ratio tempo matching SHALL remain the active
voice behavior. The shared phase anchor SHALL be used by later quantized starts and
restarts.

#### Scenario: BPM-lock anchor provides transport phase for the next pad
- **GIVEN** BPM lock is enabled from an active anchor pad
- **AND** Rust successfully anchors the transport downbeat to that pad's phase
- **AND** trigger quantization is enabled
- **WHEN** another loaded pad is triggered for the next bar
- **THEN** Rust schedules the start at the transport bar boundary derived from the anchor pad's phase
- **AND** the new pad starts at a phase-aware initial sample frame when its metadata is valid

#### Scenario: Existing active pads are not time-slipped by phase anchoring
- **GIVEN** one or more pads are active
- **WHEN** Rust anchors transport phase from the BPM-lock source pad
- **THEN** existing active voices continue with their current playback positions and existing BPM-ratio tempo matching
- **AND** no corrective seek, time-slip, or forced retrigger is applied to non-anchor voices

### Requirement: BPM Lock Phase Degrades Gracefully
If BPM lock phase anchoring cannot be established, the system SHALL preserve existing BPM
lock behavior by tempo-matching pads using BPM metadata where available and falling back to
global speed where metadata is missing.

#### Scenario: Missing anchor metadata preserves tempo matching
- **GIVEN** BPM lock is enabled
- **AND** the selected anchor pad lacks valid BPM or phase-anchor metadata
- **WHEN** Rust handles a phase-anchor request
- **THEN** Rust does not update the transport downbeat from that pad
- **AND** BPM lock continues to use existing tempo-ratio matching
