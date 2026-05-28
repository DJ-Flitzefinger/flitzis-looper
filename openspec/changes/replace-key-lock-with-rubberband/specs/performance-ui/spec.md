## MODIFIED Requirements

### Requirement: BPM Lock And Key Lock Controls Affect Playback State
The system SHALL provide BPM lock and Key Lock controls whose visual state reflects the current mode and whose activation changes the corresponding mode.

Key Lock controls SHALL represent the global Rubber Band backed Key Lock mode. The performer-facing Settings UI SHALL NOT expose obsolete custom delay-line tuning controls once the Rubber Band backend is active. Any remaining Key Lock settings SHALL be backend-agnostic controls that can be validated and applied without callback-unsafe work.

#### Scenario: Lock buttons reflect current state
- **GIVEN** Key Lock is disabled
- **WHEN** the performance view is rendered
- **THEN** the Key Lock control is visually indicated as disabled
- **WHEN** the performer enables Key Lock
- **THEN** the Key Lock control is visually indicated as enabled

#### Scenario: Obsolete delay-line settings are absent
- **GIVEN** the Rubber Band Key Lock backend is active
- **WHEN** the performer opens Settings
- **THEN** the UI does not show custom delay-line minimum, range, head count, interpolation, window, smoothing, or output-gain controls for the removed backend
- **AND** enabling or disabling Key Lock remains available from the performance controls
