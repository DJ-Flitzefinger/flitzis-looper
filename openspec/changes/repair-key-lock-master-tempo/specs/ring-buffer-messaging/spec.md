## MODIFIED Requirements

### Requirement: Real-time Safety
The system SHALL keep Key Lock control and processing within the existing fixed-size ring-buffer
and bounded audio-thread state model.

Python SHALL publish Key Lock mode and Key Lock DSP parameter changes as fixed-size scalar control
messages. The audio callback SHALL consume that state without reading files, loading plugins,
resizing DSP buffers, allocating audio payloads, blocking, logging, acquiring the Python GIL, or
invoking neural inference.

#### Scenario: Key Lock message remains fixed-size
- **GIVEN** the performer toggles Key Lock
- **WHEN** Python sends the update to Rust
- **THEN** the control message contains only the bounded boolean mode state
- **AND** the audio callback applies it using already-owned mixer state

#### Scenario: Key Lock parameter message remains fixed-size
- **GIVEN** the performer changes a Key Lock DSP parameter in Settings
- **WHEN** Python sends the update to Rust
- **THEN** the control message contains only bounded numeric and enum parameter values
- **AND** the audio callback applies it using already-owned mixer state
