# multi-loop-mode Specification

## Purpose
To define the global MultiLoop mode that controls whether pads can loop concurrently (polyphonic looping) or enforce one-at-a-time playback, with immediate onset on trigger.

## Requirements
### Requirement: MultiLoop Mode State
The system SHALL provide a global MultiLoop mode setting that can be enabled or disabled at runtime.

#### Scenario: MultiLoop defaults to disabled
- **WHEN** the application is started
- **THEN** MultiLoop mode is disabled

#### Scenario: MultiLoop can be enabled and disabled
- **WHEN** the performer enables MultiLoop mode
- **THEN** MultiLoop mode becomes enabled
- **WHEN** the performer disables MultiLoop mode
- **THEN** MultiLoop mode becomes disabled

### Requirement: MultiLoop Enabled Allows Concurrent Loops
When MultiLoop mode is enabled, the system SHALL allow multiple pads to be active simultaneously. Triggering a pad SHALL start or restart that pad’s loop without stopping other active pads.

#### Scenario: Two pads play concurrently
- **WHEN** MultiLoop mode is enabled
- **AND** the performer triggers pad 1
- **AND** the performer triggers pad 2
- **THEN** pad 1 is active
- **AND** pad 2 is active
- **AND** both pads contribute to the mixed audio output (see `play-samples`)

#### Scenario: Retrigger affects only the selected pad
- **WHEN** MultiLoop mode is enabled
- **AND** pad 1 is active
- **AND** pad 2 is active
- **AND** the performer retriggers pad 1
- **THEN** pad 2 remains active

### Requirement: MultiLoop Disabled Enforces One-at-a-time
When MultiLoop mode is disabled, triggering a pad SHALL stop all other active pads before starting the triggered pad.

#### Scenario: Triggering a new pad stops the previously active pad
- **WHEN** MultiLoop mode is disabled
- **AND** pad 1 is active
- **AND** the performer triggers pad 2
- **THEN** pad 1 becomes inactive promptly
- **AND** pad 2 becomes active

### Requirement: Loop Onset Is Determined by Trigger Time
The system SHALL start loops immediately when a pad trigger action occurs. Loop onset SHALL be determined by the user’s trigger timing (no quantization to a global grid).

#### Scenario: Onsets are independent
- **WHEN** MultiLoop mode is enabled
- **AND** pad 1 is triggered at time T1
- **AND** pad 2 is triggered at time T2
- **THEN** pad 1 starts playback at time T1
- **AND** pad 2 starts playback at time T2

### Requirement: Triggering an Unloaded Pad Has No Effect
Triggering a pad with no loaded audio SHALL NOT stop other pads and SHALL NOT change active-pad state.

#### Scenario: Empty pad trigger is ignored
- **WHEN** MultiLoop mode is disabled
- **AND** pad 1 is active
- **AND** the performer triggers an unloaded pad
- **THEN** pad 1 remains active

