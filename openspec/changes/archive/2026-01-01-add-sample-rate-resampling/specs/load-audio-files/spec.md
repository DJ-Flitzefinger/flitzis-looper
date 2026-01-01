## MODIFIED Requirements
### Requirement: Load Audio File Into Sample Slot
The system SHALL expose a Python API to load an audio file from a filesystem path into a named sample slot identified by an integer `id` in the range 0..36. The decoder SHALL support loading at least: WAV, FLAC, MP3, AIFF (`.aif`/`.aiff`), and OGG. If the audio file has a sample rate that differs from the audio output device's sample rate, the system SHALL automatically resample the audio to match the output device's sample rate using a high-quality algorithm.

#### Scenario: Load succeeds with sample rate conversion
- **WHEN** `AudioEngine.load_sample(id, path)` is called with an existing audio file in a supported format with a different sample rate than the output device
- **THEN** the file is decoded and resampled to match the output device's sample rate
- **AND** the resampled buffer is associated with `id` for subsequent playback

#### Scenario: Load succeeds with matching sample rate
- **WHEN** `AudioEngine.load_sample(id, path)` is called with an existing audio file in a supported format with the same sample rate as the output device
- **THEN** the file is decoded into an immutable, in-memory sample buffer
- **AND** the buffer is associated with `id` for subsequent playback
- **AND** no resampling is performed (optimization)

#### Scenario: Loading replaces an already-loaded sample
- **WHEN** a sample is already loaded into slot `id`
- **AND** `AudioEngine.load_sample(id, path)` is called and succeeds
- **THEN** the buffer associated with `id` is replaced by the newly loaded buffer (with resampling if needed)
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

## REMOVED Requirements
### Requirement: Sample rate mismatch error
**Reason**: Sample rate mismatches are now handled automatically through resampling
**Migration**: Remove error handling for `SampleLoadError::SampleRateMismatch`
