## ADDED Requirements

### Requirement: Persist Stem Cache Metadata
The system SHALL persist per-pad stem cache metadata as project-local source-version and
cache artifact references.

Project restore SHALL revalidate persisted stem cache metadata against the current loaded
source version and complete project-local cache files before marking stems available for
playback. Restore SHALL degrade safely to full-mix playback when metadata is missing, stale,
invalid, or incomplete.

#### Scenario: Current stem cache metadata restores as available
- **GIVEN** a saved project has stem cache metadata for pad source version A
- **AND** the pad still loads source version A
- **AND** all expected stem cache files exist
- **WHEN** the project is restored
- **THEN** the pad's stem cache may be marked available
- **AND** playback remains usable if Rust publication later rejects the prepared set

#### Scenario: Stale stem cache metadata is not restored as playable
- **GIVEN** a saved project has stem cache metadata for source version A
- **AND** the pad now loads source version B
- **WHEN** the project is restored
- **THEN** stems for source version A are not eligible for playback on that pad
- **AND** the pad remains playable using full-mix playback

### Requirement: Persist Durable Stem Mix Preferences
The system SHALL persist durable per-pad stem mix preferences separately from transient stem
generation and performance gesture state.

New and older projects SHALL default each pad's stem mix preference to full-mix playback.
Momentary solo, momentary mute, generation progress, blocked reasons, and last error messages
SHALL remain session-only unless a later OpenSpec change explicitly makes them durable.

#### Scenario: Stem mix preference round-trips
- **GIVEN** a project is saved with pad A configured for all-stems mode
- **WHEN** the project is loaded again
- **THEN** pad A's durable stem mix preference is restored as all-stems
- **AND** actual playback still falls back to full mix until current prepared stems are valid

#### Scenario: Older project defaults to full mix
- **GIVEN** a project file was created before stem mix preferences existed
- **WHEN** the project is loaded
- **THEN** every pad's stem mix preference is treated as full-mix playback

#### Scenario: Runtime stem progress is not persisted
- **GIVEN** stem generation is running for a pad
- **WHEN** project state is saved
- **THEN** generation progress, blocked reasons, and transient error text are not written as durable project settings
