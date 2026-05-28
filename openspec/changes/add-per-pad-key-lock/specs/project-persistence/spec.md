## MODIFIED Requirements

### Requirement: Persist And Restore ProjectState
The system SHALL persist `ProjectState` to a JSON file at `./samples/flitzis_looper.config.json` and SHALL restore it on application start.

If the config file does not exist, the system SHALL start with `ProjectState` defaults.

If the config file exists but cannot be read or validated, the system SHALL start with `ProjectState` defaults and SHALL keep the UI usable.

The persisted global Key Lock value SHALL remain durable performer intent for the global master control. Project persistence SHALL also store per-pad Key Lock intent as bounded boolean project data for loaded pads while unloaded pads SHALL be normalized to disabled per-pad Key Lock intent. Project persistence SHALL NOT store Rubber Band handles, native DLL paths, vcpkg paths, runtime buffers, measured latency, or callback-internal backend state.

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

#### Scenario: Backend runtime state is not persisted
- **GIVEN** Key Lock playback has initialized Rubber Band runtime state
- **WHEN** project state is saved
- **THEN** the saved JSON does not contain Rubber Band handles, native DLL paths, vcpkg paths, runtime buffers, measured latency, or callback-internal backend state

## ADDED Requirements

### Requirement: Persist Per-Pad Key Lock Intent
The system SHALL persist and restore per-pad Key Lock intent only for pads with loaded audio.

New projects and older project files that do not contain per-pad Key Lock values SHALL default every pad's value to disabled. Persisted per-pad Key Lock data SHALL be validated as a fixed-length boolean list matching the project pad count before it can be used. When project data is loaded or saved, unloaded pads SHALL have disabled per-pad Key Lock values. Global Key Lock changes SHALL persist overwritten values only for currently loaded pads through the same project persistence path as per-pad edits.

#### Scenario: Older project defaults per-pad Key Lock off
- **GIVEN** a project file was created before per-pad Key Lock values existed
- **WHEN** the project is loaded
- **THEN** every pad's per-pad Key Lock value is treated as disabled

#### Scenario: Per-pad Key Lock values round-trip
- **GIVEN** a project is saved with Pad 3 loaded and Pad 3 Key Lock enabled
- **AND** Pad 4 Key Lock disabled
- **WHEN** the project is loaded again
- **THEN** Pad 3's per-pad Key Lock value is restored as enabled
- **AND** Pad 4's per-pad Key Lock value is restored as disabled

#### Scenario: Global Key Lock overwrite is durable for loaded pads
- **GIVEN** a project has mixed per-pad Key Lock values
- **AND** only some pads have loaded audio
- **WHEN** the performer enables global Key Lock
- **AND** project state is saved
- **THEN** the saved project contains enabled per-pad Key Lock values for loaded pads
- **AND** unloaded pads are saved with disabled per-pad Key Lock values

#### Scenario: Unloaded pad Key Lock intent is not restored
- **GIVEN** a project file contains an enabled per-pad Key Lock value for an unloaded pad
- **WHEN** the project is loaded
- **THEN** that unloaded pad's per-pad Key Lock value is treated as disabled

#### Scenario: Invalid per-pad Key Lock length is rejected
- **GIVEN** a project file contains a per-pad Key Lock list with fewer or more entries than the project pad count
- **WHEN** the project is loaded
- **THEN** the invalid project data fails model validation or falls back through the safe project-load path
- **AND** the application remains usable
