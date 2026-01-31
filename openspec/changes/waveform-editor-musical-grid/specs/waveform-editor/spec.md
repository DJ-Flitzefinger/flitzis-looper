## ADDED Requirements

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
