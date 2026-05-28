## ADDED Requirements

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
