## ADDED Requirements

### Requirement: Waveform editor provides bounded bar stepping controls
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

### Requirement: Waveform editor renders only in-frame with toolbar close
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

## MODIFIED Requirements

### Requirement: Waveform editor window for selected pad
The system SHALL provide the waveform editor for the selected pad as an in-frame Looper center
surface rather than as a separate ImGui window.

The waveform editor SHALL open when the performer activates the "Adjust Loop" action for the
currently selected loaded pad.

The waveform editor SHALL be closable through the toolbar close `X` affordance.

The waveform editor MUST NOT create a separate ImGui or platform window while this in-frame
surface is visible.

#### Scenario: Opening the waveform editor
- **GIVEN** a loaded pad is selected
- **WHEN** the performer activates "Adjust Loop" in the selected-pad sidebar
- **THEN** the waveform editor is visible in the Looper center surface
- **AND** no separate waveform editor window is opened

#### Scenario: Closing the waveform editor
- **GIVEN** the waveform editor is visible
- **WHEN** the performer closes the editor via the toolbar close `X`
- **THEN** the waveform editor is no longer visible
- **AND** the Looper center performance surface is visible again

### Requirement: Waveform editor provides transport and navigation controls
The waveform editor SHALL provide selected-pad transport and view navigation controls in an upper
toolbar above the waveform display.

The toolbar SHALL include icon-only Play and Pause transport buttons for the selected pad. The
toolbar SHALL NOT provide a separate dedicated Stop square; stopping the selected pad is handled by
right mouse down on Play.

Play left mouse down SHALL retrigger the selected pad from the effective loop start immediately
without waiting for mouse release. Play right mouse down SHALL stop the selected pad immediately
without waiting for mouse release.

Pause left mouse down SHALL toggle pause/resume for the selected pad immediately. Pause right mouse
down SHALL pause the selected pad immediately and SHALL resume it on right mouse release only if
that hold action caused the pause.

View-Jump-Start and View-Jump-End SHALL move only the waveform view for the selected pad and SHALL
NOT change playback state.

#### Scenario: Play left mouse down retriggers from loop start
- **GIVEN** Pad A is selected
- **AND** Pad A is currently playing
- **WHEN** the performer presses Play with the left mouse button in the waveform editor
- **THEN** Pad A playback continues but restarts from the effective loop start immediately

#### Scenario: Play right mouse down stops selected pad
- **GIVEN** Pad A is selected
- **AND** Pad A is currently playing
- **WHEN** the performer presses Play with the right mouse button in the waveform editor
- **THEN** Pad A stops immediately
- **AND** no other active pad is stopped by this action

#### Scenario: Pause left mouse down toggles pause and resume
- **GIVEN** Pad A is selected and playing
- **WHEN** the performer presses Pause with the left mouse button
- **THEN** Pad A pauses immediately and keeps its playhead position
- **WHEN** the performer presses Pause with the left mouse button again
- **THEN** Pad A resumes from the paused position

#### Scenario: Pause right hold is momentary
- **GIVEN** Pad A is selected and playing
- **WHEN** the performer presses and holds Pause with the right mouse button
- **THEN** Pad A pauses immediately
- **WHEN** the performer releases the right mouse button
- **THEN** Pad A resumes from the held position

#### Scenario: Pause right hold does not resume a previously paused pad
- **GIVEN** Pad A is selected and already paused
- **WHEN** the performer presses and releases Pause with the right mouse button
- **THEN** Pad A remains paused after release

#### Scenario: Playback controls affect only the selected pad
- **GIVEN** Pad A is selected
- **AND** another pad, Pad B, is active
- **WHEN** the performer uses Play or Pause mouse actions in the waveform editor
- **THEN** only Pad A playback state changes
- **AND** Pad B playback is not stopped by this action

#### Scenario: View-Jump controls do not affect playback
- **GIVEN** Pad A is selected
- **AND** Pad A is currently playing
- **WHEN** the performer presses View-Jump-Start or View-Jump-End
- **THEN** only the waveform view scroll/pan position changes for Pad A
- **AND** Pad A playback state does not change
- **AND** other pads' playback is not stopped by this action

### Requirement: Waveform editor supports mouse interactions
The waveform editor SHALL support waveform display mouse interactions for loop-marker editing,
middle-click seeking, and view navigation.

Mouse wheel up/down SHALL zoom the waveform view in/out. Holding and dragging the middle mouse
button MAY pan the waveform view after the initial middle mouse down seek, but SHALL NOT issue
repeated seek commands during the same hold.

Empty-plot left-click SHALL set the selected pad loop start using the loop-region snapping and
sample-quantization rules. After accepting the new loop start, the editor SHALL retrigger only the
selected pad from the new effective loop start without stopping other active pads.

Empty-plot left-click SHALL NOT set loop end and SHALL NOT perform the middle-click seek behavior.
Right-click loop-end editing SHALL remain available only when auto-loop is off.

#### Scenario: Mouse wheel zooms
- **GIVEN** the waveform editor is visible
- **WHEN** the performer uses the mouse wheel over the waveform
- **THEN** the zoom level changes

#### Scenario: Middle mouse down seeks and middle drag pans
- **GIVEN** the waveform editor is visible
- **AND** the selected pad is playing
- **WHEN** the performer presses the middle mouse button over the waveform
- **THEN** one seek command is sent for the selected pad
- **WHEN** the performer keeps holding the middle mouse button and drags horizontally
- **THEN** the waveform view pans
- **AND** no additional seek commands are sent until a new middle mouse press occurs

#### Scenario: Empty plot left-click sets loop start and retriggers selected pad
- **GIVEN** the waveform editor is visible
- **AND** Pad A is selected and loaded
- **AND** Pad B is active
- **WHEN** the performer left-clicks an empty point in the waveform plot at time T
- **THEN** Pad A loop start becomes approximately T subject to the snapping rules in `loop-region`
- **AND** Pad A playback starts from the new effective loop start
- **AND** Pad B playback is not stopped by this action

#### Scenario: Draggable marker edits loop start
- **GIVEN** the waveform editor is visible
- **WHEN** the performer drags the loop-start marker to time T
- **THEN** the loop start becomes approximately T subject to the snapping rules in `loop-region`

#### Scenario: Right click sets loop end only in manual mode
- **GIVEN** the waveform editor is visible
- **AND** auto-loop is disabled
- **WHEN** the performer right-clicks at time T in the waveform
- **THEN** the loop end becomes approximately T

### Requirement: Waveform editor displays a single musical grid aligned to loop snapping
The waveform editor SHALL render a single musical time grid overlay aligned to the same snapping
grid used for loop marker edits.

The waveform editor MUST NOT render the previous non-musical grid concurrently with the musical
grid.

The effective BPM used for grid rendering SHALL be manual BPM override first, else analysis BPM.
If no effective BPM is available, the waveform editor SHALL NOT render the musical grid.

The grid anchor SHALL be `analysis_grid_onset_sec + grid_offset_sec`. `analysis_grid_onset_sec`
SHALL be `downbeats[0]` when available, else `beats[0]`, else `0.0` seconds. This analysis grid
onset SHALL NOT replace the loaded-track default loop start of `0.0`.

A 1/64-note interval MUST be defined as 1/16 of a beat:

- `beat_sec = 60 / effective_bpm`
- `grid_64th_sec = beat_sec / 16`

The editor SHALL choose the finest visible subdivision whose adjacent minor grid lines are at
least 12 px apart, falling back to a 4-bar subdivision when no finer subdivision is readable.

#### Scenario: Musical grid uses analysis anchor without changing loop default
- **GIVEN** a loaded pad has analysis with `downbeats[0] = 2.0`
- **AND** the loaded pad default loop start is `0.0`
- **WHEN** the waveform editor renders the musical grid
- **THEN** the grid anchor is derived from 2.0 seconds plus grid offset
- **AND** the loop start remains `0.0`
- **AND** no additional non-musical grid is rendered

#### Scenario: Extreme zoom shows 1/64-note lines
- **GIVEN** an effective BPM is available
- **AND** the current zoom level yields at least 12 px between adjacent 1/64-note grid lines
- **WHEN** the waveform editor renders the grid
- **THEN** it selects 1/64-note minor lines
- **AND** stronger major lines remain aligned to beat or bar boundaries
