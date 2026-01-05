## ADDED Requirements

### Requirement: Sidebar Load/Unload Actions For Selected Pad
The system SHALL provide audio slot actions for the currently selected pad in the left sidebar.

When the selected pad has no loaded audio, the sidebar SHALL provide a user action labeled "Load Audio" that opens the file selection dialog for that pad.

When the selected pad has loaded audio, the sidebar SHALL provide a user action labeled "Unload Audio" that unloads audio for that pad (see `load-audio-files`).

#### Scenario: Sidebar shows "Load Audio" for an empty selected pad
- **GIVEN** the selected pad has no loaded audio
- **WHEN** the left sidebar is rendered
- **THEN** the sidebar contains an action labeled "Load Audio"

#### Scenario: Sidebar "Load Audio" opens a file dialog
- **GIVEN** the selected pad has no loaded audio
- **WHEN** the performer activates "Load Audio" in the left sidebar
- **THEN** the system opens a file selection dialog filtered to at least: `wav`, `flac`, `mp3`, `aif/aiff`, `ogg`

#### Scenario: Sidebar shows "Unload Audio" for a loaded selected pad
- **GIVEN** the selected pad has loaded audio
- **WHEN** the left sidebar is rendered
- **THEN** the sidebar contains an action labeled "Unload Audio"

#### Scenario: Sidebar "Unload Audio" unloads the selected pad
- **GIVEN** the selected pad has loaded audio
- **WHEN** the performer activates "Unload Audio" in the left sidebar
- **THEN** the selected pad’s audio is unloaded (see `load-audio-files`)

## MODIFIED Requirements

### Requirement: Performance Pad Grid Layout
The system SHALL render a 6×6 pad grid (36 pads) in the primary content window.

#### Scenario: Grid renders on startup
- **WHEN** the UI is started
- **THEN** 36 pad controls are visible
- **AND** the pads are arranged as 6 columns by 6 rows

#### Scenario: Empty pads are labeled by number
- **WHEN** the UI is started
- **THEN** each pad with no loaded audio is labeled with its pad number from 1 through 36

#### Scenario: Loaded pads show the loaded filename
- **WHEN** audio is loaded into a pad’s sample slot (see `load-audio-files`)
- **THEN** that pad’s label shows the loaded audio file’s basename (filename only, no directory path)

#### Scenario: Unloading restores numeric labels
- **WHEN** audio is unloaded from a pad’s sample slot (see `load-audio-files`)
- **THEN** that pad’s label reverts to its pad number
