## ADDED Requirements

### Requirement: Stop All Sample Playback
The system SHALL provide a Python API `AudioEngine.stop_all()` that stops all currently active voices, regardless of sample ID.

#### Scenario: Stop-all ends all active voices
- **WHEN** one or more samples are playing due to prior `play_sample(...)` calls
- **AND** `AudioEngine.stop_all()` is called
- **THEN** all currently active voices stop contributing to the audio output

#### Scenario: Stop-all is safe when nothing is playing
- **WHEN** no samples are currently playing
- **AND** `AudioEngine.stop_all()` is called
- **THEN** the call succeeds

#### Scenario: Stop-all before engine initialization fails
- **WHEN** an `AudioEngine` has not been initialized via `run()`
- **AND** `AudioEngine.stop_all()` is called
- **THEN** the call fails with a Python exception
