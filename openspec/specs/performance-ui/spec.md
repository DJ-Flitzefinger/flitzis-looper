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
- **WHEN** audio is loaded into a pad’s sample slot (see `performance-pad-interactions`, `load-audio-files`)
- **THEN** that pad’s label shows the loaded audio file’s basename (filename only, no directory path)

#### Scenario: Unloading restores numeric labels
- **WHEN** audio is unloaded from a pad’s sample slot (see `performance-pad-interactions`, `load-audio-files`)
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

