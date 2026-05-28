## ADDED Requirements

### Requirement: Performer Stem Generation Uses Background Tasks
The system SHALL route performer stem-generation requests through the non-real-time per-pad
background-task path.

The request SHALL respect existing loading, analysis, stem-generation, unloading, and playing
pad gates. Progress, success, and failure SHALL be reported through controller/session state so
the UI can render status without blocking.

#### Scenario: Generate stems action starts a background task
- **GIVEN** a loaded pad is stopped and has no conflicting per-pad task
- **WHEN** the performer requests stem generation from the UI
- **THEN** the system schedules a per-pad stem-generation background task
- **AND** UI rendering remains responsive while the task runs

#### Scenario: Conflicting task blocks UI stem generation
- **GIVEN** a pad is currently loading, analyzing, already generating stems, unloading, or playing
- **WHEN** the performer requests stem generation from the UI
- **THEN** the request is rejected or deferred through controller state
- **AND** no stem generation work is run by the audio callback
