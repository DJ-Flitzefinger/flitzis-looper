## MODIFIED Requirements

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
