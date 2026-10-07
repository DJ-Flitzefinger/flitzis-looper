## MODIFIED Requirements

### Requirement: Restore Loads Cached Audio Through The Normal Loader
The system SHALL restore project originals through the same bounded asynchronous
loader as newly selected files, reusing complete validated compatible PCM or
regenerating it from an immutable capture of the project original.

Playback derivatives SHALL retain current engine-format behavior outside the
callback. Decoder/source, playback, device and analyzer dimensions SHALL remain
distinct. Saved-loop residency SHALL not truncate durable full-source duration,
analysis, source zero or accepted evidence. Missing/corrupt originals SHALL retain
the existing unusable-assignment behavior rather than treat PCM as an original.

#### Scenario: Cached audio sample rate mismatch is resampled outside the callback
- **GIVEN** a valid project original differs from the current output format
- **WHEN** the application restores it
- **THEN** a compatible full playback derivative is validated or regenerated off-thread
- **AND** the UI remains usable and the decoder-domain identity is preserved

#### Scenario: Historical acceptance is incompatible with a new derivative
- **WHEN** fresh playback rate, PCM digest, extent or processing provenance no longer matches saved evidence
- **THEN** historical acceptance is not relabelled or replayed as CURRENT
- **AND** fresh matching verification and existing adoption ACK are required

#### Scenario: Saved short loop restores with full metadata
- **WHEN** a saved loop is admitted as a resident window
- **THEN** the original full duration, absolute marker coordinates and complete analysis remain intact
- **AND** resident frame count is represented separately
