## MODIFIED Requirements
### Requirement: Load Audio File Into Sample Slot
The system SHALL expose a Python API to load an audio file from a filesystem path into a named sample slot identified by an integer `id` in the range 0..36.

Before the loaded sample is considered part of the current project, the system SHALL copy the decoded audio into the project’s `./samples/` directory as a WAV file resampled to the current audio engine output sample rate.

The cached WAV encoding MUST use the same sample format as the engine’s in-memory sample buffers (currently `f32`) to minimize conversion during fast load.

The persisted sample path stored in `ProjectState.sample_paths[id]` SHALL refer to the project-local WAV file under `./samples/`.

The decoder SHALL support loading at least: WAV, FLAC, MP3, AIFF (`.aif`/`.aiff`), and OGG.

#### Scenario: Load succeeds
- **WHEN** `AudioEngine.load_sample_async(id, path)` is called with an existing audio file in a supported format
- **THEN** the file is decoded and resampled for playback
- **AND** a WAV copy is written under `./samples/` using the original basename as the primary name
- **AND** `ProjectState.sample_paths[id]` points to that project-local WAV path

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

### Requirement: Unload Sample Slot
The system SHALL expose a Python API to unload a previously loaded sample from a sample slot identified by an integer `id` in the range 0..36.

If `ProjectState.sample_paths[id]` refers to a project-local cached WAV under `./samples/`, the system SHALL attempt to delete that cached file when unloading the pad.

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
- **GIVEN** `ProjectState.sample_paths[id]` points to a cached WAV under `./samples/`
- **AND** that file exists on disk
- **WHEN** `AudioEngine.unload_sample(id)` is called
- **THEN** the cached WAV file is removed from `./samples/`

#### Scenario: Unload ignores missing cached WAV file
- **GIVEN** `ProjectState.sample_paths[id]` points to a cached WAV under `./samples/`
- **AND** that file does not exist on disk
- **WHEN** `AudioEngine.unload_sample(id)` is called
- **THEN** the system does not crash

#### Scenario: Unload missing sample id is handled safely
- **WHEN** `AudioEngine.unload_sample(id)` is called for an `id` with no loaded sample
- **THEN** the request is ignored (or dropped)

#### Scenario: Unload sample id is out of range
- **WHEN** `AudioEngine.unload_sample(id)` is called with `id >= 36`
- **THEN** the call fails with a Python exception

## ADDED Requirements
### Requirement: Project Sample Cache Avoids Silent Overwrites
If writing a cached WAV to `./samples/` would overwrite an existing file with different content, the system SHALL choose a non-colliding filename deterministically (e.g., suffixing a stable identifier) rather than silently overwriting.

#### Scenario: Basename collision is handled without data loss
- **GIVEN** a project already contains `./samples/loop.wav`
- **AND** the user loads a different file whose basename would also be `loop.wav`
- **WHEN** the system writes the project-local cached WAV
- **THEN** the system writes a distinct WAV filename under `./samples/`
- **AND** the original `./samples/loop.wav` is not overwritten
