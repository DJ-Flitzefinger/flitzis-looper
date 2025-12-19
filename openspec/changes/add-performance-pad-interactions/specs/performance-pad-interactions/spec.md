## ADDED Requirements

### Requirement: Trigger and Retrigger Pads
The system SHALL allow the performer to trigger or retrigger pads from the performance pad grid using left-click.

#### Scenario: Left-click triggers a loaded pad
- **WHEN** a sample is loaded into the pad’s sample slot
- **AND** the performer left-clicks the pad
- **THEN** the system triggers playback from the start of the sample

#### Scenario: Left-click retriggers deterministically
- **WHEN** a sample is currently playing for a pad
- **AND** the performer left-clicks the same pad
- **THEN** the system stops playback for that pad
- **AND** the system triggers playback again from the start

### Requirement: Stop Pads Quickly
The system SHALL stop a pad using a right-click on the pad.

#### Scenario: Right-click stops the pad
- **WHEN** a sample is currently playing for a pad
- **AND** the performer right-clicks the pad
- **THEN** playback for that pad stops promptly

### Requirement: Pad Context Menu
The system SHALL open a per-pad context menu on middle-click.

#### Scenario: Middle-click opens a pad menu
- **WHEN** the performer middle-clicks a pad
- **THEN** a context menu for that pad is shown

#### Scenario: Context menu provides a stop action
- **WHEN** the pad context menu is open
- **AND** the performer selects the "Stop" action
- **THEN** playback for that pad stops
