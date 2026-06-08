# load-audio-files Specification

## Purpose
To support sample-based playback by loading, decoding, and unloading audio files as immutable in-memory buffers associated with sample slot IDs, without performing disk I/O or decoding in the real-time audio callback.
## Requirements

### Requirement: Load Audio File Into Sample Slot
The system SHALL expose a Python API to load an audio file from a filesystem path into a named sample slot identified by an integer `id` in the range `0..NUM_SAMPLES`.

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
- **AND** `ProjectState.sample_paths[id]` is updated to the new project-local audio path
- **AND** any currently active voices for `id` stop contributing to the audio output

#### Scenario: Sample id is out of range
- **WHEN** `AudioEngine.load_sample_async(id, path)` is called with `id >= NUM_SAMPLES`
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
The system SHALL expose a Python API to unload a previously loaded sample from a sample slot identified by an integer `id` in the range `0..NUM_SAMPLES`.

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

#### Scenario: Unload removes cached audio file
- **GIVEN** `ProjectState.sample_paths[id]` points to a cached audio file under `./samples/`
- **AND** that file exists on disk
- **WHEN** `AudioEngine.unload_sample(id)` is called
- **THEN** the cached audio file is removed from `./samples/`

#### Scenario: Unload ignores missing cached audio file
- **GIVEN** `ProjectState.sample_paths[id]` points to a cached audio file under `./samples/`
- **AND** that file does not exist on disk
- **WHEN** `AudioEngine.unload_sample(id)` is called
- **THEN** the system does not crash

#### Scenario: Unload missing sample id is handled safely
- **WHEN** `AudioEngine.unload_sample(id)` is called for an `id` with no loaded sample
- **THEN** the request is ignored (or dropped)

#### Scenario: Unload sample id is out of range
- **WHEN** `AudioEngine.unload_sample(id)` is called with `id >= NUM_SAMPLES`
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


<!-- Added from add-per-pad-key-lock -->
### Requirement: Unload Clears Per-Pad Key Lock Intent
The system SHALL reset a pad's per-pad Key Lock intent to disabled when audio is unloaded from that pad.

Loading into an empty pad SHALL also clear any stale per-pad Key Lock intent before the new load is scheduled. The system SHALL publish the disabled per-pad Key Lock live default outside the audio callback so a later track loaded into the same pad cannot inherit stale Key Lock state.

#### Scenario: Unload clears pad Key Lock
- **GIVEN** Pad 3 has loaded audio
- **AND** Pad 3 Key Lock is enabled
- **WHEN** the performer unloads audio from Pad 3
- **THEN** Pad 3 has no loaded audio
- **AND** Pad 3's per-pad Key Lock value is disabled
- **AND** the audio engine receives disabled live Key Lock state for Pad 3

#### Scenario: Loading into empty pad clears stale Key Lock
- **GIVEN** Pad 3 has no loaded audio
- **AND** stale project data has Pad 3 Key Lock enabled
- **WHEN** the performer loads new audio into Pad 3
- **THEN** Pad 3's per-pad Key Lock value is disabled before the new load is scheduled
- **AND** the audio engine receives disabled live Key Lock state for Pad 3


<!-- Added from prepare-realtime-callback-safety -->
### Requirement: Loaded Sample Replacement Retires Old Audio Handles Outside The Callback
The system SHALL keep loaded sample replacement and unload operations real-time safe when old
audio handles are released.

When a loaded sample slot is replaced or unloaded in the audio callback, any old full-mix sample
handle and associated prepared-stem handles SHALL be moved to bounded non-audio cleanup instead
of being deallocated directly on the callback thread.

#### Scenario: Replacing a loaded sample defers old handle cleanup
- **GIVEN** a sample slot already contains a loaded full-mix buffer
- **WHEN** a new loaded sample publication for the same slot reaches the audio callback
- **THEN** the old buffer handle is retired through non-audio cleanup
- **AND** the new buffer becomes the slot's loaded full-mix source
- **AND** the callback performs no disk I/O, blocking wait, logging, Python/GIL access, neural
  inference, plugin loading, or large audio-payload deallocation


<!-- Added from reset-pad-settings-on-unload -->
### Requirement: Unload Resets Track-Bound Pad Settings
The system SHALL reset track-bound per-pad project settings to their default values when audio is
unloaded from a pad.

Track-bound settings SHALL include the pad's sample path, sample duration, analysis result, manual
BPM override, manual key override, Gain/Trim, low/mid/high EQ, loop start, loop end, auto-loop
state, auto-loop bar count, grid offset samples, stem cache metadata, stem mix preference, and
per-pad Key Lock intent.

The system SHALL publish neutral live defaults for pad BPM, Gain/Trim, EQ, loop region, and
per-pad Key Lock outside the audio callback so a later track loaded into the same pad does not
inherit stale live audio state.

The system MUST NOT reset global project settings, input mappings, selected pad/bank, sidebar
visibility, or other non-track-bound UI preferences as part of unloading a pad.

#### Scenario: Unload clears persisted track settings
- **GIVEN** pad `id` has loaded audio
- **AND** pad `id` has non-default Gain/Trim, EQ, grid offset, loop, manual BPM, and manual key
  settings
- **WHEN** the performer unloads audio from pad `id`
- **THEN** `ProjectState.sample_paths[id]` is `None`
- **AND** the track-bound per-pad settings for `id` are reset to their default values

#### Scenario: Later load starts from pad defaults
- **GIVEN** pad `id` is empty
- **AND** the persisted config still contains stale track-bound settings for `id`
- **WHEN** the performer loads a new audio file into pad `id`
- **THEN** the stale track-bound settings for `id` are cleared before the new load is scheduled
- **AND** the newly loaded track starts from default pad Gain/Trim, EQ, grid offset, loop, BPM, and
  key settings


<!-- Added from tolerate-imperfect-mp3-loads -->
### Requirement: Tolerate Imperfect MP3 Metadata And Frames
The system SHALL load supported MP3 files that contain decodable audio even when track-level
metadata is incomplete or isolated MP3 frames are malformed.

The loader SHALL derive source channel count and sample rate from decoded audio buffers when that
metadata is absent from the probed track.

The loader SHALL skip isolated recoverable decode errors and continue decoding later packets when
at least one valid audio frame can still be decoded.

The loader MUST reject a file when no decodable audio frames are found, or when decoded buffers for
one selected stream change sample rate or channel count mid-stream.

Decoding tolerance SHALL remain outside the audio callback and MUST NOT add disk I/O, blocking
waits, logging, Python/GIL access, neural inference, plugin loading, or unbounded work to the
real-time path.

#### Scenario: MP3 missing track channel metadata still loads
- **GIVEN** an MP3 file has no channel count in the probed track metadata
- **AND** decoding its audio packets yields buffers with a stable channel count and sample rate
- **WHEN** `AudioEngine.load_sample_async(id, path)` loads the file
- **THEN** the file is decoded and resampled for playback
- **AND** the loaded sample uses the decoded buffer channel count for channel mapping

#### Scenario: MP3 with isolated malformed frame still loads
- **GIVEN** an MP3 file contains at least one malformed packet or frame
- **AND** later packets still decode to valid audio buffers for the selected stream
- **WHEN** `AudioEngine.load_sample_async(id, path)` loads the file
- **THEN** the loader skips the recoverable decode error
- **AND** the file is decoded and resampled for playback from the usable audio frames

#### Scenario: MP3 with no decodable frames fails
- **GIVEN** an MP3 file has no packets that decode to usable audio buffers
- **WHEN** `AudioEngine.load_sample_async(id, path)` loads the file
- **THEN** the load fails
- **AND** no sample slot state is modified

