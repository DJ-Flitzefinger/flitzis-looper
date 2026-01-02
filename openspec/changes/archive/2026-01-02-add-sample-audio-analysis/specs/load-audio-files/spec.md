## ADDED Requirements
### Requirement: Store Analysis Results In App State
The system SHALL store detected BPM, key, and beat grid for each loaded sample slot in application state intended for persistence, so that results do not need to be recalculated on restart.

The persisted beat grid SHALL use a reduced representation consisting of beat times and downbeat times (in seconds). This representation is sufficient for planned waveform overlays, onset suggestion, and beat alignment.

#### Scenario: Load stores analysis results
- **WHEN** a sample is loaded successfully into slot `id`
- **THEN** the system stores the detected BPM and key for `id`
- **AND** the system stores the detected beat grid for `id`

#### Scenario: Unload clears analysis results
- **GIVEN** a sample is loaded into slot `id` and analysis results exist
- **WHEN** the sample is unloaded from slot `id`
- **THEN** analysis results for `id` are cleared

#### Scenario: Replacing a sample replaces analysis results
- **GIVEN** a sample is loaded into slot `id` and analysis results exist
- **WHEN** a different sample is loaded into slot `id` successfully
- **THEN** analysis results for `id` correspond to the newly loaded sample
