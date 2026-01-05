# performance-ui Specification

## Purpose
To define the primary performance view UI (6×6 pad grid + 6-bank selector) with stable control identifiers and a legacy-inspired theme.
## Requirements
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

### Requirement: Bank Selector
The system SHALL provide 6 bank selector controls (Bank 1..6) and highlight the currently selected bank. The bank selector buttons are positioned below the pad grid.

#### Scenario: Bank 1 is selected by default
- **WHEN** the UI is started
- **THEN** Bank 1 is visually indicated as selected

#### Scenario: Selecting a different bank updates the selection
- **WHEN** the user selects Bank 3
- **THEN** Bank 3 is visually indicated as selected
- **AND** Bank 1 is visually indicated as not selected

### Requirement: Legacy-Inspired Theme
The system SHALL apply a legacy-inspired theme for the performance UI using a dark background and high-contrast buttons.

#### Scenario: Theme uses legacy palette defaults
- **WHEN** the UI is started
- **THEN** the performance UI background uses `#1e1e1e` (or an equivalent dark color)
- **AND** pad controls use `#3a3a3a` (or an equivalent inactive-pad color)
- **AND** active pad controls use `#2ecc71` (legacy `COLOR_BTN_ACTIVE`, or an equivalent active-pad color)
- **AND** active pad label text uses `#000000` (or an equivalent active-text color)
- **AND** bank selector controls use distinct colors for active vs inactive (e.g., `#ffaa00` vs `#cc7700`)

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

### Requirement: Global Speed Controls
The system SHALL provide global speed controls in the performance view consisting of:
- A speed control that allows selecting a speed multiplier in the range 0.5×..2.0×.
- A speed increase action ("+") that increases the speed multiplier by 0.05×.
- A speed decrease action ("-") that decreases the speed multiplier by 0.05×.
- A reset action that restores the speed multiplier to 1.0×.

The speed control MUST default to 1.0× on startup.

The speed control SHALL be rendered vertically and positioned to the right of the performance pad grid.
The speed increase/decrease actions and reset action SHALL be positioned adjacent to the speed control and vertically aligned.

#### Scenario: Speed controls are visible in the performance view
- **WHEN** the UI is started
- **THEN** a global speed control is visible
- **AND** a global speed increase action is visible
- **AND** a global speed reset action is visible
- **AND** a global speed decrease action is visible

#### Scenario: Adjusting speed sends a speed update to the audio engine
- **GIVEN** the audio engine is running
- **WHEN** the performer changes the global speed control from 1.0× to 1.25×
- **THEN** the application calls `AudioEngine.set_speed(1.25)`

#### Scenario: Increment and decrement adjust speed in fixed steps
- **GIVEN** the global speed multiplier is 1.0×
- **WHEN** the performer activates the speed increase action once
- **THEN** the global speed multiplier becomes 1.05×
- **WHEN** the performer activates the speed decrease action once
- **THEN** the global speed multiplier becomes 1.0×

#### Scenario: Reset restores default speed
- **GIVEN** the global speed multiplier is not 1.0×
- **WHEN** the performer activates the reset action
- **THEN** the global speed multiplier becomes 1.0×
- **AND** the UI control reflects 1.0×

### Requirement: Stable Speed Control Identifiers
The system SHALL assign deterministic item tags to the global speed controls to enable programmatic updates and event binding.

#### Scenario: Speed control tags are stable
- **WHEN** the UI is started
- **THEN** the speed control exists with tag `speed_slider`
- **AND** the speed increase action exists with tag `speed_plus_btn`
- **AND** the speed reset action exists with tag `speed_reset_btn`
- **AND** the speed decrease action exists with tag `speed_minus_btn`

### Requirement: Pad Loading Progress Indicator
When a pad’s sample slot is being loaded asynchronously (see `async-sample-loading`), the system **SHALL** show a loading progress indicator directly on that pad.

The indicator **SHALL** include:
- The current loader `stage` text (main task with optional sub-task), e.g. `Loading (decoding)`.
- The current total progress percentage rendered as an integer percent string (e.g. `33 %`).
- A background progress bar rendered as a filled rectangle whose width is proportional to progress.

The system **SHALL** show the stage + percentage in the selected-pad sidebar as well when the selected pad is loading.

The progress bar color **SHALL** be a slightly darker shade than the pad’s normal background color.

#### Scenario: Loading pad shows stage and percentage text
- **WHEN** a pad is loading and the UI has received a `LoaderEvent::Progress` for that pad
- **THEN** the pad label includes the current `stage` and a percentage derived from `percent`

#### Scenario: Loading pad shows a progress bar
- **WHEN** a pad is loading and has `percent == 0.33`
- **THEN** the pad shows a filled rectangle background whose width is approximately 33% of the pad width

#### Scenario: Selected-pad sidebar shows stage and percentage
- **WHEN** the selected pad is loading and the UI has received a `LoaderEvent::Progress` for that pad
- **THEN** the sidebar shows the current `stage` and a percentage derived from `percent`

#### Scenario: Progress indicator clears on completion
- **WHEN** a pad finishes loading successfully
- **THEN** the loading progress indicator is no longer shown on that pad
- **AND** the pad returns to its normal background rendering

### Requirement: Display Pad BPM And Key
When a pad has BPM information, the system SHALL display BPM for that pad.

When a pad has detected analysis metadata, the system SHALL display the pad’s key.

The BPM and key SHALL be shown:
- In the pad control, positioned in the top-right corner.
- In the selected-pad sidebar.

When a manual BPM exists for a pad (see `pad-manual-bpm`), the displayed BPM SHALL use that manual BPM value instead of the detected BPM.

#### Scenario: Pad shows BPM and key when available
- **GIVEN** a pad has a loaded sample with detected BPM and key
- **WHEN** the performance view is rendered
- **THEN** the pad renders BPM and key in its top-right corner

#### Scenario: Sidebar shows BPM and key for selected pad
- **GIVEN** the selected pad has a loaded sample with detected BPM and key
- **WHEN** the sidebar is rendered
- **THEN** the sidebar renders BPM and key for the selected pad

#### Scenario: Manual BPM overrides detected BPM in display
- **GIVEN** a pad has a loaded sample with detected BPM and key
- **AND** the pad also has a manual BPM value
- **WHEN** the performance view is rendered
- **THEN** the pad renders the manual BPM value as BPM

#### Scenario: Manual BPM displays even without analysis
- **GIVEN** a pad has a loaded sample
- **AND** the pad has a manual BPM value
- **AND** the pad has no detected analysis metadata
- **WHEN** the performance view is rendered
- **THEN** the pad renders BPM
- **AND** the pad does not render a key value

### Requirement: Manual BPM Entry In Selected-Pad Sidebar
When a pad is selected and has audio loaded, the system SHALL provide a manual BPM entry control in the left sidebar.

The manual BPM entry control SHALL:
- Accept a numeric BPM value (float).
- Apply the value to the selected pad as its manual BPM.
- Allow clearing the value to remove the manual BPM override.

#### Scenario: Entering a BPM sets manual BPM
- **GIVEN** a pad is selected and has audio loaded
- **WHEN** the performer enters a BPM value (e.g., 120.0) in the sidebar control
- **THEN** the selected pad’s manual BPM becomes 120.0

#### Scenario: Clearing the BPM removes the manual override
- **GIVEN** a pad is selected and has audio loaded
- **AND** the pad currently has a manual BPM
- **WHEN** the performer clears the BPM value in the sidebar control
- **THEN** the selected pad’s manual BPM becomes unset

### Requirement: Tap BPM Control In Selected-Pad Sidebar
When a pad is selected and has audio loaded, the system SHALL provide a Tap BPM control in the left sidebar.

The Tap BPM control SHALL register a tap on **left mouse button down** (not on button release).

Activating the Tap BPM control repeatedly SHALL compute and set manual BPM for the selected pad (see `pad-manual-bpm`).

#### Scenario: Tap BPM uses mouse down
- **GIVEN** a pad is selected and has audio loaded
- **WHEN** the performer presses the left mouse button down on the Tap BPM control
- **THEN** the system records a Tap BPM event immediately

#### Scenario: Tap BPM sets manual BPM
- **GIVEN** a pad is selected and has audio loaded
- **WHEN** the performer taps the Tap BPM control repeatedly
- **THEN** the selected pad’s manual BPM is updated based on the computed BPM

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

