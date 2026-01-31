## ADDED Requirements

### Requirement: Persist per-pad grid offset samples
Project persistence MUST store and restore `grid_offset_samples` per pad as part of project persistence.

If `grid_offset_samples` is missing when loading older projects, the system SHALL treat it as 0 samples.

#### Scenario: Missing grid offset field loads as zero
- **GIVEN** a project file created before `grid_offset_samples` existed
- **WHEN** the project is loaded
- **THEN** each pad's `grid_offset_samples` is treated as 0

#### Scenario: Stored grid offset is restored per pad
- **GIVEN** a project is saved with `grid_offset_samples = +123` for Pad A
- **AND** the project is saved with `grid_offset_samples = -456` for Pad B
- **WHEN** the project is loaded
- **THEN** Pad A has `grid_offset_samples = +123`
- **AND** Pad B has `grid_offset_samples = -456`
