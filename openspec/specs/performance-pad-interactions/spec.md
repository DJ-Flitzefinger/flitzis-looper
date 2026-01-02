# performance-pad-interactions Specification

## Purpose
To specify the mouse interaction model for the performance pad grid: left-click triggers/retriggers a pad, right-click stops it, and middle-click opens a per-pad context menu for actions like loading/unloading pad audio.
## Requirements
### Requirement: Trigger and Retrigger Pads
The system SHALL allow the performer to trigger or retrigger pads from the performance pad grid using left mouse button down.

#### Scenario: Left mouse down triggers a loaded pad
- **WHEN** a sample is loaded into the pad’s sample slot
- **AND** the performer presses the left mouse button down on the pad
- **THEN** the system triggers playback from the start of the sample

#### Scenario: Left mouse down retriggers deterministically
- **WHEN** a sample is currently playing for a pad
- **AND** the performer presses the left mouse button down on the same pad
- **THEN** the system stops playback for that pad
- **AND** the system triggers playback again from the start

### Requirement: Stop Pads Quickly
The system SHALL stop a pad using a right mouse button down on the pad.

#### Scenario: Right mouse down stops the pad
- **WHEN** a sample is currently playing for a pad
- **AND** the performer presses the right mouse button down on the pad
- **THEN** playback for that pad stops promptly

### Requirement: Pad Context Menu
The system SHALL open a per-pad context menu on middle-click. The context menu SHALL provide a single audio-slot action whose label reflects the current pad state: "Load Audio" when no audio is loaded, and "Unload Audio" when audio is loaded.

#### Scenario: Middle-click opens a pad menu
- **WHEN** the performer middle-clicks a pad
- **THEN** a context menu for that pad is shown

#### Scenario: Context menu shows "Load Audio" for an empty pad
- **WHEN** the pad context menu is open for a pad with no loaded audio
- **THEN** the context menu contains an action labeled "Load Audio"

#### Scenario: Selecting "Load Audio" opens a file dialog
- **WHEN** the pad context menu is open for a pad with no loaded audio
- **AND** the performer selects the "Load Audio" action
- **THEN** the system opens a file selection dialog filtered to at least: `wav`, `flac`, `mp3`, `aif/aiff`, `ogg`

#### Scenario: Selecting a file loads audio into the pad
- **WHEN** the file selection dialog is open
- **AND** the performer selects a valid audio file
- **THEN** the selected file is loaded into the pad’s sample slot
- **AND** subsequent left-click triggers play the loaded audio as a loop (see `play-samples`)

#### Scenario: Cancelling the file dialog leaves the pad unchanged
- **WHEN** the file selection dialog is open
- **AND** the performer cancels the dialog
- **THEN** no pad audio is loaded/unloaded

#### Scenario: Context menu shows "Unload Audio" for a loaded pad
- **WHEN** the pad context menu is open for a pad with audio loaded
- **THEN** the context menu contains an action labeled "Unload Audio"

#### Scenario: Selecting "Unload Audio" unloads the pad
- **WHEN** the pad context menu is open for a pad with audio loaded
- **AND** the performer selects the "Unload Audio" action
- **THEN** the pad’s audio is unloaded (see `load-audio-files`)
- **AND** any currently playing audio for that pad stops promptly

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

