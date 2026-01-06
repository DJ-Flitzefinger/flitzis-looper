# load-audio-files Specification

## Purpose
To support sample-based playback by loading, decoding, and unloading audio files as immutable in-memory buffers associated with sample slot IDs, without performing disk I/O or decoding in the real-time audio callback.

## ADDED Requirements

### Requirement: Store Original Audio Files In Project Cache
The system SHALL copy the original audio file into the project's `./samples/` directory instead of creating a resampled WAV file.

The cached file SHALL preserve the original file's format and extension (e.g., `.mp3`, `.flac`, `.wav`).

#### Scenario: Load stores original file in cache
- **WHEN** `AudioEngine.load_sample_async(id, path)` is called with an existing audio file in a supported format
- **THEN** the file is decoded and resampled for playback
- **AND** the original file is copied to `./samples/` using the original basename and extension
- **AND** `ProjectState.sample_paths[id]` points to that project-local path

## MODIFIED Requirements

### Requirement: Load Audio File Into Sample Slot
Before the loaded sample is considered part of the current project, the system SHALL copy the original audio file into the project's `./samples/` directory.

The persisted sample path stored in `ProjectState.sample_paths[id]` SHALL refer to the project-local copy of the original audio file under `./samples/`.

#### Scenario: Load succeeds
- **WHEN** `AudioEngine.load_sample_async(id, path)` is called with an existing audio file in a supported format
- **THEN** the file is decoded and resampled for playback
- **AND** the original file is copied to `./samples/` using the original basename and extension
- **AND** `ProjectState.sample_paths[id]` points to that project-local path

### Requirement: Unload Sample Slot
If `ProjectState.sample_paths[id]` refers to a project-local cached file under `./samples/`, the system SHALL attempt to delete that cached file when unloading the pad.

#### Scenario: Unload removes cached original file
- **GIVEN** `ProjectState.sample_paths[id]` points to a cached file under `./samples/`
- **AND** that file exists on disk
- **WHEN** `AudioEngine.unload_sample(id)` is called
- **THEN** the cached file is removed from `./samples/`

## ADDED Requirements

### Requirement: Project Sample Cache Avoids Silent Overwrites
If copying an audio file to `./samples/` would overwrite an existing file with the same basename, the system SHALL choose a non-colliding filename by appending a numeric suffix (e.g., `_0`, `_1`) rather than silently overwriting.

#### Scenario: Basename collision is handled without data loss
- **GIVEN** a project already contains `./samples/loop.mp3`
- **AND** the user loads a different file whose basename would also be `loop.mp3`
- **WHEN** the system copies the file to the project cache
- **THEN** the system writes a distinct filename under `./samples/` (e.g., `loop_0.mp3`)
- **AND** the original `./samples/loop.mp3` is not overwritten

## REMOVED Requirements

### Requirement: WAV-Specific Caching Requirements
The requirement that "the cached WAV encoding MUST use the same sample format as the engine's in-memory sample buffers" is removed.

The requirement that "If writing a cached WAV to `./samples/` would overwrite an existing file with different content, the system SHALL choose a non-colliding filename deterministically (e.g., suffixing a stable identifier)" is replaced by the new collision handling requirement above.
