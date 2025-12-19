## ADDED Requirements
### Requirement: AudioEngine Initialization
The system SHALL initialize an audio output stream with cpal when `AudioEngine::new()` is called.

#### Scenario: Success case
- **WHEN** the system has at least one audio output device
- **THEN** a valid cpal stream is created
- **AND** no error is returned
- **AND** the sample rate is set to 48000 Hz
- **AND** the buffer size is 512 samples

## ADDED Requirements
### Requirement: AudioEngine Playback
The system SHALL start streaming audio when `AudioEngine::play()` is called.

#### Scenario: Success case
- **WHEN** an AudioEngine is initialized
- **THEN** the audio stream starts
- **AND** a silent buffer is streamed at 48000 Hz
- **AND** the buffer size is 512 samples

## ADDED Requirements
### Requirement: AudioEngine Shutdown
The system SHALL gracefully terminate the audio stream when `AudioEngine::stop()` is called.

#### Scenario: Success case
- **WHEN** an AudioEngine is playing
- **THEN** the audio stream terminates gracefully
- **AND** no memory leaks occur

## ADDED Requirements
### Requirement: AudioEngine Device Handling
The system SHALL return `AudioError::DeviceNotFound` when no audio output devices are available.

#### Scenario: Device not found
- **WHEN** no audio output devices are available
- **THEN** `AudioError::DeviceNotFound` is returned
- **AND** no stream is created

## ADDED Requirements
### Requirement: Python AudioEngine Instantiation
The system SHALL expose an AudioEngine class to Python that can be instantiated with `AudioEngine()`.

#### Scenario: Python instantiation success
- **WHEN** Python code imports flitzis_looper_rs
- **THEN** the AudioEngine class is available
- **AND** an instance can be created with `AudioEngine()`
- **AND** the instance has play() and stop() methods

#### Scenario: Python FFI integration
- **WHEN** an AudioEngine is instantiated from Python
- **THEN** the underlying Rust AudioEngine is created
- **AND** the cpal stream is initialized
- **AND** no Python GIL is held during audio processing