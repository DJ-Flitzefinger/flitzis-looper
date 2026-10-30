## ADDED Requirements

### Requirement: Initial scalar grid base survives project restoration
The system SHALL persist an optional finite nonnegative source-time base for each
pad, validate the fixed pad count, and preserve it through save/restore,
same-source reload, reanalysis and manual BPM or loop edits. Missing legacy fields
SHALL default to no explicit base. Unload/new assignment SHALL clear the old base.

#### Scenario: Base restores without musical analysis
- **GIVEN** a saved explicit base, no analysis or BPM and manual offset zero
- **WHEN** the project source is restored
- **THEN** that base is retained and published as native timing metadata
- **AND** saved loop intent is preserved

#### Scenario: Legacy project keeps its original grid
- **GIVEN** a project without the base field
- **WHEN** it is restored
- **THEN** analysis fallback plus signed offset determines its grid
- **AND** automatic activity placement does not run again
