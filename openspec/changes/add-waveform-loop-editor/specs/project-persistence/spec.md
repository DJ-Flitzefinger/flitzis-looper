## ADDED Requirements

### Requirement: Persist and restore per-pad loop settings
The system SHALL persist per-pad loop settings (see `loop-region`) as part of `ProjectState` and SHALL restore them on application start.

#### Scenario: Loop settings persist across restarts
- **GIVEN** a pad has customized loop settings
- **WHEN** the application is restarted
- **THEN** the loop settings are restored for that pad
