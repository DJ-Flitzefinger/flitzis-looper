## ADDED Requirements

### Requirement: Persist and migrate dB pad Gain/Trim
The system SHALL persist per-pad Gain/Trim as dB intent and migrate legacy per-pad gain values
without accidentally boosting old projects.

New project defaults SHALL store or derive `0.0 dB` Gain/Trim for every pad. If an older project
contains the legacy `pad_gain` field instead of the dB field, the loader SHALL migrate old unity
values `1.0` and `100` to `0.0 dB`. Legacy values below unity SHALL be converted to dB through
`20 * log10(linear_gain)` and clamped to `-12.0 dB`. Missing Gain/Trim data SHALL default to
`0.0 dB`.

#### Scenario: Missing gain defaults to neutral trim
- **GIVEN** a project file does not contain per-pad Gain/Trim data
- **WHEN** the project is loaded
- **THEN** every pad has Gain/Trim `0.0 dB`

#### Scenario: Legacy unity gain migrates to zero dB
- **GIVEN** a project file contains legacy `pad_gain` value `1.0` for Pad A
- **AND** a project file contains legacy `pad_gain` value `100` for Pad B
- **WHEN** the project is loaded
- **THEN** Pad A has Gain/Trim `0.0 dB`
- **AND** Pad B has Gain/Trim `0.0 dB`

#### Scenario: Legacy reduced gain migrates without boost
- **GIVEN** a project file contains legacy `pad_gain` value `0.5`
- **WHEN** the project is loaded
- **THEN** the pad has Gain/Trim approximately `-6.0 dB`
- **AND** the pad does not load with positive Gain/Trim
