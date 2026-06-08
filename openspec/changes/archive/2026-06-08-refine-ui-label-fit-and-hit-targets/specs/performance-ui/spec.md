## ADDED Requirements

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

### Requirement: UI Polish Realtime Boundary
The system SHALL keep these UI presentation refinements outside the realtime audio callback.

#### Scenario: Rendering refinements do not add realtime work
- **WHEN** filename wrapping, bottom-bar alignment, Master Volume width, or Pitch grab sizing is changed
- **THEN** no disk I/O, Python/GIL access, logging, blocking work, heavy allocation, neural inference,
  or new callback work is added to the Rust audio callback
