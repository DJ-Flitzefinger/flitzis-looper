# minimal-audio-engine Specification

## Purpose
To provide a low-latency real-time audio engine implemented in Rust with bindings for use from Python code.
## Requirements
### Requirement: Python AudioEngine Instantiation
The system SHALL expose an AudioEngine class to Python that can be instantiated with `AudioEngine()`.

#### Scenario: Python instantiation success
- **WHEN** Python code imports `flitzis_looper_rs`
- **THEN** the `AudioEngine` class is available
- **AND** an instance can be created with `AudioEngine()`
- **AND** the instance has `run()` and `shut_down()` methods

#### Scenario: Python FFI integration
- **WHEN** an `AudioEngine` is instantiated from Python
- **THEN** the underlying Rust `AudioEngine` is created
- **AND** the cpal stream is initialized
- **AND** no Python GIL is held during audio processing

