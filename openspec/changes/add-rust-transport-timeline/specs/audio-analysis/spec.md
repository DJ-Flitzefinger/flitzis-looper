## ADDED Requirements

### Requirement: Beatgrid Metadata Can Be Published To Rust Timing
The system SHALL make audio-analysis beatgrid and downbeat metadata available to Rust
playback timing as bounded, precomputed timing metadata.

Analysis, validation, and conversion of beatgrid vectors SHALL happen outside the audio
callback. The audio callback SHALL receive only fixed-size messages or preallocated state
updates that are safe to process in real time.

#### Scenario: Analysis results produce bounded timing metadata
- **GIVEN** a pad has analysis results with BPM, beats, downbeats, and bars
- **WHEN** the controller or background Rust code prepares playback timing metadata
- **THEN** it computes finite bounded values suitable for Rust transport use
- **AND** no beat detection or vector allocation is required in the audio callback

#### Scenario: Persisted analysis can restore timing metadata
- **GIVEN** a project restores persisted analysis results for a pad
- **WHEN** the pad is restored to the audio engine
- **THEN** the system can republish the pad's bounded beatgrid/downbeat timing metadata to Rust
- **AND** quantized playback can use the metadata without re-running analysis in the callback
