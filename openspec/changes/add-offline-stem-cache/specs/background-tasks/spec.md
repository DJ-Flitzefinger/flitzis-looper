## ADDED Requirements

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
