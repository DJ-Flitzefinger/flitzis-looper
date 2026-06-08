## ADDED Requirements

### Requirement: Beatgrid Metadata Can Be Published To Rust Timing
The system SHALL make audio-analysis beatgrid and downbeat metadata available to Rust
playback timing as bounded, precomputed timing metadata.

Analysis, validation, and conversion of beatgrid vectors SHALL happen outside the audio
callback. The audio callback SHALL receive only fixed-size messages or preallocated state
updates that are safe to process in real time. The bounded metadata SHALL include a finite
non-negative per-pad phase anchor in seconds and SHALL NOT publish full beat-grid vectors into the
audio callback.

#### Scenario: Analysis results produce bounded timing metadata
- **GIVEN** a pad has analysis results with BPM, beats, downbeats, and bars
- **WHEN** the controller or background Rust code prepares playback timing metadata
- **THEN** it computes finite bounded values suitable for Rust transport use
- **AND** no beat detection or vector allocation is required in the audio callback

#### Scenario: Invalid timing metadata falls back safely
- **GIVEN** a pad has analysis metadata with no finite non-negative downbeat or beat anchor
- **WHEN** bounded playback timing metadata is prepared
- **THEN** the published phase anchor is `0.0` seconds
- **AND** the audio callback updates fixed-size state without panic, allocation, blocking, disk I/O, or GIL access

#### Scenario: Unloaded pad does not publish stale timing metadata
- **GIVEN** a pad has been unloaded
- **AND** the pad has a persisted negative grid offset from earlier editing
- **WHEN** controller code clears the pad BPM and analysis state
- **THEN** the control layer does not publish a stale negative pad timing anchor to Rust
- **AND** unload completes without raising an audio-engine timing metadata validation error

#### Scenario: Persisted analysis can restore timing metadata
- **GIVEN** a project restores persisted analysis results for a pad
- **WHEN** the pad is restored to the audio engine
- **THEN** the system can republish the pad's bounded beatgrid/downbeat timing metadata to Rust
- **AND** quantized playback can use the metadata without re-running analysis in the callback
