# background-tasks Specification

## Purpose
To define non-realtime per-pad background operations, progress reporting, and conflict prevention for tasks such as analysis and offline stem generation.
## Requirements
### Requirement: Background Tasks Support Per-Pad Operations
The system SHALL support running non-real-time operations for a pad as background tasks with progress reporting.

Background tasks MAY be part of the sample loading pipeline (automatic) or MAY be triggered manually for an already-loaded pad.

#### Scenario: A pad runs an analysis-only background task
- **GIVEN** a pad has a loaded audio sample
- **WHEN** the user triggers "Analyze audio"
- **THEN** the system schedules an analysis-only background task for that pad
- **AND** the UI receives progress updates for that task

#### Scenario: A pad runs a background task that is not part of loading
- **GIVEN** a pad has a loaded audio sample
- **WHEN** the user triggers a future operation such as "Generate stems"
- **THEN** the system schedules a background task for that pad
- **AND** that task runs without re-running the sample loading pipeline

### Requirement: Background Task Concurrency Rules
The system SHALL prevent conflicting background operations for the same pad.

#### Scenario: Manual tasks are blocked while loading is in progress
- **GIVEN** a pad is currently loading
- **WHEN** the user tries to start a manual background task for that pad
- **THEN** the task is rejected or deferred


<!-- Added from add-offline-stem-cache -->
### Requirement: Stem Generation Runs As A Per-Pad Background Task
The system SHALL run stem generation as a non-real-time per-pad background task with
progress and error reporting.

Stem generation SHALL NOT run as part of the audio callback. It SHALL NOT block UI
rendering while the task is in progress.

#### Scenario: Stem task reports progress
- **GIVEN** a pad has loaded source audio and is not playing
- **WHEN** stem generation starts
- **THEN** the system reports task progress for that pad outside the audio callback
- **AND** the UI remains responsive while generation runs


<!-- Added from add-offline-stem-cache -->
### Requirement: Stem Tasks Respect Per-Pad Concurrency
The system SHALL prevent conflicting per-pad tasks from running concurrently with stem
generation.

A stem generation request SHALL be rejected or deferred while the same pad is loading,
unloading, analyzing, already generating stems, or playing.

#### Scenario: Loading pad blocks stem generation
- **GIVEN** a pad is currently loading source audio
- **WHEN** the performer requests stem generation for that pad
- **THEN** the system rejects or defers the stem task
- **AND** the load task continues without being replaced by stem generation

#### Scenario: Existing stem task blocks another stem task
- **GIVEN** stem generation is already running for a pad
- **WHEN** the performer requests stem generation again for that pad
- **THEN** the system rejects or defers the duplicate task
- **AND** no second conflicting stem generation task is started for that pad


<!-- Added from add-offline-stem-cache -->
### Requirement: Stem Task Completion Is Revalidated Before Publication
The system SHALL revalidate pad playback state and source version before publishing a
completed stem task result.

If the pad is playing, unloaded, or associated with a different source version when the task
completes, the result SHALL NOT replace audio-thread stem buffers for that pad.

#### Scenario: Completed task is stale
- **GIVEN** stem generation started for pad source version A
- **AND** the pad is replaced with source version B before generation completes
- **WHEN** the task finishes
- **THEN** the result for source version A is marked stale
- **AND** it is not published to the audio thread for source version B


<!-- Added from add-stem-performance-controls -->
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


<!-- Added from harden-gen3-runtime-control-paths -->
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

