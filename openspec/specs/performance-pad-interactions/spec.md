# performance-pad-interactions Specification

## Purpose
To specify the mouse interaction model for the performance pad grid: left-click triggers/retriggers a pad, right-click stops it, and middle-click opens a per-pad context menu for actions like loading/unloading pad audio.
## Requirements
### Requirement: Trigger and Retrigger Pads
The system SHALL allow the performer to trigger or retrigger pads from the performance pad grid using left mouse button down.

When a pad has a configured loop region (see `loop-region`), triggering the pad SHALL start playback from the loop start.

When a pad has no configured loop region, triggering the pad SHALL start playback from the start of the sample.

#### Scenario: Left mouse down triggers a loaded pad
- **WHEN** a sample is loaded into the pad’s sample slot
- **AND** the performer presses the left mouse button down on the pad
- **THEN** the system triggers playback from the pad’s loop start when configured
- **AND** otherwise triggers playback from the start of the sample

#### Scenario: Left mouse down retriggers deterministically
- **WHEN** a sample is currently playing for a pad
- **AND** the performer presses the left mouse button down on the same pad
- **THEN** the system stops playback for that pad
- **AND** the system triggers playback again using the same start-point rules

### Requirement: Stop Pads Quickly
The system SHALL stop a pad using a right mouse button down on the pad.

#### Scenario: Right mouse down stops the pad
- **WHEN** a sample is currently playing for a pad
- **AND** the performer presses the right mouse button down on the pad
- **THEN** playback for that pad stops promptly

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

### Requirement: Rename Re-detect BPM To Analyze Audio
Any existing user-facing action labeled "Re-detect BPM" SHALL be renamed to "Analyze audio".

#### Scenario: UI uses the new label
- **WHEN** the UI is rendered for a loaded pad
- **THEN** the action label is "Analyze audio" and not "Re-detect BPM"

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

