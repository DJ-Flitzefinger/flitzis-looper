## ADDED Requirements

### Requirement: Middle-click Selects Pad
The system SHALL select a pad in the performance pad grid when the performer clicks the middle mouse button on that pad.

Middle-click selection SHALL NOT trigger playback and SHALL NOT stop playback.

#### Scenario: Middle-click selects the pad
- **WHEN** the performer middle-clicks a pad in the performance pad grid
- **THEN** that pad becomes the selected pad
- **AND** the left sidebar reflects the newly selected pad

#### Scenario: Middle-click on selected pad is a no-op
- **GIVEN** a pad is already selected
- **WHEN** the performer middle-clicks the selected pad
- **THEN** the selected pad remains unchanged

#### Scenario: Middle-click does not affect playback
- **GIVEN** a pad is currently playing
- **WHEN** the performer middle-clicks that pad
- **THEN** the system does not stop playback due to the middle-click
- **AND** the system does not trigger playback due to the middle-click

## MODIFIED Requirements

### Requirement: Analyze Audio Action
For a pad with loaded audio, the system SHALL provide a user action labeled "Analyze audio" that triggers BPM, key, and beat grid detection for that pad.

This action SHALL be available from:
- The left sidebar when the pad is selected.

#### Scenario: Sidebar triggers analysis
- **GIVEN** a pad has loaded audio and is selected
- **AND** the pad is not currently loading
- **WHEN** the performer activates the "Analyze audio" action in the sidebar
- **THEN** the system triggers analysis for that pad

#### Scenario: Analyze audio is unavailable while loading
- **GIVEN** a pad is currently loading
- **WHEN** the performer views the sidebar actions for that pad
- **THEN** the system does not allow triggering "Analyze audio" for that pad

## REMOVED Requirements

### Requirement: Pad Context Menu
**Reason**: The pad context menu duplicates actions that are already available via left sidebar buttons.

**Migration**: Select the pad (middle-click or existing selection behavior) and use the left sidebar actions (Load/Unload/Analyze/etc.).

**Removal scope**: Remove the middle-click popup behavior and all associated context-menu UI code and identifiers.
