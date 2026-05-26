## MODIFIED Requirements

### Requirement: UI refactor preserves existing UI semantics
The UI refactor SHALL preserve the observable UI semantics required by existing specs, including
pad label basename formatting, pad loading progress behavior, loaded-pad BPM/key metadata display,
and selected-pad Gain-area metering.

The UI refactor SHALL NOT require rendering the previous vertical right-edge peak meter inside
performance pad buttons; per-pad peak state SHALL remain available for the selected-pad Gain-area
meter and clip indicator.

#### Scenario: Existing UI tests remain valid
- **WHEN** the existing Python test suite is executed
- **THEN** UI context and render-related tests continue to pass

#### Scenario: Pad metering state feeds selected Gain area
- **GIVEN** a selected pad has cached peak telemetry
- **WHEN** the left sidebar is rendered
- **THEN** the selected-pad Gain-area meter can render that cached peak
- **AND** the performance pad button does not need a vertical level meter
