## MODIFIED Requirements

### Requirement: Stem Availability Indicators
The system SHALL show per-pad stem availability, generation progress, blocked state, and
errors in the performance UI without blocking rendering.

The pad grid MAY use compact indicators for stem availability, generation progress, and errors,
but SHALL NOT render a compact blocked-state indicator. The selected-pad sidebar SHALL provide the
detailed status for the selected pad, including blocked generation reasons. Pad-grid compact
indicators SHALL NOT show hover tooltips or hover status messages. UI rendering SHALL use
controller/session snapshots and SHALL NOT inspect cache directories, decode audio, run
inference, or perform blocking work.

#### Scenario: Selected pad shows available stems
- **GIVEN** the selected pad has loaded audio
- **AND** the pad has a complete current prepared stem cache
- **WHEN** the performance UI is rendered
- **THEN** the selected-pad sidebar indicates that stems are available
- **AND** the pad grid may show a compact stem-available indicator for that pad

#### Scenario: Stem generation progress is visible
- **GIVEN** stem generation is running for the selected pad
- **WHEN** the performance UI is rendered
- **THEN** the selected-pad sidebar shows the current generation stage and progress when available
- **AND** the pad grid may show a compact progress indicator for that pad
- **AND** the UI remains responsive while generation continues outside the audio callback

#### Scenario: Stem generation error preserves pad usability
- **GIVEN** stem generation failed for the selected pad
- **WHEN** the performance UI is rendered
- **THEN** the selected-pad sidebar shows the error outside the audio callback
- **AND** the pad grid may show a compact error indicator for that pad
- **AND** the pad remains playable using full-mix playback

#### Scenario: Blocked stem generation is detailed only in the sidebar
- **GIVEN** a loaded pad is playing and stem generation is blocked for that pad
- **WHEN** the performance UI is rendered
- **THEN** the pad grid does not show a compact blocked indicator for that pad
- **AND** the selected-pad sidebar remains the detailed blocked-status surface

#### Scenario: Pad-grid indicators do not show hover messages
- **GIVEN** the pad grid shows a compact stem status indicator for a pad
- **WHEN** the performer hovers that pad
- **THEN** the UI does not show a stem status tooltip or hover message over the pad
- **AND** the selected-pad sidebar remains the detailed status surface
