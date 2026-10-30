## ADDED Requirements


### Requirement: Fresh Defaults Preserve Explicit Saved Choices

Fresh project/settings factories SHALL select the1_16 trigger step while leaving
trigger quantization disabled, FLAC recording at24bit/compression8, and the
existing BS-RoFormer MUSDB18HQ separator. Other defaults SHALL remain unchanged.
An explicit saved step, recording format/precision or separator SHALL remain
authoritative. Applying fresh defaults SHALL NOT rewrite private settings,
recordings, stem artifacts or pending requests, and SHALL NOT invoke inference.

#### Scenario: Fresh state uses the requested choices
- **WHEN** a fresh project is created without saved choices
- **THEN** its trigger step SHALL be1_16 with quantization disabled
- **AND** recording SHALL select FLAC24 with compression8
- **AND** future stem requests SHALL use BS-RoFormer MUSDB18HQ

#### Scenario: Explicit saved preferences survive reopen
- **GIVEN** saved preferences explicitly select1_32, WAV and Demucs
- **WHEN** that project is reopened
- **THEN** those explicit preferences SHALL remain unchanged


<!-- Added from add-stem-performance-controls -->

## MODIFIED Requirements


### Requirement: Persist Trigger Quantization Settings
The system SHALL persist trigger quantization as a separate enabled flag and musical grid step.

The persisted enabled flag SHALL default to `false` for new projects. The persisted grid step
SHALL default to `1_16` and SHALL be constrained to `1_64`, `1_32`, or `1_16`.

When older project files contain the legacy `trigger_quantization` field, the loader SHALL
migrate `immediate`, `disabled`, or `off` to disabled quantization and SHALL migrate legacy
beat/bar modes to the supported compatibility `1_16` grid step.

#### Scenario: New projects store disabled quantization with default grid
- **WHEN** a new project state is created
- **THEN** `trigger_quantization_enabled` is `false`
- **AND** `trigger_quantization_step` is `1_16`

#### Scenario: Legacy next-beat mode migrates to enabled supported grid
- **GIVEN** an older project contains `trigger_quantization = "next_beat"`
- **WHEN** the project is loaded
- **THEN** `trigger_quantization_enabled` is `true`
- **AND** `trigger_quantization_step` is `1_16`

#### Scenario: Legacy immediate mode migrates to disabled quantization
- **GIVEN** an older project contains `trigger_quantization = "immediate"`
- **WHEN** the project is loaded
- **THEN** `trigger_quantization_enabled` is `false`
- **AND** `trigger_quantization_step` remains the default `1_16`
