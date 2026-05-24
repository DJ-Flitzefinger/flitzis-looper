## MODIFIED Requirements

### Requirement: Loop Onset Is Determined by Trigger Time
The system SHALL start loops immediately when trigger quantization is disabled and a pad trigger
action occurs. Loop onset SHALL be determined by the user's trigger timing, matching the existing
non-quantized behavior.

When trigger quantization is enabled, the user's trigger timing SHALL create a trigger
request, and Rust SHALL align the actual loop onset to the selected transport grid boundary
using the Rust-owned timeline.

#### Scenario: Non-quantized onsets remain independent
- **WHEN** MultiLoop mode is enabled
- **AND** trigger quantization is disabled
- **AND** pad 1 is triggered at time T1
- **AND** pad 2 is triggered at time T2
- **THEN** pad 1 starts playback at time T1
- **AND** pad 2 starts playback at time T2

#### Scenario: Quantized onsets align to the selected grid
- **WHEN** MultiLoop mode is enabled
- **AND** trigger quantization is enabled with grid step `1 Bar`
- **AND** pad 1 and pad 2 are triggered before the same next bar boundary
- **THEN** Rust schedules both pad starts at that next bar boundary
- **AND** both starts are targeted by absolute output-frame positions

## ADDED Requirements

### Requirement: Quantized Single-Loop Transitions Are Atomic
The system SHALL keep quantized single-loop transitions atomic when MultiLoop mode is
disabled and trigger quantization is enabled.

Triggering a loaded pad SHALL schedule the stop-other-pads operation and the requested pad
start as one atomic transition at the same absolute output frame.

If the scheduler cannot accept that transition, the system SHALL leave currently playing
pads unchanged.

#### Scenario: Quantized one-at-a-time switch happens at one frame
- **WHEN** MultiLoop mode is disabled
- **AND** trigger quantization is enabled with grid step `1 Bar`
- **AND** pad 1 is active
- **AND** the performer triggers loaded pad 2
- **THEN** Rust schedules pad 1 to stop and pad 2 to start at the same next-bar output frame

#### Scenario: Rejected quantized switch does not stop the active pad
- **WHEN** MultiLoop mode is disabled
- **AND** trigger quantization is enabled
- **AND** pad 1 is active
- **AND** the scheduler is full
- **AND** the performer triggers loaded pad 2
- **THEN** the transition is rejected
- **AND** pad 1 remains active
