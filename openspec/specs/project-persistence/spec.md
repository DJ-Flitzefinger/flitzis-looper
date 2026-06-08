# project-persistence Specification

## Purpose
To persist durable performer intent in a project-local JSON file while keeping invalid or missing restored assets from blocking application startup.
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
When restoring a project, the system SHALL treat `ProjectState.sample_paths[*]` as references to project-local audio files under `./samples/`.

If a referenced audio file is missing, the system SHALL ignore that pad assignment and MUST NOT crash.

If a referenced audio file exists but is not usable by the audio engine (e.g., invalid or unsupported format), the system SHALL ignore that pad assignment and MUST NOT crash.

#### Scenario: Missing cached audio file does not crash
- **GIVEN** `ProjectState.sample_paths[pad_id]` points to a file under `./samples/`
- **AND** that file does not exist on disk
- **WHEN** the application restores the project
- **THEN** the system ignores the sample for `pad_id`
- **AND** the UI remains usable

#### Scenario: Corrupt cached audio file does not crash
- **GIVEN** `ProjectState.sample_paths[pad_id]` points to a file under `./samples/`
- **AND** that file exists but cannot be decoded
- **WHEN** the application restores the project
- **THEN** the system ignores the sample for `pad_id`
- **AND** the UI remains usable

### Requirement: Restore Loads Cached Audio Through The Normal Loader
When restoring a pad from a cached audio file, the system SHALL schedule the same async loader path used for newly selected files.

The loader SHALL decode, channel-map, and resample the restored file to the current audio engine output format outside the audio callback. Restore MUST NOT perform disk I/O, decoding, or resampling in the audio callback.

#### Scenario: Cached audio sample rate mismatch is resampled outside the callback
- **GIVEN** `ProjectState.sample_paths[pad_id]` points to a cached audio file under `./samples/`
- **AND** that audio file exists but has a sample rate different from the current output sample rate
- **WHEN** the application restores the project
- **THEN** the system schedules the cached file through the async loader
- **AND** the loader resamples the decoded audio outside the audio callback
- **AND** the UI remains usable

### Requirement: Persist per-pad grid offset samples
Project persistence MUST store and restore `grid_offset_samples` per pad as part of project persistence.

If `grid_offset_samples` is missing when loading older projects, the system SHALL treat it as 0 samples.

#### Scenario: Missing grid offset field loads as zero
- **GIVEN** a project file created before `grid_offset_samples` existed
- **WHEN** the project is loaded
- **THEN** each pad's `grid_offset_samples` is treated as 0

#### Scenario: Stored grid offset is restored per pad
- **GIVEN** a project is saved with `grid_offset_samples = +123` for Pad A
- **AND** the project is saved with `grid_offset_samples = -456` for Pad B
- **WHEN** the project is loaded
- **THEN** Pad A has `grid_offset_samples = +123`
- **AND** Pad B has `grid_offset_samples = -456`


<!-- Added from add-low-jitter-input-mapping -->
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


<!-- Added from add-low-jitter-input-mapping -->
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


<!-- Added from add-per-pad-key-lock -->
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


<!-- Added from add-rust-transport-timeline -->
### Requirement: Persist Trigger Quantization Settings
The system SHALL persist trigger quantization as a separate enabled flag and musical grid step.

The persisted enabled flag SHALL default to `false` for new projects. The persisted grid step
SHALL default to `1_32` and SHALL be constrained to `1_64`, `1_32`, or `1_16`.

When older project files contain the legacy `trigger_quantization` field, the loader SHALL
migrate `immediate`, `disabled`, or `off` to disabled quantization and SHALL migrate legacy
beat/bar modes to the supported compatibility `1_16` grid step.

#### Scenario: New projects store disabled quantization with default grid
- **WHEN** a new project state is created
- **THEN** `trigger_quantization_enabled` is `false`
- **AND** `trigger_quantization_step` is `1_32`

#### Scenario: Legacy next-beat mode migrates to enabled supported grid
- **GIVEN** an older project contains `trigger_quantization = "next_beat"`
- **WHEN** the project is loaded
- **THEN** `trigger_quantization_enabled` is `true`
- **AND** `trigger_quantization_step` is `1_16`

#### Scenario: Legacy immediate mode migrates to disabled quantization
- **GIVEN** an older project contains `trigger_quantization = "immediate"`
- **WHEN** the project is loaded
- **THEN** `trigger_quantization_enabled` is `false`
- **AND** `trigger_quantization_step` remains the default `1_32`


<!-- Added from add-stem-performance-controls -->
### Requirement: Persist Stem Cache Metadata
The system SHALL persist per-pad stem cache metadata as project-local source-version and
cache artifact references.

Project restore SHALL revalidate persisted stem cache metadata against the current loaded
source version and complete project-local cache files before marking stems available for
playback. Restore SHALL degrade safely to full-mix playback when metadata is missing, stale,
invalid, or incomplete.

Project restore SHALL publish a restored current prepared stem set to the Rust audio engine after
the matching full-mix sample has loaded successfully. If Rust publication rejects the restored
prepared set, restore SHALL mark those stems unavailable for performer controls and SHALL preserve
full-mix playback.

#### Scenario: Current stem cache metadata restores as available
- **GIVEN** a saved project has stem cache metadata for pad source version A
- **AND** the pad still loads source version A
- **AND** all expected stem cache files exist
- **WHEN** the project is restored
- **THEN** the pad's stem cache may be marked available
- **AND** playback remains usable if Rust publication later rejects the prepared set

#### Scenario: Restored current stems are published after full mix load
- **GIVEN** a saved project has complete stem cache metadata for pad source version A
- **AND** the pad's full-mix sample restores successfully as source version A
- **WHEN** the restored sample load completes
- **THEN** the system publishes the prepared stem set to the Rust audio engine
- **AND** a restored all-stems preference can affect playback without regenerating stems

#### Scenario: Rejected restored stems become unavailable
- **GIVEN** a saved project has complete stem cache metadata for pad source version A
- **AND** the pad's full-mix sample restores successfully as source version A
- **WHEN** Rust rejects publication of the restored prepared stem set
- **THEN** the system marks the stems unavailable for performer controls
- **AND** the pad remains playable using full-mix playback

#### Scenario: Stale stem cache metadata is not restored as playable
- **GIVEN** a saved project has stem cache metadata for source version A
- **AND** the pad now loads source version B
- **WHEN** the project is restored
- **THEN** stems for source version A are not eligible for playback on that pad
- **AND** the pad remains playable using full-mix playback


<!-- Added from add-stem-performance-controls -->
### Requirement: Persist Durable Stem Mix Preferences
The system SHALL persist durable per-pad stem mix preferences separately from transient stem
generation and performance gesture state.

New and older projects SHALL default each pad's stem mix preference to full-mix playback.
Momentary solo, momentary mute, per-stem enabled masks, generation progress, blocked reasons, and
last error messages SHALL remain session-only unless a later OpenSpec change explicitly makes them
durable.

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

#### Scenario: Runtime stem mask is not persisted
- **GIVEN** the performer changes the selected-pad stem mask during a session
- **WHEN** project state is saved
- **THEN** the enabled-stem mask is not written as a durable project setting
- **AND** the durable full-mix/all-stems mode preference remains independent


<!-- Added from add-stem-performance-controls -->
### Requirement: Persist Demucs Stem Quality Settings
The system SHALL persist global Demucs stem-generation quality settings in project state.

New and older projects SHALL default Demucs shifts to 1 and Demucs overlap to 0.5. Persisted
quality settings SHALL be validated against the app-supported ranges before they can be used for
generation: shifts from 1 through 20 and overlap from 0.25 through 0.95. Project-level changes
SHALL be written through the existing `samples/flitzis_looper.config.json` persistence path
without requiring a separate Apply action.

#### Scenario: Demucs quality settings round-trip
- **GIVEN** a project is saved with Demucs shifts set to 4
- **AND** Demucs overlap set to 0.25
- **WHEN** the project is loaded again
- **THEN** the same Demucs quality settings are restored

#### Scenario: Demucs quality changes persist without apply
- **GIVEN** the Settings page is open
- **WHEN** the performer changes Demucs shifts or Demucs overlap within the supported range
- **THEN** the project state is persisted to `samples/flitzis_looper.config.json` through the
  existing project persistence path
- **AND** no separate Apply action is required

#### Scenario: Older project defaults quality settings
- **GIVEN** a project file was created before Demucs quality settings existed
- **WHEN** the project is loaded
- **THEN** Demucs shifts defaults to 1
- **AND** Demucs overlap defaults to 0.5


<!-- Added from clarify-state-ownership-boundary -->
### Requirement: Project State Owns Durable Performer Intent
The system SHALL treat `ProjectState` as the durable owner of project and performer intent that
must survive application restart.

Durable performer intent includes project-local sample references, sample duration metadata,
analysis metadata, manual BPM and key overrides, loop settings, per-pad gain and EQ values, global
speed and volume, global performance modes, trigger quantization settings, input-mapping
enablement, stem cache metadata, stem mix preference, and project-scoped generation/settings
values.

The system SHALL NOT persist transient runtime projections such as active pads, paused pads,
playheads, peaks, pending async task progress, Learn input capture, waveform editor state, or
settings overlay state as live audio truth.

#### Scenario: Restart restores intent but not live playback
- **GIVEN** a project has persisted sample references, loop settings, per-pad gain, and global
  modes
- **WHEN** the application starts
- **THEN** those durable settings are restored from `ProjectState`
- **AND** no pad is treated as already playing solely because it was playing before shutdown


<!-- Added from clarify-state-ownership-boundary -->
### Requirement: Project Restore Publishes Durable Intent After Audio Startup
The system SHALL publish restored durable audio intent to Rust only after the audio engine has
started and the controllers have been constructed.

Restore publication SHALL use bounded control or parameter messages and SHALL keep disk I/O,
sample decoding, project JSON reads, cache validation, Python UI work, and background task
scheduling outside the audio callback.

#### Scenario: Startup restore keeps callback isolated from persistence
- **GIVEN** a project file exists with saved global and per-pad audio settings
- **WHEN** the application starts and restores the project
- **THEN** Python control code loads and validates the persisted state
- **AND** Python publishes bounded audio messages after the Rust engine is running
- **AND** the audio callback does not read project JSON, inspect sample paths, or validate cache
  files


<!-- Added from harden-gen3-runtime-control-paths -->
### Requirement: Startup restore publishes only loaded pad audio intent
The system SHALL publish restored per-pad live-audio state during startup only for pads with valid restored sample assignments, while preserving explicit neutralization when pads are unloaded or cleared.

Per-pad gain, EQ, BPM, timing metadata, loop region, Key Lock, stem mode, and stem mask publication SHALL be skipped for empty pads during normal startup projection. Controller paths that unload, clear, or force-reset a pad SHALL still publish the bounded neutral state needed to remove stale Rust live-audio state for that pad.

#### Scenario: Startup skips empty pad per-pad settings
- **GIVEN** a restored project contains valid cached audio for pad 1 only
- **AND** persisted per-pad settings for empty pads differ from defaults
- **WHEN** the application starts and projects restored state to Rust
- **THEN** per-pad live-audio settings are published for pad 1 as needed
- **AND** normal startup projection does not publish per-pad gain, EQ, BPM, timing, loop, Key Lock, stem mode, or stem mask state for the empty pads

#### Scenario: Missing restored sample still neutralizes that pad
- **GIVEN** a restored project references a cached audio file for pad 2
- **AND** the cached file is missing or unusable
- **WHEN** startup clears pad 2
- **THEN** the controller publishes the bounded neutral state required to clear Rust live state for pad 2
- **AND** the system does not publish restored non-default per-pad settings for pad 2 as if the sample were loaded

#### Scenario: Explicit unload keeps force-reset behavior
- **GIVEN** pad 3 is loaded and has non-default live per-pad state
- **WHEN** the performer unloads pad 3
- **THEN** the controller clears project/session state for pad 3
- **AND** the controller publishes bounded neutral live-audio state for pad 3
- **AND** future startup projection treats pad 3 as empty unless it has a valid restored sample assignment


<!-- Added from rework-pad-gain-trim -->
### Requirement: Persist and migrate dB pad Gain/Trim
The system SHALL persist per-pad Gain/Trim as dB intent and migrate legacy per-pad gain values
without accidentally boosting old projects.

New project defaults SHALL store or derive `0.0 dB` Gain/Trim for every pad. If an older project
contains the legacy `pad_gain` field instead of the dB field, the loader SHALL migrate old unity
values `1.0` and `100` to `0.0 dB`. Legacy values below unity SHALL be converted to dB through
`20 * log10(linear_gain)` and clamped to `-12.0 dB`. Missing Gain/Trim data SHALL default to
`0.0 dB`.

#### Scenario: Missing gain defaults to neutral trim
- **GIVEN** a project file does not contain per-pad Gain/Trim data
- **WHEN** the project is loaded
- **THEN** every pad has Gain/Trim `0.0 dB`

#### Scenario: Legacy unity gain migrates to zero dB
- **GIVEN** a project file contains legacy `pad_gain` value `1.0` for Pad A
- **AND** a project file contains legacy `pad_gain` value `100` for Pad B
- **WHEN** the project is loaded
- **THEN** Pad A has Gain/Trim `0.0 dB`
- **AND** Pad B has Gain/Trim `0.0 dB`

#### Scenario: Legacy reduced gain migrates without boost
- **GIVEN** a project file contains legacy `pad_gain` value `0.5`
- **WHEN** the project is loaded
- **THEN** the pad has Gain/Trim approximately `-6.0 dB`
- **AND** the pad does not load with positive Gain/Trim

