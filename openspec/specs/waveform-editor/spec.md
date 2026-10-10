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
The waveform editor SHALL provide control buttons in an upper area above the waveform display.

#### Scenario: Transport toolbar layout and press behavior retain the complete contract
- **WHEN** the waveform editor toolbar is rendered or its transport and navigation controls are pressed
- **THEN** the following complete normative contract applies:

The waveform editor SHALL provide control buttons in an upper area above the waveform display.

The toolbar SHALL start with five icon-only buttons (no text labels), ordered left-to-right as follows:

- Pause: icon is two vertical bars; pauses loop playback of the selected pad if it is currently playing
- Play: icon is a right-pointing triangle; starts loop playback of the selected pad AND ALWAYS restarts from the loop start each time it is pressed
- Stop: icon is a square; stops loop playback of the selected pad AND resets the pad playhead to the loop start immediately
- View-Jump-Start: icon is a left-pointing triangle; moves ONLY the waveform VIEW to the start of the full track for the selected pad; MUST NOT change playback state
- View-Jump-End: icon is a right-pointing triangle; moves ONLY the waveform VIEW to the end of the full track for the selected pad; MUST NOT change playback state

All five buttons SHALL behave as triggers:

- The action SHALL execute on press (mouse-down), not on mouse release.

All other, already-existing waveform editor toolbar controls SHALL remain unchanged and SHALL be positioned to the right of these five buttons.
The previously existing Play/Pause UI control in the waveform editor toolbar SHALL be removed from the UI in favor of the buttons above (reusing existing logic as needed).

#### Scenario: Play trigger restarts from loop start on press

- **GIVEN** Pad A is selected
- **AND** Pad A is currently playing
- **WHEN** the performer presses Play in the waveform editor (mouse-down)
- **THEN** Pad A playback continues but restarts from the loop start immediately

#### Scenario: Stop stops playback and resets to loop start on press

- **GIVEN** Pad A is selected
- **AND** Pad A is currently playing
- **WHEN** the performer presses Stop in the waveform editor (mouse-down)
- **THEN** Pad A playback stops immediately
- **AND** Pad A playhead is reset to the loop start immediately

#### Scenario: Playback controls affect only the selected pad

- **GIVEN** Pad A is selected
- **AND** another pad (Pad B) is active
- **WHEN** the performer presses Play, Pause, or Stop in the waveform editor (mouse-down)
- **THEN** only Pad A playback state changes
- **AND** Pad B playback is not stopped by this action

#### Scenario: View-Jump controls do not affect playback

- **GIVEN** Pad A is selected
- **AND** Pad A is currently playing
- **WHEN** the performer presses View-Jump-Start or View-Jump-End (mouse-down)
- **THEN** only the waveform view scroll/pan position changes for Pad A
- **AND** Pad A playback state does not change
- **AND** other pads’ playback is not stopped by this action

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
- **THEN** the resulting marker time corresponds to an integer sample index at the loaded sample rate

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

#### Scenario: Grid Offset placement persistence and interaction retain the complete contract
- **WHEN** the per-pad Grid Offset control is rendered, initialized or dragged
- **THEN** the following complete normative contract applies:

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

#### Scenario: Musical grid alignment readability and subdivision retain the complete contract
- **WHEN** the waveform editor grid is rendered at any zoom level with or without an effective BPM
- **THEN** the following complete normative contract applies:

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


<!-- Added from add-rust-transport-timeline -->
### Requirement: Waveform Editor Shows A Zero-Amplitude Reference
The waveform editor SHALL render a horizontal zero-amplitude reference line across the waveform
plot.

The zero-amplitude line SHALL remain visible alongside the waveform, loop region, playhead, and
musical grid overlays without changing playback, loop marker, or audio-thread behavior.

#### Scenario: Zero line is visible in the waveform plot
- **GIVEN** the waveform editor is open for a loaded pad
- **WHEN** the waveform plot is rendered
- **THEN** a horizontal line is drawn at amplitude `0.0`
- **AND** the line spans the currently visible time range


<!-- Added from add-rust-transport-timeline -->
### Requirement: Waveform Grid Shares The Trigger Quantization Basis
The waveform editor SHALL render its musical grid and loop snapping on the same 1/64-note unit
basis used by trigger quantization and Rust pad timing metadata.

#### Scenario: Grid basis and timing edits retain source-side alignment
- **WHEN** the waveform editor grid is rendered or its pad timing or playback controls change
- **THEN** the following complete normative contract applies:

The waveform editor SHALL render its musical grid and loop snapping on the same 1/64-note unit
basis used by trigger quantization and Rust pad timing metadata.

The finest loop editor musical grid line spacing SHALL be one sixteenth of a beat in 4/4. This is
the same subdivision exposed as the minimum `1/64` trigger quantization grid step, while the
default trigger quantization Settings value remains `1/16`.

The waveform editor grid anchor SHALL be the same per-pad timing anchor published to Rust for
pad timing metadata. Adjusting the per-pad Grid Offset SHALL update this published timing anchor
so the visible loop-editor grid and Rust pad timing metadata stay aligned.

The waveform editor grid anchor SHALL remain stable when other pads are started, stopped,
paused, retriggered, or unloaded. Toggling trigger quantization, changing pitch/speed, enabling
BPM lock, or enabling key lock SHALL NOT move the source-side loop editor grid unless the
performer explicitly edits the pad's loop/grid settings.

#### Scenario: Finest loop editor line spacing matches minimum quantization
- **GIVEN** an effective BPM is available
- **AND** the waveform editor is zoomed far enough for the finest musical grid to be readable
- **WHEN** the waveform editor renders the musical grid
- **THEN** adjacent finest grid lines are spaced one sixteenth of a beat apart
- **AND** the `1/64` trigger quantization grid uses the same subdivision interval

#### Scenario: Grid offset updates Rust pad timing metadata
- **GIVEN** a loaded pad has an effective BPM
- **WHEN** the performer adjusts the waveform editor Grid Offset
- **THEN** the waveform editor grid lines move by that sample offset
- **AND** the control layer publishes the shifted grid anchor as the pad timing metadata used by
  Rust playback timing

#### Scenario: Other pad playback does not move the editor grid
- **GIVEN** the waveform editor is open for pad 2
- **AND** pad 2 has a visible musical grid
- **WHEN** pad 1 starts, stops, or is retriggered
- **THEN** pad 2's waveform editor grid lines remain at the same source-side times

#### Scenario: Quantize toggle does not move the editor grid
- **GIVEN** the waveform editor is open for a loaded pad
- **WHEN** trigger quantization is enabled or disabled
- **THEN** the pad's waveform editor grid anchor remains unchanged


<!-- Added from repair-multi-loop-bpm-sync -->
### Requirement: Loop Editor Source Grid Remains Stable During Playback Sync Changes
The system SHALL keep the Loop Editor source-side grid anchor and snapped loop markers stable when playback sync state changes.

#### Scenario: Playback sync changes preserve every source-domain editing anchor
- **WHEN** playback sync, transport, pitch, speed, locks, quantization or another pad's playback changes
- **THEN** the following complete normative contract applies:

The system SHALL keep the Loop Editor source-side grid anchor and snapped loop markers stable when playback sync state changes.

Changing global Pitch/Speed, enabling or disabling BPM Lock, recomputing master BPM, enabling or disabling Key Lock, toggling trigger quantization, changing the trigger quantization step, or starting/stopping/retriggering another pad SHALL NOT move a pad's Loop Editor Grid Offset anchor, snapped loop start, snapped loop end, or visible source-side grid lines unless the performer edits that pad's loop or grid settings.

The Loop Editor grid SHALL remain a source-domain editing grid. The Rust transport timeline and trigger quantization grid MAY share the same 1/64-note unit basis, but they SHALL NOT reinterpret or move the source-side Loop Editor grid.

#### Scenario: Snapped loop start stays on the shifted grid at 1.5x
- **GIVEN** a pad has analysis downbeat metadata
- **AND** the performer sets a non-zero Grid Offset
- **AND** auto-loop snapping stores the loop start on the shifted 1/64-note source grid
- **WHEN** global Pitch/Speed changes to `1.5x`
- **AND** BPM Lock and Key Lock are toggled
- **THEN** the stored loop start remains at the same source time
- **AND** the visible Loop Editor grid anchor remains at the same source time

#### Scenario: Other pad playback does not move the editor grid
- **GIVEN** the Loop Editor is open for pad 1
- **AND** pad 1 has a shifted source-side grid anchor and snapped loop start
- **WHEN** pad 2 starts, stops, or is retriggered
- **THEN** pad 1's grid anchor and snapped loop start remain unchanged

#### Scenario: Trigger quantization does not redefine the source grid
- **GIVEN** a pad has a visible Loop Editor musical grid
- **WHEN** trigger quantization is enabled, disabled, or changed between supported grid steps
- **THEN** the pad's Loop Editor source grid remains unchanged
- **AND** future triggers still use the Rust transport grid only to choose output start time


<!-- Added from rework-waveform-loop-editor -->
### Requirement: Waveform editor provides bounded bar stepping controls
The waveform editor SHALL provide per-pad auto-loop bar controls that support bounded musical bar
stepping for the selected pad.

#### Scenario: Bar-step gestures retain sequence increments and track bounds
- **WHEN** the selected pad's auto-loop bar decrement or increment control receives left or right mouse-down
- **THEN** the following complete normative contract applies:

The waveform editor SHALL provide per-pad auto-loop bar controls that support bounded musical bar
stepping for the selected pad.

Left mouse down on the decrement/increment arrows SHALL move to the previous/next value in the
sequence `0.5, 1, 2, 4, 8, 16, 32, ...`.

Right mouse down on the decrement/increment arrows SHALL subtract or add exactly `1.0` bar.

The controls SHALL reject changes below `0.5` bars and changes above the maximum loop length that
fits from the current loop start to the loaded track duration at the effective BPM.

#### Scenario: Left-click arrows follow musical powers of two
- **GIVEN** the selected pad is loaded
- **AND** auto-loop is enabled
- **AND** the effective BPM and loaded track duration allow at least 16 bars from the current loop
  start
- **AND** the current bar count is 8
- **WHEN** the performer left-clicks the increment arrow
- **THEN** the bar count becomes 16
- **WHEN** the performer left-clicks the decrement arrow
- **THEN** the bar count becomes 8

#### Scenario: Right-click arrows change by exactly one bar
- **GIVEN** the selected pad is loaded
- **AND** the current bar count is 8
- **WHEN** the performer right-clicks the decrement arrow
- **THEN** the bar count becomes 7
- **WHEN** the performer right-clicks the increment arrow
- **THEN** the bar count becomes 8

#### Scenario: Bar changes beyond fit bounds are no-ops
- **GIVEN** the selected pad is loaded
- **AND** the effective BPM and loaded track duration allow at most 6 bars from the current loop
  start
- **AND** the current bar count is 6
- **WHEN** the performer activates an increment arrow
- **THEN** the stored bar count remains 6
- **AND** the loop region sent to the audio engine is unchanged


<!-- Added from rework-waveform-loop-editor -->
### Requirement: Waveform editor supports middle-click playback seek
The waveform editor SHALL seek the selected pad's active or paused voice when the performer presses
the middle mouse button over the waveform plot.

The middle-click seek SHALL use the plot time under the cursor, clamped to the loaded track
duration, and SHALL NOT change loop start, loop end, auto-loop state, bar count, or grid offset.

If the selected pad has no active or paused voice, the seek SHALL be a no-op and SHALL NOT start
playback.

#### Scenario: Middle-click seek before loop plays into loop
- **GIVEN** Pad A is selected and playing
- **AND** Pad A has an active loop from 10.0 seconds to 18.0 seconds
- **WHEN** the performer middle-clicks the waveform at 5.0 seconds
- **THEN** Pad A seeks to approximately 5.0 seconds
- **AND** playback continues forward until it reaches the loop start
- **AND** subsequent playback loops between 10.0 seconds and 18.0 seconds
- **AND** Pad A loop markers are unchanged

#### Scenario: Middle-click seek inside loop keeps normal wrapping
- **GIVEN** Pad A is selected and playing
- **AND** Pad A has an active loop from 10.0 seconds to 18.0 seconds
- **WHEN** the performer middle-clicks the waveform at 12.0 seconds
- **THEN** Pad A seeks to approximately 12.0 seconds
- **AND** playback wraps from the loop end back to 10.0 seconds

#### Scenario: Middle-click seek after loop plays to track end
- **GIVEN** Pad A is selected and playing
- **AND** Pad A has an active loop from 10.0 seconds to 18.0 seconds
- **AND** the loaded track duration is 30.0 seconds
- **WHEN** the performer middle-clicks the waveform at 22.0 seconds
- **THEN** Pad A seeks to approximately 22.0 seconds
- **AND** playback continues forward until the track end
- **AND** playback then jumps to 10.0 seconds and loops normally

#### Scenario: Middle-click seek does not start a stopped pad
- **GIVEN** Pad A is selected and loaded
- **AND** Pad A is not active and not paused
- **WHEN** the performer middle-clicks the waveform
- **THEN** Pad A does not start playback
- **AND** Pad A loop markers are unchanged


<!-- Added from rework-waveform-loop-editor -->
### Requirement: Waveform editor renders only in-frame with toolbar close
The waveform editor SHALL render only in the Looper center surface and SHALL NOT open as a separate
ImGui or platform window.

#### Scenario: In-frame editor layout close control and target sizes retain the complete contract
- **WHEN** the waveform editor is opened, rendered or resized, or its toolbar controls are used
- **THEN** the following complete normative contract applies:

The waveform editor SHALL render only in the Looper center surface and SHALL NOT open as a separate
ImGui or platform window.

The waveform editor SHALL replace the performance surface while it is open, similar to the
Settings page, and SHALL resize with the Looper main window.

The waveform editor SHALL NOT provide a title bar, floating-window presentation, maximize/restore
control, or in-frame/floating mode toggle.

The waveform editor toolbar SHALL provide an icon-only close `X` button at the far right of the
same horizontal control area that contains the editor transport, view, and loop controls.

Toolbar icon hit targets SHALL be at least 32 logical pixels on both axes and no smaller than
1.5 times the current ImGui frame height.

#### Scenario: Waveform editor opens in-frame
- **GIVEN** the waveform editor is closed
- **WHEN** the performer activates `Adjust Loop` for a loaded selected pad
- **THEN** the editor replaces the Looper center performance surface
- **AND** no separate waveform editor window is opened

#### Scenario: Toolbar close button closes the editor
- **GIVEN** the waveform editor is visible
- **WHEN** the performer activates the toolbar close `X`
- **THEN** the waveform editor closes
- **AND** the Looper center performance surface is visible again

#### Scenario: Toolbar hit targets are easier to press
- **GIVEN** the waveform editor is visible
- **WHEN** the toolbar is rendered
- **THEN** Play, Pause, view-jump, bar-step, `ALL`, grid-offset, and close controls each expose hit
  targets at least 32 logical pixels on both axes
