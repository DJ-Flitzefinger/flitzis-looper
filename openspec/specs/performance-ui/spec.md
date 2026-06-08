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

### Requirement: Selected-pad sidebar offers loop editing
When a pad is selected and has loaded audio, the system SHALL provide an action labeled "Adjust Loop" in the selected-pad sidebar.

Activating "Adjust Loop" SHALL open the waveform editor for the selected pad (see `waveform-editor`).

#### Scenario: Adjust Loop action is available for loaded pad
- **GIVEN** a pad is selected
- **AND** the pad has loaded audio
- **WHEN** the left sidebar is rendered
- **THEN** the sidebar contains an action labeled "Adjust Loop"

#### Scenario: Adjust Loop opens waveform editor
- **GIVEN** a pad is selected
- **AND** the pad has loaded audio
- **WHEN** the performer activates "Adjust Loop"
- **THEN** the waveform editor window opens for that pad


<!-- Added from add-low-jitter-input-mapping -->
### Requirement: Input Mapping Settings Controls
The system SHALL provide Settings controls for enabling input mapping and clearing keyboard or
MIDI mappings.

The Settings page SHALL include `Input Mapping: ON/OFF`, `Delete all Keyboard Mappings`, and
`Delete all MIDI Mappings`. It SHALL NOT add a mapping editor, MIDI device selector, MIDI output,
LED feedback, conflict dialog, `Open midi.json`, or `Open keyboard.json` in this change.

#### Scenario: Settings toggles input mapping
- **GIVEN** the Settings page is open
- **WHEN** the performer turns Input Mapping on
- **THEN** project state records input mapping as enabled
- **AND** Python publishes the enabled state to the Rust input layer

#### Scenario: Settings clears all MIDI mappings
- **GIVEN** the Settings page is open
- **WHEN** the performer activates `Delete all MIDI Mappings`
- **THEN** `config/input/midi.json` is rewritten with `mappings = []`
- **AND** the Rust MIDI mapping snapshot is refreshed

#### Scenario: Settings clears all keyboard mappings
- **GIVEN** the Settings page is open
- **WHEN** the performer activates `Delete all Keyboard Mappings`
- **THEN** `config/input/keyboard.json` is rewritten with `mappings = []`
- **AND** normal keyboard playback no longer resolves the cleared mappings


<!-- Added from add-low-jitter-input-mapping -->
### Requirement: Learn Control Is Direct And Non-Destructive
The system SHALL provide a direct Learn control without replacing the performance surface.

The `L` control SHALL activate Learn, show active/pending state through session/UI state, and clear
text input focus when Learn starts. The Learn control SHALL NOT open a separate mapping editor or
require a MIDI device selector.

#### Scenario: Learn starts from bottom bar
- **GIVEN** input mapping is enabled
- **WHEN** the performer activates `L`
- **THEN** Learn waits for one keyboard or MIDI input
- **AND** text input focus is cleared


<!-- Added from add-per-pad-key-lock -->
### Requirement: Selected-Pad Key Lock Control
The system SHALL provide a per-pad `KEY LOCK` control in the selected-pad left sidebar.

The selected-pad `KEY LOCK` control SHALL be rendered at the bottom of the selected-pad sidepanel under the Stem Mix / stem controls only when the selected pad has loaded audio. The control SHALL NOT be rendered for unloaded or loading selected pads. The control SHALL use the same `mode-on` and `mode-off` visual language as the global Key Lock control. Activating the selected-pad `KEY LOCK` control SHALL toggle only the selected loaded pad's per-pad Key Lock value.

#### Scenario: Selected pad shows per-pad Key Lock control
- **GIVEN** a loaded pad is selected
- **WHEN** the left selected-pad sidebar is rendered
- **THEN** a `KEY LOCK` control is visible under the Stem Mix / stem controls
- **AND** the control visually reflects the selected pad's per-pad Key Lock value

#### Scenario: Empty selected pad hides per-pad Key Lock control
- **GIVEN** an unloaded pad is selected
- **WHEN** the left selected-pad sidebar is rendered
- **THEN** no selected-pad `KEY LOCK` control is rendered
- **AND** the unloaded pad has no enabled per-pad Key Lock value

#### Scenario: Per-pad Key Lock enables only the selected pad
- **GIVEN** global Key Lock is disabled
- **AND** Pad 3 is selected
- **AND** Pad 3 has loaded audio
- **WHEN** the performer activates the selected-pad `KEY LOCK` control
- **THEN** Pad 3's per-pad Key Lock value is enabled
- **AND** other pads' per-pad Key Lock values remain disabled

#### Scenario: Per-pad Key Lock can override a global baseline
- **GIVEN** global Key Lock has been enabled
- **AND** currently loaded pads have enabled per-pad Key Lock values
- **AND** Pad 3 is selected
- **AND** Pad 3 has loaded audio
- **WHEN** the performer activates the selected-pad `KEY LOCK` control
- **THEN** Pad 3's per-pad Key Lock value is disabled
- **AND** other loaded pads' per-pad Key Lock values remain enabled
- **AND** unloaded pads have no enabled per-pad Key Lock value


<!-- Added from add-rust-transport-timeline -->
### Requirement: Trigger Quantization Controls
The system SHALL expose trigger quantization as a bottom-bar `Q` toggle and move the
quantization grid selection to the Settings page.

The bottom-bar `Q` button SHALL default to disabled for new projects, SHALL render with the
disabled red mode style while disabled, and SHALL render with the enabled green mode style while
enabled. Activating `Q` SHALL toggle only the global trigger-quantization enabled state.

The performance view SHALL NOT render the previous `IMMEDIATE`/`BEAT`/`BAR` segmented trigger
quantization controls. The bottom-bar `Q`, input-learn `L`, Multi Loop button, selected-pad stem
mask buttons, and Settings toggle SHALL share a consistent horizontal alignment and SHALL be
visually grouped by function.

The Settings page SHALL expose the persisted trigger quantization grid as fixed musical steps:
`1/16`, `1/32`, and `1/64`. The default grid step SHALL be `1/32`, and the minimum `1/64` step
SHALL match the loop editor's finest musical grid line spacing when the loop editor is zoomed far
enough to show that grid.

The UI SHALL NOT bypass controller actions, send full beat-grid metadata, touch audio-thread
state directly, perform disk I/O in the audio callback, acquire the Python GIL in the audio
callback, or introduce unbounded audio-thread work.

#### Scenario: New projects default to disabled triggering
- **WHEN** the application starts with a new project
- **THEN** the bottom-bar `Q` button indicates disabled trigger quantization
- **AND** pad triggers preserve immediate behavior unless the performer enables `Q`
- **AND** the Settings page shows `1/32` as the selected quantization grid

#### Scenario: Enabling trigger quantization publishes the selected grid
- **GIVEN** the application is running with trigger quantization disabled
- **AND** the Settings page trigger quantization grid is `1/32`
- **WHEN** the performer activates the bottom-bar `Q` button
- **THEN** the project trigger quantization enabled state becomes `true`
- **AND** the control layer calls `AudioEngine.set_trigger_quantization("1_32")`
- **AND** the bottom-bar `Q` button indicates enabled trigger quantization

#### Scenario: Changing the Settings grid while disabled is persisted
- **GIVEN** trigger quantization is disabled
- **WHEN** the performer changes the Settings page trigger quantization grid to `1/32`
- **THEN** the project stores `trigger_quantization_step = "1_32"`
- **AND** the control layer does not send an audio-thread trigger quantization update until
  trigger quantization is enabled

#### Scenario: Legacy quantization mode is restored as a grid
- **GIVEN** a saved project has legacy trigger quantization mode `next_beat`
- **WHEN** the project is loaded
- **THEN** the project stores trigger quantization as enabled with grid step `1_16`
- **AND** the control layer applies `1_16` to the Rust audio engine


<!-- Added from add-stem-performance-controls -->
### Requirement: Stem Availability Indicators
The system SHALL show per-pad stem availability, generation progress, blocked state, and
errors in the performance UI without blocking rendering.

The pad grid MAY use compact indicators, while the selected-pad sidebar SHALL provide the
detailed status for the selected pad. Pad-grid compact indicators SHALL NOT show hover tooltips
or hover status messages. UI rendering SHALL use controller/session snapshots and SHALL NOT
inspect cache directories, decode audio, run inference, or perform blocking work.

#### Scenario: Selected pad shows available stems
- **GIVEN** the selected pad has loaded audio
- **AND** the pad has a complete current prepared stem cache
- **WHEN** the performance UI is rendered
- **THEN** the selected-pad sidebar indicates that stems are available
- **AND** the pad grid may show a compact stem-available indicator for that pad

#### Scenario: Stem generation progress is visible
- **GIVEN** stem generation is running for the selected pad
- **WHEN** the performance UI is rendered
- **THEN** the selected-pad sidebar shows the current generation stage and progress when available
- **AND** the UI remains responsive while generation continues outside the audio callback

#### Scenario: Stem generation error preserves pad usability
- **GIVEN** stem generation failed for the selected pad
- **WHEN** the performance UI is rendered
- **THEN** the selected-pad sidebar shows the error outside the audio callback
- **AND** the pad remains playable using full-mix playback

#### Scenario: Pad-grid indicators do not show hover messages
- **GIVEN** the pad grid shows a compact stem status indicator for a pad
- **WHEN** the performer hovers that pad
- **THEN** the UI does not show a stem status tooltip or hover message over the pad
- **AND** the selected-pad sidebar remains the detailed status surface


<!-- Added from add-stem-performance-controls -->
### Requirement: Selected-Pad Stem Generation Action
The system SHALL provide a selected-pad action for requesting offline stem generation through
the controller layer.

The action SHALL be available only for a loaded pad and SHALL follow the controller's
inactive-pad and per-pad background-task gating. The UI SHALL NOT call Rust background
generation directly, bypass the controller, or run stem work in the render loop.

#### Scenario: Generate action schedules a stopped loaded pad
- **GIVEN** the selected pad has loaded audio
- **AND** the selected pad is not playing, loading, analyzing, or already generating stems
- **WHEN** the performer activates Generate Stems
- **THEN** the UI emits a controller action for that selected pad
- **AND** stem generation is scheduled as offline/background work

#### Scenario: Generate action is blocked for an active pad
- **GIVEN** the selected pad is currently playing
- **WHEN** the performer tries to generate stems for that pad
- **THEN** the request is rejected or disabled through controller state
- **AND** no stem generation work runs in the audio callback


<!-- Added from add-stem-performance-controls -->
### Requirement: Selected-Pad Stem Deletion Action
The system SHALL provide a selected-pad Delete Stems action next to the Generate Stems action.

The Delete Stems action SHALL route through the controller layer, SHALL remove only the selected
pad's tracked project-local stem cache artifacts, SHALL clear the selected pad's stem cache
metadata, and SHALL leave the loaded full-mix audio available. UI rendering SHALL decide whether
to enable the action from controller/session snapshots and SHALL NOT inspect cache directories.

#### Scenario: Delete action removes selected pad stems
- **GIVEN** the selected pad has tracked cached stems
- **WHEN** the performer activates Delete Stems
- **THEN** the UI emits a controller action for that selected pad
- **AND** the selected pad returns to full-mix playback with no available stems

#### Scenario: Delete action is disabled without tracked stems
- **GIVEN** the selected pad has no tracked stem cache metadata
- **WHEN** the performance UI is rendered
- **THEN** the Delete Stems action is disabled
- **AND** rendering does not inspect cache directories, read files, decode audio, or run inference


<!-- Added from add-stem-performance-controls -->
### Requirement: Stem Mix Controls
The system SHALL provide selected-pad controls for choosing full-mix playback or all prepared
stems when a current prepared stem set is available.

New projects SHALL default to full-mix playback. The full-mix/all-stems selection buttons SHALL
be disabled when the selected pad has no current prepared stem set. Selecting all-stems mode
SHALL request prepared-stem playback for the selected pad only when valid stems are available and
SHALL fall back to full mix when stems are unavailable, stale, incomplete, rejected, deleted, or
disabled.

#### Scenario: New projects default to full mix
- **WHEN** the application starts with a new project
- **THEN** the selected-pad stem mix control defaults each pad to full-mix playback
- **AND** prepared stems do not affect playback until the performer chooses stem playback

#### Scenario: All-stems mode requires current prepared stems
- **GIVEN** the selected pad has a complete current prepared stem set
- **WHEN** the performer selects all-stems mode
- **THEN** the project stem mix preference for that pad becomes all-stems
- **AND** the control layer sends a bounded stem mix update to the Rust audio engine

#### Scenario: Mix mode buttons are disabled without stems
- **GIVEN** the selected pad has no current prepared stem set
- **WHEN** the selected-pad sidebar is rendered
- **THEN** the full-mix and all-stems buttons are disabled
- **AND** the UI does not persist an all-stems preference for that pad

#### Scenario: Revert to full mix
- **GIVEN** the selected pad is configured for all-stems mode
- **WHEN** the performer selects full-mix mode
- **THEN** the project stem mix preference for that pad becomes full-mix
- **AND** the pad uses the loaded full-mix buffer without requiring stem cache deletion


<!-- Added from add-stem-performance-controls -->
### Requirement: Bottom-Bar Per-Stem Mask Controls
The system SHALL render bottom-bar selected-pad stem mask controls as six compact buttons ordered
`V`, `D`, `M`, `B`, `I`, and `A`.

The buttons SHALL target the currently selected red-outlined pad. `V`, `D`, `M`, and `B` SHALL be
freely combinable component toggles. `I` SHALL select the instrumental preset Drums + Melody + Bass
and mute Vocals. `A` SHALL select the all-stems preset Vocals + Drums + Melody + Bass. `I` SHALL
NOT mean playing only `instrumental.wav`, and `A` SHALL NOT add `instrumental.wav` as a fifth
audible layer.
The system SHALL treat `I` and `A` as explicit preset display states rather than inferred aliases
for matching custom masks. Activating `V`, `D`, `M`, or `B` from either preset SHALL leave preset
display mode, enter custom display mode, and set the custom mask to only the clicked component stem.
The system SHALL treat `I` and `A` as one exclusive preset group and `V`, `D`, `M`, and `B` as a
separate component group. Activating a preset SHALL remember the last component-group custom mask,
switching between `I` and `A` SHALL preserve that remembered component mask, and clicking the
currently active preset again SHALL deactivate the preset group and restore the remembered
component mask.
Right-clicking `V`, `D`, `M`, or `B` SHALL set a non-momentary custom solo state for that component
stem without adding a separate mute feature.

#### Scenario: Selected pad controls are disabled without prepared stem playback
- **GIVEN** the selected pad is in full-mix mode
- **WHEN** the performance bottom bar is rendered
- **THEN** the `V`, `D`, `M`, `B`, `I`, and `A` buttons are disabled
- **AND** rendering does not inspect cache directories, compute source versions, read files, decode audio, run inference, or call low-level Rust background task APIs

#### Scenario: Component toggles update the selected pad
- **GIVEN** the selected pad has a current prepared stem set
- **AND** the selected pad is in all-stems mode
- **WHEN** the performer toggles `V`, `D`, `M`, or `B`
- **THEN** the selected pad's bounded enabled-stem mask is updated through controller actions
- **AND** currently playing voices are not stopped, retriggered, time-slipped, or moved to a different loop position by the toggle

#### Scenario: Preset buttons display exclusive state
- **GIVEN** the selected pad has a current prepared stem set
- **AND** the selected pad is in all-stems mode
- **WHEN** the performer selects `I`
- **THEN** only the `I` button appears active
- **AND** the underlying enabled-stem mask enables Drums, Melody, and Bass while Vocals remains disabled

#### Scenario: Component click leaves all-stems preset for custom mode
- **GIVEN** the selected pad has a current prepared stem set
- **AND** the selected pad is in all-stems mode
- **AND** the `A` preset display state is active
- **WHEN** the performer clicks `M`
- **THEN** the selected pad enters custom display mode
- **AND** only the Melody component appears active
- **AND** the `A` preset appears inactive

#### Scenario: Component click leaves instrumental preset for custom mode
- **GIVEN** the selected pad has a current prepared stem set
- **AND** the selected pad is in all-stems mode
- **AND** the `I` preset display state is active
- **WHEN** the performer clicks `V`
- **THEN** the selected pad enters custom display mode
- **AND** only the Vocals component appears active
- **AND** the `I` preset appears inactive

#### Scenario: Component right-click sets non-momentary solo
- **GIVEN** the selected pad has a current prepared stem set
- **AND** the selected pad is in all-stems mode
- **WHEN** the performer right-clicks `D`
- **THEN** the selected pad enters custom display mode
- **AND** only the Drums component appears active
- **AND** the state persists until the performer changes the component or preset buttons again

#### Scenario: Custom masks do not auto-select matching presets
- **GIVEN** the selected pad has a current prepared stem set
- **AND** the selected pad is in all-stems mode
- **WHEN** the custom component mask matches Drums + Melody + Bass or Vocals + Drums + Melody + Bass
- **THEN** the component buttons reflect the custom mask
- **AND** the `I` and `A` preset buttons remain inactive until explicitly clicked

#### Scenario: Preset deactivation restores remembered components
- **GIVEN** the selected pad has a current prepared stem set
- **AND** the selected pad is in all-stems mode
- **AND** the custom component mask enables Vocals and Bass
- **WHEN** the performer clicks `I`
- **AND** the performer clicks `I` again
- **THEN** the selected pad returns to custom display mode
- **AND** only Vocals and Bass appear active

#### Scenario: Preset switching preserves remembered components
- **GIVEN** the selected pad has a current prepared stem set
- **AND** the selected pad is in all-stems mode
- **AND** the custom component mask enables Drums and Melody
- **WHEN** the performer clicks `I`
- **AND** the performer switches between `I` and `A` one or more times
- **AND** the performer clicks the currently active preset again
- **THEN** the selected pad returns to custom display mode
- **AND** only Drums and Melody appear active


<!-- Added from add-stem-performance-controls -->
### Requirement: Settings Overlay
The system SHALL provide a bottom-right Settings toggle that replaces the main Looper display
area with a Settings page while open.

The closed state SHALL show a gear icon at the right edge of the center bottom bar, aligned with
the right edge of the bank-button row. The open state SHALL show an `X` close icon in the same
bottom-right location and SHALL return to the normal Looper display when activated. The first
Settings page controls SHALL configure bounded Demucs stem-generation quality values: shifts from
1 through 20, default 1, and overlap from 0.25 through 0.95, default 0.5. Rendering the Settings
page SHALL use project/session state and controller actions only; it
SHALL NOT inspect cache directories, compute source versions, read files, decode audio, invoke
Demucs, download models, or call low-level Rust background-task APIs from the render loop.

#### Scenario: Gear opens Settings
- **GIVEN** the normal Looper display is visible
- **WHEN** the performer activates the bottom-right gear icon
- **THEN** the Settings page replaces the main Looper display area
- **AND** the bottom-right toggle changes to an `X` close icon
- **AND** the toggle remains right-aligned with the bank-button row

#### Scenario: Close returns to Looper
- **GIVEN** the Settings page is open
- **WHEN** the performer activates the bottom-right `X` close icon
- **THEN** the normal Looper display area is rendered again
- **AND** the Settings page no longer covers the main Looper display area
- **AND** the toggle remains right-aligned with the bank-button row

#### Scenario: Stem quality controls update project settings
- **GIVEN** the Settings page is open
- **WHEN** the performer sets Demucs shifts to 4
- **AND** the performer sets Demucs overlap to 0.25
- **THEN** those bounded quality values are stored in project state
- **AND** the next Generate Stems request uses those values in its backend request

#### Scenario: Settings render loop stays non-blocking
- **GIVEN** the Settings page is open
- **WHEN** the UI renders a frame
- **THEN** rendering does not inspect cache directories, compute source versions, read files, decode audio, invoke Demucs, download models, or call low-level Rust background-task APIs


<!-- Added from fix-waveform-editor-unload-return -->
### Requirement: Unloading Edited Pad Returns To Performance View
The system SHALL close the waveform editor and return the center surface to the performance pad
view when audio is unloaded from the pad currently being edited.

Unloading audio from a different pad MUST NOT close or retarget an open waveform editor for another
loaded pad.

#### Scenario: Sidebar unload returns to pad view
- **GIVEN** the waveform editor is open for pad `id`
- **AND** pad `id` has loaded audio
- **WHEN** the performer activates "Unload Audio" for pad `id`
- **THEN** pad `id` audio is unloaded
- **AND** the waveform editor is closed
- **AND** the center surface renders the performance pad view

#### Scenario: Unloading another pad preserves editor
- **GIVEN** the waveform editor is open for pad `A`
- **AND** pad `A` has loaded audio
- **WHEN** audio is unloaded from pad `B`
- **THEN** the waveform editor remains open for pad `A`


<!-- Added from repair-key-lock-master-tempo -->
### Requirement: Manual Key Lock DSP Settings
The system SHALL expose all bounded Key Lock DSP parameters in a dedicated Settings-page block.

The block SHALL allow the performer to edit delay minimum, delay range, head count,
interpolation, window, smoothing step, and output gain within the documented supported ranges. Each
parameter control SHALL display adjacent or immediately following text explaining the performance
or sound tradeoff of higher and lower values, or for enum parameters the tradeoff of each option.
The Settings page SHALL persist the concrete parameter values with the project and publish them to
Rust as bounded control-plane state.

#### Scenario: Settings page lists manual Key Lock parameters
- **GIVEN** the Settings page is open
- **WHEN** the performer inspects Key Lock DSP
- **THEN** controls are available for delay minimum, delay range, head count, interpolation,
  window, smoothing step, and output gain
- **AND** each parameter shows a nearby performance or sound tradeoff hint

#### Scenario: Manual Key Lock parameters persist
- **GIVEN** the performer changes delay range and output gain within the supported ranges
- **WHEN** the project is saved and loaded again
- **THEN** the same concrete Key Lock parameter values are restored
- **AND** Rust receives those concrete parameters when project state is restored


<!-- Added from refine-ui-label-fit-and-hit-targets -->
### Requirement: Filename Text Fit
The system SHALL keep loaded audio filename text inside the performance UI without clipping in
the pad grid or selected-pad sidebar.

#### Scenario: Loaded pad filename wraps inside pad bounds
- **GIVEN** a pad has a loaded audio file with a basename that is wider than the pad
- **WHEN** the performance pad grid is rendered
- **THEN** the pad filename is wrapped to at most three visible title lines
- **AND** the wrapped title has approximately one character of horizontal inset from both pad edges
- **AND** the wrapped title block is vertically centered as a block, starting higher than the
  previous single-line center when multiple lines are needed

#### Scenario: Sidebar filename wraps without clipping
- **GIVEN** the selected pad has a loaded audio file with a long basename
- **WHEN** the selected-pad sidebar is rendered
- **THEN** the `Filename` value wraps in the remaining row width instead of being clipped
- **AND** no additional filename-specific horizontal padding is added in the sidebar


<!-- Added from refine-ui-label-fit-and-hit-targets -->
### Requirement: Performance Control Hit Targets
The system SHALL align bottom-bar mode controls consistently and size continuous performance
controls so they are quick to operate.

#### Scenario: Stem mask buttons align on one row
- **WHEN** the bottom bar is rendered
- **THEN** the `V`, `D`, `M`, `B`, `I`, and `A` stem buttons share the same vertical centerline

#### Scenario: Master Volume slider is wider
- **WHEN** the bottom bar is rendered
- **THEN** the Master Volume slider hit target is wider than before and at least 300 px wide

#### Scenario: Pitch fader grab is taller
- **WHEN** the right-side Pitch fader is rendered
- **THEN** its grab is enlarged symmetrically along the vertical axis
- **AND** the Pitch fader value range, center marker behavior, and BPM interaction semantics remain unchanged


<!-- Added from refine-ui-label-fit-and-hit-targets -->
### Requirement: UI Polish Realtime Boundary
The system SHALL keep these UI presentation refinements outside the realtime audio callback.

#### Scenario: Rendering refinements do not add realtime work
- **WHEN** filename wrapping, bottom-bar alignment, Master Volume width, or Pitch grab sizing is changed
- **THEN** no disk I/O, Python/GIL access, logging, blocking work, heavy allocation, neural inference,
  or new callback work is added to the Rust audio callback


<!-- Added from refine-pitch-bpm-control -->
### Requirement: Wheel And Middle-Click Gestures For Continuous Controls
The system SHALL allow hovered continuous controls to respond to mouse-wheel nudges and mouse-wheel
button reset clicks.

Hovering Master Volume then scrolling the mouse wheel SHALL adjust the control by five percentage
points per wheel movement. Hovering Gain then scrolling the mouse wheel SHALL adjust the control by
one percentage point per wheel movement. Hovering a per-pad EQ control then scrolling the mouse
wheel SHALL adjust that EQ band by 1.0 dB per wheel movement. Hovering Master Volume and clicking
the mouse wheel SHALL reset it to 100 percent. Hovering Gain and clicking the mouse wheel SHALL
reset it to 100 percent. Hovering a per-pad EQ control and clicking the mouse wheel SHALL reset
that band to 0.0 dB.

These gestures SHALL remain UI/controller behavior and SHALL NOT add disk I/O, Python/GIL access,
logging, blocking work, heavy allocation, neural inference, or any new work to the Rust audio
callback.

#### Scenario: Master Volume wheel and middle-click gestures
- **GIVEN** Master Volume is 50 percent
- **WHEN** the performer hovers Master Volume and scrolls upward once
- **THEN** Master Volume becomes approximately 55 percent
- **WHEN** the performer clicks the mouse wheel while hovering Master Volume
- **THEN** Master Volume resets to 100 percent

#### Scenario: Gain wheel and middle-click gestures
- **GIVEN** the selected pad Gain is 50 percent
- **WHEN** the performer hovers Gain and scrolls downward once
- **THEN** Gain becomes approximately 49 percent
- **WHEN** the performer clicks the mouse wheel while hovering Gain
- **THEN** Gain resets to 100 percent

#### Scenario: EQ wheel and middle-click gestures
- **GIVEN** the selected pad Mid EQ is 0.0 dB
- **WHEN** the performer hovers Mid EQ and scrolls upward once
- **THEN** Mid EQ becomes approximately 1.0 dB
- **WHEN** the performer clicks the mouse wheel while hovering Mid EQ
- **THEN** Mid EQ resets to 0.0 dB


<!-- Added from refine-pitch-bpm-control -->
### Requirement: Pitch Control Center Indicator
The system SHALL render a small horizontal center-position indicator beside the Pitch control at
the 1.00x/default speed position.

The indicator SHALL align with the Pitch fader's neutral grab position using the same usable slider
track geometry as the rendered fader. The indicator SHALL be green only while the speed multiplier
is at the neutral 1.00x/default speed position; otherwise it SHALL use the same grey as the Pitch
fader grab. The indicator SHALL be visual only and SHALL NOT perform file access, analysis, audio
work, or input mapping dispatch.

#### Scenario: Center indicator marks neutral pitch
- **GIVEN** the performance view is rendered
- **WHEN** the Pitch control is visible
- **THEN** a small horizontal marker is drawn beside the control at the neutral 1.00x position

#### Scenario: Center indicator color follows neutral state
- **GIVEN** the speed multiplier is 1.00x
- **WHEN** the Pitch control is rendered
- **THEN** the center indicator is green
- **WHEN** the performer changes Pitch away from 1.00x
- **THEN** the center indicator is grey


<!-- Added from refine-pitch-bpm-control -->
### Requirement: BPM Display Manual Entry
The system SHALL allow the performer to double-click the right-side BPM display and type a target
BPM with at most two decimal places.

The BPM entry SHALL accept only digits, `.`, and `,` at input time, so disallowed characters SHALL
not appear in the field. The entry SHALL interpret `,` as `.` and SHALL ignore all other typed
characters. The committed BPM SHALL be converted to the existing bounded speed multiplier using
the current BPM reference, and invalid or non-positive entries SHALL NOT update the speed.

This manual entry SHALL remain Python/UI control-plane behavior and SHALL NOT add disk I/O,
Python/GIL access, logging, blocking work, heavy allocation, neural inference, or any new work to
the Rust audio callback.

#### Scenario: Double-click opens exact BPM entry
- **GIVEN** the BPM display shows 120.00
- **WHEN** the performer double-clicks the BPM display
- **THEN** the display becomes a text entry initialized to `120.00`

#### Scenario: Comma input is normalized and limited to two decimals
- **GIVEN** the BPM entry is active
- **WHEN** the performer types `123,456abc`
- **THEN** the entry buffer becomes `123.45`
- **AND** committing the entry targets 123.45 BPM

#### Scenario: Invalid BPM entry is ignored
- **GIVEN** the BPM entry is active
- **WHEN** the performer commits an empty, zero, or non-positive value
- **THEN** the speed multiplier is not updated


<!-- Added from refine-gain-master-stop-controls -->
### Requirement: Master Volume Direct Mute Gesture
The system SHALL set Master Volume to the persisted minimum value when the performer right-clicks
the Master Volume slider.

The direct mute gesture SHALL move the visible Master Volume slider to the leftmost position and
SHALL persist the same `0.0` value as other explicit Master Volume edits. This UI/controller
behavior SHALL NOT add disk I/O, Python/GIL access, logging, blocking work, heavy allocation,
neural inference, or any new work to the Rust audio callback.

#### Scenario: Right-clicking Master Volume sets it to zero
- **GIVEN** Master Volume is above zero
- **WHEN** the performer right-clicks the Master Volume slider
- **THEN** Master Volume becomes `0.0`
- **AND** the slider is rendered at its leftmost position


<!-- Added from refine-gain-master-stop-controls -->
### Requirement: Bottom-Bar START/STOP Control
The system SHALL render a START/STOP button immediately to the left of the bottom-right Settings
toggle.

The START/STOP button SHALL be vertically centered on the same horizontal line as the bottom-bar
icons and SHALL be wider than an icon button so it can be used as a performance control. While no
remembered global stop is active, the START/STOP button SHALL render in the active green style.
After the performer right-presses START/STOP to stop playback, the START/STOP button SHALL render in
the red off style until the remembered loop set is restarted or cleared.

#### Scenario: START/STOP button is aligned beside Settings
- **WHEN** the bottom bar is rendered
- **THEN** the START/STOP button appears directly left of the Settings toggle
- **AND** both controls are vertically centered within the bottom bar

#### Scenario: START/STOP button reflects stopped state
- **GIVEN** no global START/STOP stop state is active
- **WHEN** the bottom bar is rendered
- **THEN** the START/STOP button is green
- **WHEN** the performer presses the right mouse button on START/STOP
- **THEN** the START/STOP button becomes red


<!-- Added from refine-gain-master-stop-controls -->
### Requirement: START/STOP Left Mouse Starts Or Restarts
The system SHALL make left mouse button down on the bottom-bar START/STOP button start or restart
the current target loop set immediately, using the same mouse-down timing style as pad triggering.

If a remembered global stop is active, START/STOP left mouse down SHALL start the remembered pads
together from the beginning of each effective loop. If no remembered global stop is active,
START/STOP left mouse down SHALL restart all currently playing pads from the beginning of their
effective loops. This action SHALL NOT stop playback.

#### Scenario: Left mouse down restarts current active loops
- **GIVEN** pads 1 and 2 are currently active
- **WHEN** the performer holds the left mouse button down on START/STOP
- **THEN** pads 1 and 2 are retriggered from the beginning of their effective loops
- **AND** no stop-all command is sent due to the left mouse button action

#### Scenario: Left mouse down restores remembered loops
- **GIVEN** START/STOP previously remembered pads 1 and 2 from a right-button stop
- **WHEN** the performer presses the left mouse button down on START/STOP
- **THEN** pads 1 and 2 are started together
- **AND** START/STOP renders green


<!-- Added from refine-gain-master-stop-controls -->
### Requirement: Manual Pad Actions Clear Remembered START/STOP Set
The system SHALL clear any remembered START/STOP restore set when the performer manually triggers
or stops a pad after START/STOP has stopped a loop set.

After a manual pad trigger or stop clears the remembered set, START/STOP left mouse down SHALL
restart only the pads that are currently playing. START/STOP right mouse down SHALL remember only
the pads that are currently playing at that moment.

#### Scenario: Manual pad trigger replaces the stopped restore target
- **GIVEN** START/STOP previously remembered pads 1, 2, and 3 from a right-button stop
- **WHEN** the performer manually triggers pad 4
- **AND** the performer presses the left mouse button down on START/STOP
- **THEN** pad 4 is restarted
- **AND** pads 1, 2, and 3 are not started from the old remembered set

#### Scenario: Manual pad stop clears the stopped restore target
- **GIVEN** START/STOP previously remembered pads 1, 2, and 3 from a right-button stop
- **AND** pad 4 is currently playing due to manual pad interaction
- **WHEN** the performer manually stops pad 4
- **AND** the performer presses the left mouse button down on START/STOP
- **THEN** pads 1, 2, and 3 are not started from the old remembered set


<!-- Added from refine-gain-master-stop-controls -->
### Requirement: START/STOP Right Mouse Stops
The system SHALL make right mouse button down on the bottom-bar START/STOP button stop the
currently playing loop set immediately and remember exactly the pads that were playing at that
moment.

Right mouse button interaction on START/STOP SHALL never start or restore playback, and SHALL not
wait for mouse-button release before stopping. The remembered set SHALL be session-only state and
SHALL NOT be persisted with the project.

#### Scenario: Right mouse down stops and remembers playing loops
- **GIVEN** pads 1 and 2 are currently playing
- **WHEN** the performer presses the right mouse button down on START/STOP
- **THEN** all active audio is stopped immediately
- **AND** pads 1 and 2 are remembered for restore
- **AND** START/STOP renders red

#### Scenario: Right mouse down never starts remembered loops
- **GIVEN** START/STOP previously remembered pads 1 and 2 from a right-button stop
- **WHEN** the performer presses the right mouse button down on START/STOP again
- **THEN** no remembered pad is started due to the right mouse button action
- **AND** START/STOP remains in the stopped state


<!-- Added from refine-gain-master-stop-controls -->
### Requirement: START/STOP Momentary Output Mute
The system SHALL make a mouse-wheel button hold on the bottom-bar START/STOP button temporarily
mute the audio engine output without changing the persisted Master Volume value.

While the mouse-wheel button is held from START/STOP, the audio-engine output volume SHALL be set
to `0.0`. When the mouse-wheel button is released, the audio-engine output volume SHALL be restored
from the current persisted Master Volume value. The visible Master Volume slider SHALL NOT move due
to this momentary mute. This mute SHALL be implemented as bounded UI/controller work using the
existing audio parameter path and SHALL NOT add disk I/O, Python/GIL access, logging, blocking work,
heavy allocation, neural inference, or any new work to the Rust audio callback.

#### Scenario: Mouse-wheel holding START/STOP mutes output without moving Master Volume
- **GIVEN** Master Volume is `0.7`
- **WHEN** the performer presses and holds the mouse-wheel button on START/STOP
- **THEN** the audio engine receives output volume `0.0`
- **AND** the persisted Master Volume remains `0.7`
- **WHEN** the performer releases the mouse-wheel button
- **THEN** the audio engine receives output volume `0.7`
- **AND** the persisted Master Volume remains `0.7`


<!-- Added from rework-pad-gain-trim -->
### Requirement: Selected-pad Gain/Trim display layout
The system SHALL render the selected-pad Gain/Trim value directly below the Gain/Trim control in
the left sidebar.

The Gain/Trim value display SHALL use dB formatting with one decimal place and a leading `+` sign
for positive values. The horizontal display and meter SHALL be approximately the same width as the
Gain/Trim control. The UI SHALL render vertical spacing between the Gain/Trim display/meter and
the three Low/Mid/High EQ controls so the controls do not appear cramped. The Gain/Trim value SHALL
NOT be rendered as a percent value inside performance pad buttons. Performance pad buttons SHALL
continue to render loaded-pad BPM/key metadata in the existing top-right pad overlay when that
metadata is available.

#### Scenario: Gain value appears below control
- **GIVEN** pad `id` is selected
- **AND** its Gain/Trim is `+3.5 dB`
- **WHEN** the left sidebar is rendered
- **THEN** the Gain/Trim control is visible
- **AND** a horizontal display directly below it shows `+3.5 dB`
- **AND** the Low, Mid, and High EQ controls appear below that display with visible spacing

#### Scenario: Gain display uses signed dB format
- **WHEN** Gain/Trim is `0.0 dB`
- **THEN** the display shows `0.0 dB`
- **WHEN** Gain/Trim is `+1.5 dB`
- **THEN** the display shows `+1.5 dB`
- **WHEN** Gain/Trim is `-3.0 dB`
- **THEN** the display shows `-3.0 dB`

#### Scenario: Loaded pad keeps BPM and key metadata
- **GIVEN** a loaded pad has BPM and key metadata
- **WHEN** the performance pad grid is rendered
- **THEN** the pad button renders a top-right metadata string such as `94.0 D#`
- **AND** the selected-pad Gain/Trim value is not rendered inside the pad button

