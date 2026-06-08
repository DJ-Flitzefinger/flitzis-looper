## MODIFIED Requirements

### Requirement: Persist And Restore ProjectState
The system SHALL persist `ProjectState` to a JSON file at `./samples/flitzis_looper.config.json` and SHALL restore it on application start.

The persisted global Key Lock value SHALL remain performer intent only. Project persistence SHALL NOT store Rubber Band handles, DLL paths, vcpkg paths, runtime buffers, measured per-process latency, or callback-internal backend state. This branch SHALL NOT require a legacy migration path for removed custom delay-line settings.

#### Scenario: Startup restores saved Key Lock intent
- **GIVEN** a valid `samples/flitzis_looper.config.json` exists with global Key Lock enabled
- **WHEN** the application starts
- **THEN** `ProjectState` is initialized from that config
- **AND** the global Key Lock intent is applied to the audio engine
- **AND** Rubber Band runtime handles are constructed from application setup, not restored from JSON

#### Scenario: Runtime backend paths are not persisted
- **GIVEN** the Rubber Band backend has been initialized locally
- **WHEN** project state is saved
- **THEN** the saved JSON does not contain Rubber Band DLL paths, vcpkg paths, native handles, or audio processing buffers
