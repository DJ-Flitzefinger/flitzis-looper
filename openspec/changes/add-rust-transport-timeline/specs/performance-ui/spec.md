## ADDED Requirements

### Requirement: Trigger Quantization Control
The system SHALL provide a performance-view trigger quantization control for the supported
Rust trigger modes: immediate, next beat, and next bar.

The control SHALL default to immediate for new projects. Selecting a mode SHALL persist the
selected global project state and send one fixed-size trigger-quantization update to the Rust
audio engine through the controller layer.

The UI SHALL NOT bypass controller actions, send full beat-grid metadata, touch audio-thread
state directly, perform disk I/O in the audio callback, acquire the Python GIL in the audio
callback, or introduce unbounded audio-thread work.

#### Scenario: New projects default to immediate triggering
- **WHEN** the application starts with a new project
- **THEN** the trigger quantization control indicates immediate triggering
- **AND** pad triggers preserve immediate behavior unless the performer chooses another mode

#### Scenario: Selecting next-bar quantization updates Rust mode
- **GIVEN** the application is running
- **WHEN** the performer selects next-bar trigger quantization
- **THEN** the project trigger quantization mode becomes `next_bar`
- **AND** the control layer calls `AudioEngine.set_trigger_quantization("next_bar")`

#### Scenario: Persisted quantization mode is restored
- **GIVEN** a saved project has trigger quantization mode `next_beat`
- **WHEN** the project is loaded
- **THEN** the control layer applies `next_beat` to the Rust audio engine
