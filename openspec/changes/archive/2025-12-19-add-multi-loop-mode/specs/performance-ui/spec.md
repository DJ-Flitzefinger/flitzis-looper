## ADDED Requirements

### Requirement: MultiLoop Toggle Control
The system SHALL provide a MultiLoop toggle control in the performance view, positioned below the bank selector controls, that enables or disables MultiLoop mode (see `multi-loop-mode`). The control SHALL visually indicate whether MultiLoop mode is enabled.

#### Scenario: MultiLoop control is visible
- **WHEN** the UI is started
- **THEN** a MultiLoop control is visible in the performance view

#### Scenario: MultiLoop control is positioned below bank selector
- **WHEN** the UI is started
- **THEN** the MultiLoop control is positioned below the bank selector controls

#### Scenario: MultiLoop control indicates enabled state
- **WHEN** MultiLoop mode is enabled
- **THEN** the MultiLoop control is visually indicated as enabled
- **WHEN** MultiLoop mode is disabled
- **THEN** the MultiLoop control is visually indicated as disabled

### Requirement: Active Pad Indication
The system SHALL visually indicate which pads are currently active (playing) in the pad grid.

#### Scenario: Triggering a pad marks it active
- **WHEN** a pad becomes active due to a trigger
- **THEN** the corresponding pad control is visually indicated as active

#### Scenario: Stopping a pad clears the active indicator
- **WHEN** a pad stops due to an explicit stop or unload
- **THEN** the corresponding pad control is visually indicated as inactive

## MODIFIED Requirements

### Requirement: Stable UI Identifiers
The system SHALL assign deterministic item tags to each pad and bank control to enable programmatic updates and event binding.

#### Scenario: Pad tags are stable and enumerable
- **WHEN** the UI is started
- **THEN** pads exist with tags `pad_btn_01` through `pad_btn_36`

#### Scenario: Bank tags are stable and enumerable
- **WHEN** the UI is started
- **THEN** bank controls exist with tags `bank_btn_1` through `bank_btn_6`

#### Scenario: MultiLoop tag is stable
- **WHEN** the UI is started
- **THEN** the MultiLoop control exists with tag `multiloop_btn`
