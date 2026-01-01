# minimal-audio-engine Specification

## Purpose
To provide a low-latency real-time audio engine implemented in Rust with bindings for use from Python code.
## Requirements
### Requirement: Minimal Audio Engine Implementation
The system SHALL provide a minimal audio engine capable of real-time playback, mixing, and streaming using Rust with thread-safe communication to Python.

The audio engine implementation SHALL be organized in a modular structure with clear separation of concerns.

#### Module Structure
```
audio_engine/
├── mod.rs – Main module that orchestrates sub-modules
├── mixer.rs – Real-time mixing engine
├── voice.rs – Voice state management
├── sample_loader.rs – Audio file loading and decoding
├── constants.rs – Configuration constants
└── errors.rs – Audio-specific error types
```

#### Scenario: Modular Audio Engine Initialization
- **WHEN** the audio engine is initialized via `AudioEngine::new()`
- **THEN** it loads all necessary sub-modules and maintains the same public API
- **AND** the internal structure follows clear separation of concerns between loading, mixing, messaging, and streaming

#### Scenario: Maintained API Compatibility
- **WHEN** the audio engine is refactored into modules
- **THEN** all existing public methods remain unchanged
- **AND** all existing behavior is preserved
- **AND** no breaking changes to the Python FFI interface

#### Scenario: Improved Code Organization
- **WHEN** a developer needs to modify a specific component
- **THEN** they can navigate to the appropriate sub-module
- **AND** each module has a clear, single responsibility
- **AND** modules are testable in isolation

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

