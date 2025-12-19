## ADDED Requirements

### Requirement: Performance Pad Grid Layout
The system SHALL render a 6×6 pad grid (36 pads) in the primary content window.

#### Scenario: Grid renders on startup
- **WHEN** the UI is started
- **THEN** 36 pad controls are visible
- **AND** the pads are arranged as 6 columns by 6 rows

#### Scenario: Pads are uniquely identifiable
- **WHEN** the UI is started
- **THEN** each pad is labeled with its pad number from 1 through 36

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
- **AND** bank selector controls use distinct colors for active vs inactive (e.g., `#ffaa00` vs `#cc7700`)

### Requirement: Stable UI Identifiers
The system SHALL assign deterministic item tags to each pad and bank control to enable programmatic updates and event binding.

#### Scenario: Pad tags are stable and enumerable
- **WHEN** the UI is started
- **THEN** pads exist with tags `pad_btn_01` through `pad_btn_36`

#### Scenario: Bank tags are stable and enumerable
- **WHEN** the UI is started
- **THEN** bank controls exist with tags `bank_btn_1` through `bank_btn_6`
