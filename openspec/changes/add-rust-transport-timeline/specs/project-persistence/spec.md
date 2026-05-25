## ADDED Requirements

### Requirement: Persist Trigger Quantization Settings
The system SHALL persist trigger quantization as a separate enabled flag and musical grid step.

The persisted enabled flag SHALL default to `false` for new projects. The persisted grid step
SHALL default to `1_64` and SHALL be constrained to `1_64`, `1_32`, or `1_16`.

When older project files contain the legacy `trigger_quantization` field, the loader SHALL
migrate `immediate`, `disabled`, or `off` to disabled quantization and SHALL migrate legacy
beat/bar modes to the supported compatibility `1_16` grid step.

#### Scenario: New projects store disabled quantization with default grid
- **WHEN** a new project state is created
- **THEN** `trigger_quantization_enabled` is `false`
- **AND** `trigger_quantization_step` is `1_64`

#### Scenario: Legacy next-beat mode migrates to enabled supported grid
- **GIVEN** an older project contains `trigger_quantization = "next_beat"`
- **WHEN** the project is loaded
- **THEN** `trigger_quantization_enabled` is `true`
- **AND** `trigger_quantization_step` is `1_16`

#### Scenario: Legacy immediate mode migrates to disabled quantization
- **GIVEN** an older project contains `trigger_quantization = "immediate"`
- **WHEN** the project is loaded
- **THEN** `trigger_quantization_enabled` is `false`
- **AND** `trigger_quantization_step` remains the default `1_64`
