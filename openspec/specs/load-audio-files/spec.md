# load-audio-files Specification

## Purpose
To support sample-based playback by loading, decoding, and unloading audio files as immutable in-memory buffers associated with sample slot IDs, without performing disk I/O or decoding in the real-time audio callback.
## Requirements

### Requirement: Load Audio File Into Sample Slot
The system SHALL expose a Python API to load an audio file from a filesystem path into a named sample slot identified by an integer `id` in the range 0..36.

Before the loaded sample is considered part of the current project, the system SHALL copy the original audio file into the project's `./samples/` directory.

The persisted sample path stored in `ProjectState.sample_paths[id]` SHALL refer to the project-local audio file under `./samples/`.

If a file with the same basename already exists in `./samples/`, the system SHALL choose a non-colliding filename by appending `_0`, `_1`, etc., rather than overwriting the existing file.

The decoder SHALL support loading at least: WAV, FLAC, MP3, AIFF (`.aif`/`.aiff`), and OGG.

#### Scenario: Load succeeds
- **WHEN** `AudioEngine.load_sample_async(id, path)` is called with an existing audio file in a supported format
- **THEN** the file is decoded and resampled for playback
- **AND** the original audio file is copied under `./samples/` using the original basename as the primary name (with numeric suffix if needed to avoid collision)
- **AND** `ProjectState.sample_paths[id]` points to that project-local audio file path

#### Scenario: Loading replaces an already-loaded sample
- **WHEN** a sample is already loaded into slot `id`
- **AND** `AudioEngine.load_sample_async(id, path)` is called and succeeds
- **THEN** the buffer associated with `id` is replaced by the newly loaded buffer
- **AND** `ProjectState.sample_paths[id]` is updated to the new project-local WAV path
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

### Requirement: Efficient Publication To Audio Thread
The system SHALL publish loaded sample buffers to the audio callback via shared memory handles (e.g., reference-counted pointers) rather than copying full sample data through control messages.

#### Scenario: Sample publication uses a lightweight handle
- **WHEN** a sample is loaded
- **THEN** the audio thread receives only an `id` and a handle to the sample buffer
- **AND** the sample data is not duplicated solely for cross-thread transfer

### Requirement: Unload Sample Slot
The system SHALL expose a Python API to unload a previously loaded sample from a sample slot identified by an integer `id` in the range 0..36.

If `ProjectState.sample_paths[id]` refers to a project-local audio file under `./samples/`, the system SHALL attempt to delete that cached file when unloading the pad.

If the cached file is not present, the system MUST ignore the deletion attempt and MUST NOT crash.

#### Scenario: Unload removes sample for subsequent playback
- **WHEN** a sample is loaded into slot `id`
- **AND** `AudioEngine.unload_sample(id)` is called
- **THEN** the slot `id` has no loaded sample associated with it
- **AND** subsequent `AudioEngine.play_sample(id, ...)` triggers are ignored (or dropped)

#### Scenario: Unload stops currently playing audio for the sample id
- **WHEN** one or more voices are playing for slot `id`
- **AND** `AudioEngine.unload_sample(id)` is called
- **THEN** all currently active voices for `id` stop contributing to the audio output

#### Scenario: Unload removes cached WAV file
- **GIVEN** `ProjectState.sample_paths[id]` points to a cached audio file under `./samples/`
- **AND** that file exists on disk
- **WHEN** `AudioEngine.unload_sample(id)` is called
- **THEN** the cached audio file is removed from `./samples/`

#### Scenario: Unload ignores missing cached WAV file
- **GIVEN** `ProjectState.sample_paths[id]` points to a cached audio file under `./samples/`
- **AND** that file does not exist on disk
- **WHEN** `AudioEngine.unload_sample(id)` is called
- **THEN** the system does not crash

#### Scenario: Unload missing sample id is handled safely
- **WHEN** `AudioEngine.unload_sample(id)` is called for an `id` with no loaded sample
- **THEN** the request is ignored (or dropped)

#### Scenario: Unload sample id is out of range
- **WHEN** `AudioEngine.unload_sample(id)` is called with `id >= 36`
- **THEN** the call fails with a Python exception

### Requirement: Store Analysis Results In App State
The system SHALL store detected BPM, key, and beat grid for each loaded sample slot in application state intended for persistence, so that results do not need to be recalculated on restart.

The persisted beat grid SHALL use a reduced representation consisting of beat times and downbeat times (in seconds). This representation is sufficient for planned waveform overlays, onset suggestion, and beat alignment.

#### Scenario: Load stores analysis results
- **WHEN** a sample is loaded successfully into slot `id`
- **THEN** the system stores the detected BPM and key for `id`
- **AND** the system stores the detected beat grid for `id`

#### Scenario: Unload clears analysis results
- **GIVEN** a sample is loaded into slot `id` and analysis results exist
- **WHEN** the sample is unloaded from slot `id`
- **THEN** analysis results for `id` are cleared

#### Scenario: Replacing a sample replaces analysis results
- **GIVEN** a sample is loaded into slot `id` and analysis results exist
- **WHEN** a different sample is loaded into slot `id` successfully
- **THEN** analysis results for `id` correspond to the newly loaded sample



