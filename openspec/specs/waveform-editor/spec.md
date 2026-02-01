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

### Requirement: Waveform editor displays a single musical grid aligned to loop snapping
The waveform editor SHALL render a SINGLE musical time grid overlay.

This grid SHALL be aligned to the same musical 1/64-note grid concept used for loop marker snapping (see `loop-region`).
Alignment means:
- The musical grid uses the same effective BPM.
- The musical grid uses the same anchor concept.
- All rendered grid lines fall on 1/64-note grid points derived from that BPM + anchor.

The waveform editor MUST NOT render the existing non-musical grid concurrently with the musical grid (no double/overlapping grid).

**Anchor and BPM**
- The effective BPM used for grid rendering SHALL be the same effective BPM used for musical snapping: manual BPM override first, else analysis BPM.
- `beat_sec = 60 / effective_bpm`
- The grid MUST be anchored at `grid_anchor_sec`, defined as:
  - `grid_anchor_sec = default_onset_sec + grid_offset_sec`
  - `default_onset_sec` follows "Default loop region uses analysis downbeat onset" in `loop-region`.
  - Until a grid offset setting exists, `grid_offset_sec = 0`.
- A 1/64-note interval MUST be defined as 1/16 of a beat:
  - `grid_64th_sec = beat_sec / 16`

If no effective BPM is available, the waveform editor SHALL NOT render the musical grid.

**Zoom-dependent visible subdivision selection**
At any zoom level where an effective BPM is available, the waveform editor SHALL choose a visible subdivision step (minor line spacing) from the following set:
- `4 bars` (16 beats)
- `1 bar` (4 beats)
- `1 beat`
- `1/2 beat`
- `1/4 beat`
- `1/8 beat`
- `1/16 beat` (1/64-note)
- `1/32`
- `1/64`

Note: `1/32` and `1/64` are shorthand for 1/32-note and 1/64-note subdivisions in 4/4, and MAY be treated as aliases of `1/8 beat` and `1/16 beat` respectively.

**Readability constraint**
- Let `minor_step_sec` be the time interval for a candidate subdivision (computed from `beat_sec`, using 4 beats per bar).
- Let `minor_step_px` be the horizontal pixel distance between adjacent minor grid lines at the current zoom level.
- The editor SHALL choose the finest (smallest `minor_step_sec`) candidate such that `minor_step_px >= 12 px`.
- If no candidate satisfies the constraint, the editor SHALL fall back to `4 bars`.

**Grid line placement and styling**
- Minor grid lines SHALL be drawn at times `t = grid_anchor_sec + n * minor_step_sec` for integer `n`.
- Major grid lines SHALL be drawn stronger than minor grid lines.
- Major emphasis rules SHALL be:
  - When the minor step is `1 bar`, every `4 bars` line is major.
  - When the minor step is `1 beat`, every `1 bar` line is major.
  - When the minor step is finer than `1 beat` (i.e., `1/2 beat` or smaller), every `1 beat` line is major.
- At maximum zoom, the grid MUST be able to show 1/64-note (1/16 beat) minor lines when the readability constraint permits.

#### Scenario: Default zoom shows bars only with stronger 4-bar lines
- **GIVEN** an effective BPM is available
- **AND** the current zoom level yields `minor_step_px < 12 px` for a `1 beat` grid
- **AND** the current zoom level yields `minor_step_px >= 12 px` for a `1 bar` grid
- **WHEN** the waveform editor renders the grid
- **THEN** it selects `1 bar` as the visible subdivision
- **AND** it renders bar-aligned minor lines
- **AND** every 4th bar line is drawn stronger than the other bar lines
- **AND** no additional non-musical grid is rendered

#### Scenario: Medium zoom shows beats with stronger bar lines
- **GIVEN** an effective BPM is available
- **AND** the current zoom level yields `minor_step_px < 12 px` for a `1/2 beat` grid
- **AND** the current zoom level yields `minor_step_px >= 12 px` for a `1 beat` grid
- **WHEN** the waveform editor renders the grid
- **THEN** it selects `1 beat` as the visible subdivision
- **AND** it renders beat-aligned minor lines
- **AND** bar lines are drawn stronger than beat lines

#### Scenario: Close zoom shows 1/16-note lines with stronger beat lines
- **GIVEN** an effective BPM is available
- **AND** the current zoom level yields `minor_step_px < 12 px` for a `1/8 beat` grid
- **AND** the current zoom level yields `minor_step_px >= 12 px` for a `1/4 beat` grid
- **WHEN** the waveform editor renders the grid
- **THEN** it selects `1/4 beat` as the visible subdivision
- **AND** it renders 1/16-note minor lines
- **AND** beat lines are drawn stronger than 1/16-note lines

#### Scenario: Extreme zoom shows 1/64-note lines
- **GIVEN** an effective BPM is available
- **AND** the current zoom level yields `minor_step_px >= 12 px` for a `1/16 beat` grid
- **WHEN** the waveform editor renders the grid
- **THEN** it selects `1/16 beat` (1/64-note) as the visible subdivision
- **AND** it renders 1/64-note minor lines

