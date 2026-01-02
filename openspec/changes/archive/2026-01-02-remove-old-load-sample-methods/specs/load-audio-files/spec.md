## MODIFIED Requirements
### Requirement: Load Audio File Into Sample Slot
The system SHALL expose a Python API to load an audio file from a filesystem path into a named sample slot identified by an integer `id` in the range 0..36. The decoder SHALL support loading at least: WAV, FLAC, MP3, AIFF (`.aif`/`.aiff`), and OGG.

#### Scenario: Load succeeds
- **WHEN** `AudioEngine.load_sample_async(id, path)` is called with an existing audio file in a supported format
- **THEN** the file is decoded into an immutable, in-memory sample buffer
- **AND** the buffer is associated with `id` for subsequent playback

#### Scenario: Loading replaces an already-loaded sample
- **WHEN** a sample is already loaded into slot `id`
- **AND** `AudioEngine.load_sample_async(id, path)` is called and succeeds
- **THEN** the buffer associated with `id` is replaced by the newly loaded buffer
- **AND** any currently active voices for `id` stop contributing to the audio output

#### Scenario: Sample id is out of range
- **WHEN** `AudioEngine.load_sample_async(id, path)` is called with `id >= 36`
- **THEN** the call fails with a Python exception
- **AND** no sample slot state is modified

#### Scenario: File path is invalid
- **WHEN** `AudioEngine.load_sample_async(id, path)` is called with a path that does not exist
- **THEN** the call fails with a Python exception
- **AND** no sample slot state is modified

#### Scenario: File format is unsupported
- **WHEN** `AudioEngine.load_sample_async(id, path)` is called with a file that cannot be decoded
- **THEN** the call fails with a Python exception
- **AND** no sample slot state is modified

## REMOVED Requirements
### Requirement: Non-Real-Time Loading
**Reason**: Loading is performed asynchronously via `load_sample_async`; this requirement referenced `load_sample(...)`.
**Migration**: Use `AudioEngine.load_sample_async(...)`.

#### Scenario: Audio callback stays real-time safe during loading
- **WHEN** `AudioEngine.load_sample(...)` is called
- **THEN** no disk I/O is performed in the CPAL audio callback
- **AND** the callback continues rendering without blocking
