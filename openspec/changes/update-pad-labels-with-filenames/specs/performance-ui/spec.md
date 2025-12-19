## MODIFIED Requirements

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

### Requirement: Legacy-Inspired Theme
The system SHALL apply a legacy-inspired theme for the performance UI using a dark background and high-contrast buttons.

#### Scenario: Theme uses legacy palette defaults
- **WHEN** the UI is started
- **THEN** the performance UI background uses `#1e1e1e` (or an equivalent dark color)
- **AND** pad controls use `#3a3a3a` (or an equivalent inactive-pad color)
- **AND** active pad controls use `#2ecc71` (legacy `COLOR_BTN_ACTIVE`, or an equivalent active-pad color)
- **AND** active pad label text uses `#000000` (or an equivalent active-text color)
- **AND** bank selector controls use distinct colors for active vs inactive (e.g., `#ffaa00` vs `#cc7700`)
