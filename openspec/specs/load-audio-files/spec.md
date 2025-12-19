# load-audio-files Specification

## Purpose
To support sample-based playback by loading and decoding audio files into immutable in-memory buffers associated with sample slot IDs, without performing disk I/O or decoding in the real-time audio callback.
## Requirements
### Requirement: Load Audio File Into Sample Slot
The system SHALL expose a Python API to load an audio file from a filesystem path into a named sample slot identified by an integer `id` in the range 0..36.

#### Scenario: Load succeeds
- **WHEN** `AudioEngine.load_sample(id, path)` is called with an existing audio file supported by `symphonia` (e.g., WAV)
- **THEN** the file is decoded into an immutable, in-memory sample buffer
- **AND** the buffer is associated with `id` for subsequent playback

#### Scenario: Sample id is out of range
- **WHEN** `AudioEngine.load_sample(id, path)` is called with `id >= 36`
- **THEN** the call fails with a Python exception
- **AND** no sample slot state is modified

#### Scenario: File path is invalid
- **WHEN** `AudioEngine.load_sample(id, path)` is called with a path that does not exist
- **THEN** the call fails with a Python exception
- **AND** no sample slot state is modified

#### Scenario: File format is unsupported
- **WHEN** `AudioEngine.load_sample(id, path)` is called with a file that cannot be decoded
- **THEN** the call fails with a Python exception
- **AND** no sample slot state is modified

### Requirement: Non-Real-Time Loading
The system SHALL perform all disk I/O and audio decoding outside the real-time audio callback.

#### Scenario: Audio callback stays real-time safe during loading
- **WHEN** `AudioEngine.load_sample(...)` is called
- **THEN** no disk I/O is performed in the CPAL audio callback
- **AND** the callback continues rendering without blocking

### Requirement: Efficient Publication To Audio Thread
The system SHALL publish loaded sample buffers to the audio callback via shared memory handles (e.g., reference-counted pointers) rather than copying full sample data through control messages.

#### Scenario: Sample publication uses a lightweight handle
- **WHEN** a sample is loaded
- **THEN** the audio thread receives only an `id` and a handle to the sample buffer
- **AND** the sample data is not duplicated solely for cross-thread transfer

