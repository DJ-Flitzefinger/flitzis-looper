## MODIFIED Requirements

### Requirement: Waveform editor window for selected pad
The system SHALL provide the selected loaded pad's waveform editor in the Looper
center surface, with the authoritative Adjust Loop open/toggle action and the
labeled toolbar close action specified below.

The editor SHALL replace the performance surface while open and SHALL NOT open a
separate ImGui/platform window or require a window-title X to close.

#### Scenario: Existing toggle and labeled close share editor state
- **GIVEN** a loaded pad is selected
- **WHEN** the performer activates Adjust Loop
- **THEN** the editor opens or closes through the existing authoritative toggle
- **AND** the labeled toolbar close closes that same editor state
- **AND** neither action changes transport, loop markers or grid timing

### Requirement: Waveform editor renders only in-frame with toolbar close
The system SHALL render the waveform editor only in the Looper center surface
and provide a clearly visible `CLOSE LOOP EDITOR` button immediately to the right
of Grid Offset in its toolbar.

This E11-18 contract supersedes the earlier icon-only X at the far right. The
button SHALL invoke the identical close action used when Adjust Loop closes an
open editor. The editor SHALL resize with the main window without a floating
presentation, title bar, maximize/restore control or in-frame/floating toggle.
Toolbar hit targets SHALL remain at least 32 logical pixels on both axes and at
least 1.5 times current ImGui frame height. The complete label SHALL remain visible
at supported window sizes and font/DPI scales, using layout wrapping if needed.

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
The system SHALL render one waveform representation and one upper musical-number
row without vertical displacement, duplicate overlay or flicker while zooming the
waveform editor in and out (E11-04).

The current ImPlot rectangle SHALL anchor labels/overlays and their clipping.
Stable horizontal zoom frames SHALL NOT alternate vertical plot/number-row
geometry, create a scrollbar feedback cycle or draw stale cached geometry over
the new view. Raw/envelope crossover, pending/ready/error projections, playback
overlays, resize, source replacement and relevant font/DPI changes SHALL preserve
that contract with bounded cached render data.

#### Scenario: Repeated zoom crosses representation and readiness changes
- **GIVEN** the productive editor includes its full toolbar and labels
- **WHEN** repeated zoom in/out crosses raw/envelope and asynchronous readiness states
- **THEN** each actual draw frame contains one waveform branch and one upper number-row baseline
- **AND** plot/label geometry and clip commands do not create a displaced duplicate
- **AND** stationary settled frames stop changing the view cache query

#### Scenario: Resize font and DPI preserve geometry
- **GIVEN** ordinary and narrow supported windows at 100%, 125%, 150% and 200% font/display scaling
- **WHEN** the performer zooms, resizes or replaces the source
- **THEN** waveform and labels follow the actual scaled plot rectangle without a second row
- **AND** legitimate view/source changes request matching cache data
- **AND** hardware-free draw checks and final Human visual acceptance are recorded separately
