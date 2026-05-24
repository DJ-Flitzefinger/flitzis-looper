## ADDED Requirements

### Requirement: Phase-Aware Single-Loop Transitions Remain Atomic
The system SHALL keep phase-aware single-loop transitions atomic when MultiLoop mode is
disabled and trigger quantization is enabled.

When a loaded pad is triggered, the system SHALL keep the stop-all operation and the
phase-aware pad start as one atomic scheduled transition at one absolute output frame.

If the scheduler cannot accept the transition, currently playing pads SHALL remain
unchanged. If the target pad cannot play, currently playing pads SHALL remain unchanged.

#### Scenario: Quantized phase-aware switch happens at one frame
- **WHEN** MultiLoop mode is disabled
- **AND** trigger quantization is enabled with grid step `1/16`
- **AND** pad 1 is active
- **AND** loaded pad 2 has valid BPM and phase-anchor metadata
- **WHEN** the performer triggers pad 2
- **THEN** Rust schedules one transition that stops pad 1 and starts pad 2 at the same output frame
- **AND** pad 2 starts at the phase-aware initial sample frame for that target frame

#### Scenario: Rejected phase-aware switch does not stop the active pad
- **WHEN** MultiLoop mode is disabled
- **AND** trigger quantization is enabled
- **AND** pad 1 is active
- **AND** the scheduler is full
- **WHEN** the performer triggers loaded pad 2
- **THEN** the transition is rejected
- **AND** pad 1 remains active
