# project-persistence Specification

## Purpose
To persist and restore project state to/from disk, making projects portable and survivable across application restarts.

## MODIFIED Requirements

### Requirement: Restore Ignores Missing Or Unusable Cached Samples
When restoring a project, the system SHALL treat `ProjectState.sample_paths[*]` as references to project-local sample cache files under `./samples/`.

If a referenced sample cache file is missing, the system SHALL ignore that pad assignment and MUST NOT crash.

If a referenced sample cache file exists but cannot be decoded by the audio engine (e.g., unsupported format, corrupt file), the system SHALL ignore that pad assignment and MUST NOT crash.

#### Scenario: Missing cached file does not crash
- **GIVEN** `ProjectState.sample_paths[pad_id]` points to a file under `./samples/`
- **AND** that file does not exist on disk
- **WHEN** the application restores the project
- **THEN** the system ignores the sample for `pad_id`
- **AND** the UI remains usable

#### Scenario: Unreadable cached file does not crash
- **GIVEN** `ProjectState.sample_paths[pad_id]` points to a file under `./samples/`
- **AND** that file exists but cannot be decoded
- **WHEN** the application restores the project
- **THEN** the system ignores the sample for `pad_id`
- **AND** the UI remains usable

## REMOVED Requirements

### Requirement: Restore Validates Cached WAV Sample Rate
The requirement "When restoring a pad from a cached WAV file, the system SHALL validate that the cached WAV sample rate matches the current audio engine output sample rate" is removed.

The scenario "Cached WAV sample rate mismatch is ignored" is removed.

## ADDED Requirements

### Requirement: Restore Preserves Analysis Results
When restoring a sample from a cached file, if `ProjectState.sample_analysis[pad_id]` contains valid analysis results, the system SHALL preserve those results and SHALL NOT automatically re-run analysis.

#### Scenario: Analysis results are restored from project state
- **GIVEN** `ProjectState.sample_paths[pad_id]` points to a usable cached file under `./samples/`
- **AND** `ProjectState.sample_analysis[pad_id]` contains valid analysis results
- **WHEN** the application restores the project
- **THEN** the system uses the stored analysis results for `pad_id`
- **AND** no automatic analysis is triggered for `pad_id`

#### Scenario: Missing analysis results triggers analysis
- **GIVEN** `ProjectState.sample_paths[pad_id]` points to a usable cached file under `./samples/`
- **AND** `ProjectState.sample_analysis[pad_id]` is `None`
- **WHEN** the application restores the project
- **THEN** the system runs analysis for `pad_id` after loading
