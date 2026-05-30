## ADDED Requirements

### Requirement: Input Mapping Files Preserve Schemas
The system SHALL store keyboard and MIDI mappings in dedicated schema-versioned input mapping
files outside normal playback hot paths.

Clearing keyboard mappings SHALL preserve the keyboard file's schema version and
`ignore_when_typing` field while setting `mappings = []`. Clearing MIDI mappings SHALL preserve
the MIDI file's schema version and `device_mode` field while setting `mappings = []`.

#### Scenario: Keyboard clear-all preserves top-level fields
- **GIVEN** `config/input/keyboard.json` contains one or more mappings
- **WHEN** the performer activates `Delete all Keyboard Mappings`
- **THEN** the file keeps its schema version
- **AND** it keeps `ignore_when_typing`
- **AND** `mappings` is empty

#### Scenario: MIDI clear-all preserves top-level fields
- **GIVEN** `config/input/midi.json` contains one or more mappings
- **WHEN** the performer activates `Delete all MIDI Mappings`
- **THEN** the file keeps its schema version
- **AND** it keeps `device_mode`
- **AND** `mappings` is empty

### Requirement: Project State Persists Input Mapping Enabled Flag
The system SHALL persist the input mapping enabled flag with project state.

Mapping file contents SHALL remain in the dedicated input mapping files, while the project state
SHALL store whether input mapping is currently enabled. New and older projects SHALL default the
input mapping enabled flag to `true`.

#### Scenario: Enabled flag round-trips through project config
- **GIVEN** input mapping is enabled in project state
- **WHEN** project state is saved and reloaded
- **THEN** input mapping remains enabled
- **AND** keyboard and MIDI mapping entries still come from their dedicated input files

#### Scenario: Missing enabled flag defaults to on
- **GIVEN** a project file was created before the input mapping enabled flag existed
- **WHEN** the project is loaded
- **THEN** input mapping is enabled
