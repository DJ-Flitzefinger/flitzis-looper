## MODIFIED Requirements

### Requirement: Load Audio File Into Sample Slot
The system SHALL expose a Python API to load an audio file from a filesystem path into a named sample slot identified by an integer `id` in the range 0..36. The decoder SHALL support loading at least: WAV, FLAC, MP3, AIFF (`.aif`/`.aiff`), and OGG.

#### Scenario: Load succeeds
- **WHEN** `AudioEngine.load_sample(id, path)` is called with an existing audio file in a supported format
- **THEN** the file is decoded into an immutable, in-memory sample buffer
- **AND** the buffer is associated with `id` for subsequent playback

#### Scenario: Loading replaces an already-loaded sample
- **WHEN** a sample is already loaded into slot `id`
- **AND** `AudioEngine.load_sample(id, path)` is called and succeeds
- **THEN** the buffer associated with `id` is replaced by the newly loaded buffer
- **AND** any currently active voices for `id` stop contributing to the audio output

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

## ADDED Requirements

### Requirement: Unload Sample Slot
The system SHALL expose a Python API to unload a previously loaded sample from a sample slot identified by an integer `id` in the range 0..36.

#### Scenario: Unload removes sample for subsequent playback
- **WHEN** a sample is loaded into slot `id`
- **AND** `AudioEngine.unload_sample(id)` is called
- **THEN** the slot `id` has no loaded sample associated with it
- **AND** subsequent `AudioEngine.play_sample(id, ...)` triggers are ignored (or dropped)

#### Scenario: Unload stops currently playing audio for the sample id
- **WHEN** one or more voices are playing for slot `id`
- **AND** `AudioEngine.unload_sample(id)` is called
- **THEN** all currently active voices for `id` stop contributing to the audio output

#### Scenario: Unload missing sample id is handled safely
- **WHEN** `AudioEngine.unload_sample(id)` is called for an `id` with no loaded sample
- **THEN** the request is ignored (or dropped)
- **AND** the audio callback continues without panic or blocking

#### Scenario: Unload sample id is out of range
- **WHEN** `AudioEngine.unload_sample(id)` is called with `id >= 36`
- **THEN** the call fails with a Python exception
