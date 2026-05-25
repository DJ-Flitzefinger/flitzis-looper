## ADDED Requirements

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
