## ADDED Requirements
### Requirement: Analyze Audio Action
For a pad with loaded audio, the system SHALL provide a user action labeled "Analyze audio" that triggers BPM, key, and beat grid detection for that pad.

This action SHALL be available from:
- The pad context menu.
- The left sidebar when the pad is selected.

#### Scenario: Pad context menu triggers analysis
- **GIVEN** a pad has loaded audio
- **AND** the pad is not currently loading
- **WHEN** the performer opens the pad context menu
- **THEN** the menu contains an action labeled "Analyze audio"
- **WHEN** the performer selects "Analyze audio"
- **THEN** the system triggers analysis for that pad

#### Scenario: Sidebar triggers analysis
- **GIVEN** a pad has loaded audio and is selected
- **AND** the pad is not currently loading
- **WHEN** the performer activates the "Analyze audio" action in the sidebar
- **THEN** the system triggers analysis for that pad

#### Scenario: Analyze audio is unavailable while loading
- **GIVEN** a pad is currently loading
- **WHEN** the performer opens the pad context menu or views the sidebar actions
- **THEN** the system does not allow triggering "Analyze audio" for that pad

### Requirement: Rename Re-detect BPM To Analyze Audio
Any existing user-facing action labeled "Re-detect BPM" SHALL be renamed to "Analyze audio".

#### Scenario: UI uses the new label
- **WHEN** the UI is rendered for a loaded pad
- **THEN** the action label is "Analyze audio" and not "Re-detect BPM"
