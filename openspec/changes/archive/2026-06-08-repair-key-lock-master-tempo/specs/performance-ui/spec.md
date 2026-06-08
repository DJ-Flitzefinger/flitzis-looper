## ADDED Requirements

### Requirement: Manual Key Lock DSP Settings
The system SHALL expose all bounded Key Lock DSP parameters in a dedicated Settings-page block.

The block SHALL allow the performer to edit delay minimum, delay range, head count,
interpolation, window, smoothing step, and output gain within the documented supported ranges. Each
parameter control SHALL display adjacent or immediately following text explaining the performance
or sound tradeoff of higher and lower values, or for enum parameters the tradeoff of each option.
The Settings page SHALL persist the concrete parameter values with the project and publish them to
Rust as bounded control-plane state.

#### Scenario: Settings page lists manual Key Lock parameters
- **GIVEN** the Settings page is open
- **WHEN** the performer inspects Key Lock DSP
- **THEN** controls are available for delay minimum, delay range, head count, interpolation,
  window, smoothing step, and output gain
- **AND** each parameter shows a nearby performance or sound tradeoff hint

#### Scenario: Manual Key Lock parameters persist
- **GIVEN** the performer changes delay range and output gain within the supported ranges
- **WHEN** the project is saved and loaded again
- **THEN** the same concrete Key Lock parameter values are restored
- **AND** Rust receives those concrete parameters when project state is restored
