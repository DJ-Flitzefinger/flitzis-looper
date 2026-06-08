## ADDED Requirements

### Requirement: Quantized Single-Loop Transitions Remain Atomic And Loop-Start Based
The system SHALL keep quantized single-loop transitions atomic when MultiLoop mode is
disabled and trigger quantization is enabled.

When a loaded pad is triggered, the system SHALL keep the stop-all operation and the
pad start as one atomic scheduled transition at one absolute output frame.

The target pad SHALL start from its effective loop start at that transition frame. Rust SHALL
NOT use phase-aware source-frame offsets or late-click catch-up to start the target pad from the
middle or end of its loop.

If the scheduler cannot accept the transition, currently playing pads SHALL remain
unchanged. If the target pad cannot play, currently playing pads SHALL remain unchanged.

#### Scenario: Quantized switch happens at one frame and loop start
- **WHEN** MultiLoop mode is disabled
- **AND** trigger quantization is enabled with grid step `1/16`
- **AND** pad 1 is active
- **AND** loaded pad 2 has valid BPM and phase-anchor metadata
- **WHEN** the performer triggers pad 2
- **THEN** Rust schedules one transition that stops pad 1 and starts pad 2 at the same output frame
- **AND** pad 2 starts at its effective loop-start source frame

#### Scenario: Rejected quantized switch does not stop the active pad
- **WHEN** MultiLoop mode is disabled
- **AND** trigger quantization is enabled
- **AND** pad 1 is active
- **AND** the scheduler is full
- **WHEN** the performer triggers loaded pad 2
- **THEN** the transition is rejected
- **AND** pad 1 remains active
