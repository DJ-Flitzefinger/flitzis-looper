## MODIFIED Requirements

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
