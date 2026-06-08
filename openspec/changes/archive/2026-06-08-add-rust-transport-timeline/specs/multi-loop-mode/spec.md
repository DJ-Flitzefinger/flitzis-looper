## MODIFIED Requirements

### Requirement: Loop Onset Is Determined by Trigger Time
The system SHALL start loops immediately when trigger quantization is disabled and a pad trigger
action occurs. Loop onset SHALL be determined by the user's trigger timing, matching the existing
non-quantized behavior.

When trigger quantization is enabled, the user's trigger timing SHALL create a trigger
request, and Rust SHALL align the actual loop onset to the selected transport grid boundary
using the permanent Rust-owned timeline.

Quantized MultiLoop playback SHALL preserve manual musical offsets between pads. A later trigger
SHALL attach to the shared global transport at its own selected-grid output frame, not be
phase-forced to the first beat, first bar, oldest active pad, or first-started pad.

#### Scenario: Non-quantized onsets remain independent
- **WHEN** MultiLoop mode is enabled
- **AND** trigger quantization is disabled
- **AND** pad 1 is triggered at time T1
- **AND** pad 2 is triggered at time T2
- **THEN** pad 1 starts playback at time T1
- **AND** pad 2 starts playback at time T2

#### Scenario: Quantized onsets align to the selected grid
- **WHEN** MultiLoop mode is enabled
- **AND** trigger quantization is enabled with grid step `1/16`
- **AND** pad 1 and pad 2 are triggered before different future `1/16` boundaries
- **THEN** Rust targets each pad start at its own selected `1/16` boundary
- **AND** both starts are targeted by absolute output-frame positions

#### Scenario: Two-bar offset is preserved
- **WHEN** MultiLoop mode is enabled
- **AND** trigger quantization is enabled with grid step `1/16`
- **AND** pad 1 is triggered on the global grid
- **AND** pad 2 is intentionally triggered two bars later on the global grid
- **THEN** pad 2 starts two bars after pad 1 in output time
- **AND** Rust does not force pad 2 to share pad 1's first-beat or first-bar phase

#### Scenario: Stopping first-started pad preserves future quantization
- **WHEN** MultiLoop mode is enabled
- **AND** trigger quantization is enabled
- **AND** pads 1, 2, 3, 4, and 5 are active on the global transport
- **AND** the performer stops pad 1 while pads 2 through 5 keep playing
- **THEN** the permanent Rust transport phase remains unchanged
- **AND** a later pad trigger quantizes against the same global transport grid

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
- **AND** trigger quantization is enabled with grid step `1/16`
- **AND** pad 1 is active
- **AND** the performer triggers loaded pad 2
- **THEN** Rust schedules pad 1 to stop and pad 2 to start at the same selected-grid output frame

#### Scenario: Rejected quantized switch does not stop the active pad
- **WHEN** MultiLoop mode is disabled
- **AND** trigger quantization is enabled
- **AND** pad 1 is active
- **AND** the scheduler is full
- **AND** the performer triggers loaded pad 2
- **THEN** the transition is rejected
- **AND** pad 1 remains active
