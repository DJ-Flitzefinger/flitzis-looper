## ADDED Requirements

### Requirement: Per-pad background completions use current request identity
The system SHALL attach an accepted pad request identity to load, analysis, and per-pad background task events, and SHALL apply completion results only when that identity still matches current pad intent.

The identity SHALL be invalidated when the pad is unloaded, when a replacement load is accepted for the same pad, or when controller state otherwise clears the source that the background task was created for.

#### Scenario: Replaced load completion is ignored
- **GIVEN** load request A is accepted for pad 1
- **AND** replacement load request B is accepted for pad 1 before request A completes
- **WHEN** request A later emits progress, success, error, analysis, or publication results
- **THEN** the system ignores request A for pad-state mutation
- **AND** request A does not replace the Rust sample cache or publish `LoadSample` into the audio command path
- **AND** request B remains the current pad intent

#### Scenario: Unloaded load completion is ignored
- **GIVEN** a load request is in progress for pad 2
- **WHEN** the performer unloads pad 2 before the request completes
- **AND** the stale load later emits a success or error
- **THEN** pad 2 remains unloaded
- **AND** stale progress, error, cached path, duration, and analysis data are not applied to project or session state

#### Scenario: Manual analysis completion is source-guarded
- **GIVEN** manual analysis is running for pad 3 and source X
- **WHEN** pad 3 is unloaded or replaced with source Y before analysis completes
- **AND** the analysis task for source X later succeeds
- **THEN** the system ignores the source X analysis result
- **AND** source Y analysis state is not overwritten
