## ADDED Requirements

### Requirement: BPM Display Shows Effective Master/Global BPM
The system SHALL display an effective BPM value in the performance view that reflects the current tempo state.

#### Scenario: BPM display reflects locked master BPM
- **GIVEN** BPM lock is enabled and the system has selected a master BPM
- **WHEN** the performance view is rendered
- **THEN** the BPM display shows the current master BPM value

#### Scenario: BPM display reflects active pad BPM scaled by speed when unlocked
- **GIVEN** BPM lock is disabled
- **AND** a pad is currently active and has an effective BPM value
- **WHEN** the performer changes global speed
- **THEN** the BPM display updates to approximately `active_pad_bpm * speed`

### Requirement: BPM Lock And Key Lock Controls Affect Playback State
The system SHALL provide BPM lock and Key lock controls whose visual state reflects the current mode and whose activation changes the corresponding mode.

#### Scenario: Lock buttons reflect current state
- **GIVEN** Key lock is disabled
- **WHEN** the performance view is rendered
- **THEN** the Key lock control is visually indicated as disabled
- **WHEN** the performer enables Key lock
- **THEN** the Key lock control is visually indicated as enabled

### Requirement: BPM Lock Anchors Master BPM To The Current Pad When Enabled
When the performer enables BPM lock, the system SHALL select the currently selected pad as the lock source and derive the master BPM from that pad when available.

#### Scenario: Enabling BPM lock captures master BPM from selected pad and speed
- **GIVEN** Pad 1 is selected
- **AND** Pad 1 has an effective BPM value
- **AND** the current global speed is 1.25×
- **WHEN** the performer enables BPM lock
- **THEN** the system sets the master BPM to approximately `Pad1_bpm * 1.25`
