## MODIFIED Requirements

### Requirement: Waveform editor window for selected pad
The system SHALL provide the selected loaded pad's waveform editor in the Looper
center surface, with the authoritative Adjust Loop open/toggle action and the
labeled toolbar close action specified below.

The editor SHALL replace the performance surface while open and SHALL NOT open a
separate ImGui/platform window or require a window-title X to close.

#### Scenario: Opening the waveform editor
- **GIVEN** a loaded pad is selected and the editor is closed
- **WHEN** the performer activates Adjust Loop
- **THEN** the editor opens through the existing authoritative toggle in the center surface
- **AND** opening the editor changes no transport, loop markers or grid timing
- **AND** sidebar and mapped keyboard/MIDI Adjust use the same view-release authority

#### Scenario: Closing the waveform editor
- **GIVEN** a loaded pad's editor is visible
- **WHEN** the performer activates CLOSE LOOP EDITOR or that pad's Adjust Loop action
- **THEN** the same close/view-release authority closes the editor
- **AND** transport, loop markers and grid timing remain unchanged

### Requirement: Waveform editor renders only in-frame with toolbar close
The system SHALL render the waveform editor only in the Looper center surface
and provide a clearly visible `CLOSE LOOP EDITOR` button immediately to the right
of Grid Offset in its toolbar.

This E11-18 contract supersedes the earlier icon-only X at the far right. The
button SHALL invoke the identical close action used when Adjust Loop closes an
open editor, including release of its current view. The editor SHALL resize with
the main window without a floating presentation, title bar, maximize/restore
control or in-frame/floating toggle.
Toolbar hit targets SHALL remain at least 32 logical pixels on both axes and at
least 1.5 times current ImGui frame height. The complete label SHALL remain visible
at supported window sizes and font/DPI scales, using layout wrapping if needed.

#### Scenario: In-frame editor layout close control and target sizes retain the complete contract
- **WHEN** the editor is opened, rendered, resized or its toolbar controls are used
- **THEN** it replaces the center performance surface and resizes with the main window
- **AND** it has no separate window, title bar, floating presentation, maximize/restore or floating-mode toggle
- **AND** its complete CLOSE LOOP EDITOR label follows Grid Offset, wrapping as needed
- **AND** toolbar hit targets retain at least 32 logical pixels per axis and 1.5 times current ImGui frame height

#### Scenario: Waveform editor opens in-frame
- **GIVEN** the waveform editor is closed and a loaded pad is selected
- **WHEN** the performer activates Adjust Loop
- **THEN** the editor replaces the center performance surface without a separate window

#### Scenario: Toolbar close button closes the editor
- **GIVEN** the waveform editor is visible
- **WHEN** the performer activates the full-text CLOSE LOOP EDITOR button
- **THEN** the shared close/view-release authority restores the center performance surface
- **AND** playback, loop and grid timing remain unchanged

#### Scenario: Toolbar hit targets are easier to press
- **GIVEN** the waveform editor is visible
- **WHEN** the toolbar is rendered at ordinary/narrow or scaled supported sizes
- **THEN** Play, Pause, view-jump, bar-step, ALL, Grid Offset and close expose full usable hit targets
- **AND** each target retains at least 32 logical pixels per axis and 1.5 times current ImGui frame height

#### Scenario: Visible close follows Grid Offset
- **GIVEN** the editor is open at an ordinary or narrow supported window size
- **WHEN** its toolbar is rendered or scaled
- **THEN** the visible CLOSE LOOP EDITOR control follows Grid Offset in toolbar order
- **AND** its full label and required hit target are usable
- **WHEN** the performer activates it once
- **THEN** the same close path returns the center surface to the performance view
- **AND** playback, loop/grid coordinates and other pad state are unchanged

## ADDED Requirements

### Requirement: Zoom maintains one waveform and upper number row (E11-04)
The system SHALL render at most one waveform representation and at most one
visible upper musical-number-row band, anchored and clipped to the current
ImPlot rectangle, without vertical displacement or flicker while zooming.
Valid data and visible label candidates SHALL each produce exactly one matching
representation. Readiness/errors and Retry/Cancel SHALL retain reserved space
above the plot; rendering SHALL remain bounded and use the current source/view.

#### Scenario: Repeated zoom crosses representation and readiness changes
- **GIVEN** the productive editor includes its full toolbar and labels
- **WHEN** repeated zoom in/out crosses raw/envelope and asynchronous readiness states
- **THEN** every request, pending, ready, error and settling frame has stable plot Y/height and upper-label distance from its actual plot edge
- **AND** matching valid data produces exactly one waveform branch, otherwise no invented waveform is drawn
- **AND** visible major/Loop-1 candidates produce exactly one visible number band, otherwise no invented labels are drawn
- **AND** plot/label geometry and clip commands do not create a displaced duplicate
- **AND** readiness/errors and Retry/Cancel remain above the plot without scrollbar feedback
- **AND** playback overlays and raw/envelope crossover preserve current source/view coordinates
- **AND** stationary settled frames stop changing the view cache query

#### Scenario: Real wheel and toolbar gestures drive the tested view
- **GIVEN** the complete productive editor is drawn with initialized runtime fonts and its center child/toolbar
- **WHEN** real mouse-wheel input over the plot or Reset/Zoom-to-Loop toolbar input changes its view
- **THEN** actual X limits change and all transition frames retain the required vertical geometry and single visible clipped representations
- **AND** draw-command clip/index/vertex and glyph geometry establish visible bands rather than treating raw out-of-clip vertices alone as a clipping defect

#### Scenario: Resize font and DPI preserve geometry
- **GIVEN** ordinary and narrow supported windows at 100%, 125%, 150% and 200% font/display scaling
- **WHEN** the performer zooms, resizes or replaces the source
- **THEN** waveform and labels follow the actual scaled plot rectangle without a second row
- **AND** legitimate view/source changes request matching cache data
- **AND** hardware-free draw checks and final Human visual acceptance are recorded separately
