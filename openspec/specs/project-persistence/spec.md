# project-persistence Specification

## Purpose
TBD - created by archiving change add-portable-project-persistence. Update Purpose after archive.
## Requirements
### Requirement: Persist And Restore ProjectState
The system SHALL persist `ProjectState` to a JSON file at `./samples/flitzis_looper.config.json` and SHALL restore it on application start.

If the config file does not exist, the system SHALL start with `ProjectState` defaults.

If the config file exists but cannot be read or validated, the system SHALL start with `ProjectState` defaults and SHALL keep the UI usable.

#### Scenario: Startup restores saved project state
- **GIVEN** a valid `samples/flitzis_looper.config.json` exists
- **WHEN** the application starts
- **THEN** `ProjectState` is initialized from that config
- **AND** persisted global settings (e.g., `speed`, `volume`, `multi_loop`, `bpm_lock`, `key_lock`) are applied

#### Scenario: Missing config starts with defaults
- **GIVEN** `samples/flitzis_looper.config.json` does not exist
- **WHEN** the application starts
- **THEN** the system uses default `ProjectState` values

#### Scenario: Invalid config does not block startup
- **GIVEN** `samples/flitzis_looper.config.json` exists but is invalid JSON or fails model validation
- **WHEN** the application starts
- **THEN** the system uses default `ProjectState` values
- **AND** the system does not crash

### Requirement: Debounced ProjectState Saving
The system SHALL save `ProjectState` when it changes, but writes to `samples/flitzis_looper.config.json` MUST be debounced so they occur at most once every 10 seconds.

The system SHOULD attempt a best-effort final save during clean shutdown.

#### Scenario: Rapid state changes result in limited disk writes
- **GIVEN** `ProjectState` changes multiple times within 10 seconds
- **WHEN** the persistence mechanism runs
- **THEN** no more than one write occurs within that 10 second window

#### Scenario: Idle dirty state is eventually flushed
- **GIVEN** `ProjectState` is changed and becomes dirty
- **WHEN** at least 10 seconds pass
- **THEN** the system writes an updated `samples/flitzis_looper.config.json`

### Requirement: Restore Ignores Missing Or Unusable Cached Samples
When restoring a project, the system SHALL treat `ProjectState.sample_paths[*]` as references to project-local sample cache files under `./samples/`.

If a referenced sample cache file is missing, the system SHALL ignore that pad assignment and MUST NOT crash.

If a referenced sample cache file exists but is not usable by the audio engine (e.g., invalid WAV), the system SHALL ignore that pad assignment and MUST NOT crash.

#### Scenario: Missing cached WAV does not crash
- **GIVEN** `ProjectState.sample_paths[pad_id]` points to a file under `./samples/`
- **AND** that file does not exist on disk
- **WHEN** the application restores the project
- **THEN** the system ignores the sample for `pad_id`
- **AND** the UI remains usable

#### Scenario: Corrupt cached WAV does not crash
- **GIVEN** `ProjectState.sample_paths[pad_id]` points to a file under `./samples/`
- **AND** that file exists but cannot be decoded
- **WHEN** the application restores the project
- **THEN** the system ignores the sample for `pad_id`
- **AND** the UI remains usable

### Requirement: Restore Validates Cached WAV Sample Rate
When restoring a pad from a cached WAV file, the system SHALL validate that the cached WAV sample rate matches the current audio engine output sample rate.

If the cached WAV sample rate does not match, the system SHALL ignore that sample and MUST NOT crash.

#### Scenario: Cached WAV sample rate mismatch is ignored
- **GIVEN** `ProjectState.sample_paths[pad_id]` points to a cached WAV under `./samples/`
- **AND** that WAV exists but has a sample rate different from the current output sample rate
- **WHEN** the application restores the project
- **THEN** the system ignores the sample for `pad_id`
- **AND** the UI remains usable

