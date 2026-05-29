## MODIFIED Requirements

### Requirement: Load Audio File Into Sample Slot
The system SHALL expose a Python API to load an audio file from a filesystem path into a named sample slot identified by an integer `id` in the range `0..NUM_SAMPLES`.

Before the loaded sample is considered part of the current project, the system SHALL copy the original audio file into the project's `./samples/` directory.

The persisted sample path stored in `ProjectState.sample_paths[id]` SHALL refer to the project-local audio file under `./samples/`.

If a file with the same basename already exists in `./samples/`, the system SHALL choose a non-colliding filename by appending `_0`, `_1`, etc., rather than overwriting the existing file.

The decoder SHALL support loading at least: WAV, FLAC, MP3, AIFF (`.aif`/`.aiff`), and OGG.

For MP3 files, the decoder SHALL preserve the decoded source timeline across isolated recoverable packet decode errors by inserting silence for a bad packet when the stream channel layout and packet duration are known.

#### Scenario: Load succeeds
- **WHEN** `AudioEngine.load_sample_async(id, path)` is called with an existing audio file in a supported format
- **THEN** the file is decoded and resampled for playback
- **AND** the original audio file is copied under `./samples/` using the original basename as the primary name (with numeric suffix if needed to avoid collision)
- **AND** `ProjectState.sample_paths[id]` points to that project-local audio file path

#### Scenario: Recoverable MP3 packet error preserves timeline
- **GIVEN** an MP3 file contains an isolated recoverable packet decode error after stream channels are known
- **AND** the damaged packet has a known packet duration
- **WHEN** the file is decoded for loading
- **THEN** the decoded buffer includes silence for that packet duration
- **AND** later decoded audio keeps its source-frame position instead of shifting earlier

#### Scenario: File format is unsupported
- **WHEN** `AudioEngine.load_sample_async(id, path)` is called with a file that cannot be decoded
- **THEN** the call fails with a Python exception
- **AND** no sample slot state is modified
