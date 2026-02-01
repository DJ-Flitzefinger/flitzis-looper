# waveform-editor Specification

## Purpose
To define the waveform editor UI window that allows performers to adjust per-pad loop start/end precisely, using audio-derived onsets and musical bar lengths, without stopping playback.
## Requirements
### Requirement: Waveform editor window for selected pad
The system SHALL provide a waveform editor as a separate ImGui window inside the main application window.

The waveform editor window SHALL open when the performer activates the "Adjust Loop" action for the currently selected pad.

The waveform editor window SHALL be closable via the ImGui window close (X) affordance.

#### Scenario: Opening the waveform editor
- **GIVEN** a pad is selected
- **WHEN** the performer activates "Adjust Loop" in the selected-pad sidebar
- **THEN** the waveform editor window is visible

#### Scenario: Closing the waveform editor
- **GIVEN** the waveform editor window is visible
- **WHEN** the performer closes the window via the ImGui close button
- **THEN** the waveform editor window is no longer visible

### Requirement: Waveform editor renders mono waveform efficiently
The waveform editor SHALL render a mono (single-channel) waveform view for the selected pad.

The waveform editor SHALL use ImPlot (available via `imgui_bundle`) for waveform plotting.

The waveform rendering MUST be performance-friendly and MUST NOT require iterating over all raw samples each frame.

#### Scenario: Rendering uses a cached representation
- **GIVEN** a pad has loaded audio
- **WHEN** the waveform editor is rendered repeatedly during playback
- **THEN** the UI remains responsive
- **AND** waveform rendering does not require per-frame full-buffer traversal

#### Scenario: Extreme zoom shows individual samples
- **GIVEN** a pad has loaded audio
- **WHEN** the performer zooms in until a small time window is visible
- **THEN** individual sample points/segments become visible in the waveform display

### Requirement: Waveform editor provides transport and navigation controls
The waveform editor SHALL provide control buttons in an upper area above the waveform display:
- Play/Pause (start/stop playback of the selected pad)
- Reset (resets loop region to the default; see `loop-region`)
- Zoom in/out
- Pan left/right
- Auto-loop checkbox with bar count and +/- buttons (see `loop-region`)

#### Scenario: Play/Pause affects only the selected pad
- **GIVEN** Pad A is selected
- **AND** another pad (Pad B) is active
- **WHEN** the performer activates Play/Pause in the waveform editor
- **THEN** only Pad A playback state changes
- **AND** Pad B playback is not stopped by this action

### Requirement: Waveform editor supports mouse interactions
The waveform editor SHALL support the following mouse interactions over the waveform display:
- Mouse wheel up/down zooms in/out.
- Holding the middle mouse button pans left/right.
- Left-click selects a custom loop start.
- Right-click selects a custom loop end only when auto-loop is off.

#### Scenario: Mouse wheel zooms
- **GIVEN** the waveform editor is visible
- **WHEN** the performer uses the mouse wheel over the waveform
- **THEN** the zoom level changes

#### Scenario: Middle mouse pans
- **GIVEN** the waveform editor is visible
- **WHEN** the performer drags with the middle mouse button held
- **THEN** the waveform view pans horizontally

#### Scenario: Left click sets loop start
- **GIVEN** the waveform editor is visible
- **WHEN** the performer left-clicks at time T in the waveform
- **THEN** the loop start becomes approximately T (subject to snapping rules in `loop-region`)

#### Scenario: Sample-accurate marker placement at extreme zoom
- **GIVEN** the waveform editor is visible
- **AND** the waveform is zoomed such that individual samples are visible
- **WHEN** the performer sets a loop marker
- **THEN** the resulting marker time corresponds to an integer sample index at the cached WAV sample rate

#### Scenario: Right click sets loop end only in manual mode
- **GIVEN** the waveform editor is visible
- **AND** auto-loop is disabled
- **WHEN** the performer right-clicks at time T in the waveform
- **THEN** the loop end becomes approximately T

### Requirement: Waveform editor shows playhead and loop region
The waveform editor SHALL visualize:
- The current playback position (playhead marker)
- The current loop region

The loop region visualization SHALL use:
- A blue loop-start line
- A red loop-end line
- A light-yellow background fill for the region between markers

#### Scenario: Loop region is visible
- **GIVEN** a pad has an active loop region
- **WHEN** the waveform editor is rendered
- **THEN** the loop region is shaded
- **AND** the start marker is blue
- **AND** the end marker is red

#### Scenario: Playhead marker updates during playback
- **GIVEN** a pad is playing
- **WHEN** the waveform editor is rendered over time
- **THEN** the playhead marker position changes to reflect current playback

### Requirement: Waveform editor provides a per-pad Grid Offset control
The waveform editor SHALL provide a "Grid Offset" knob/control in its toolbar.

The control SHALL be placed to the right of the current right-most control in the toolbar and SHALL be sized consistently with the existing toolbar controls.

The Grid Offset value SHALL be expressed and displayed as a signed integer in samples (`grid_offset_samples`).

The Grid Offset value SHALL be stored per pad. If no stored value exists for a pad (e.g., older projects), `grid_offset_samples` SHALL default to 0.

**Interaction**
- Left-click dragging the control SHALL adjust `grid_offset_samples` in fine steps of 1 sample.
- Right-click dragging the control SHALL adjust `grid_offset_samples` in coarse steps of 10 samples.

#### Scenario: Default grid offset is zero for an uninitialized pad
- **GIVEN** a pad is loaded from a project that does not contain a stored `grid_offset_samples`
- **WHEN** the waveform editor is opened for that pad
- **THEN** the Grid Offset control displays 0 samples

#### Scenario: Dragging adjusts grid offset in fine vs coarse steps
- **GIVEN** the waveform editor is open for a pad
- **WHEN** the performer left-click drags the Grid Offset control
- **THEN** the `grid_offset_samples` value changes in 1-sample steps
- **WHEN** the performer right-click drags the Grid Offset control
- **THEN** the `grid_offset_samples` value changes in 10-sample steps

