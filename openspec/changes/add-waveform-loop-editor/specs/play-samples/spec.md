## MODIFIED Requirements

### Requirement: Trigger Sample Playback By ID
The system SHALL provide a Python API to trigger playback of a previously loaded sample by integer `id` in the range 0..36, with a floating-point `velocity` in the range 0.0 to 1.0.

Triggered playback SHALL loop continuously until stopped via `AudioEngine.stop_sample(id)` (or the sample is unloaded; see `load-audio-files`).

If a loop region is configured for `id` (see `loop-region`), playback SHALL start at the loop start and SHALL loop over the configured loop region.

If no loop region is configured, playback SHALL start at the start of the sample buffer and SHALL loop over the full sample buffer.

#### Scenario: Triggered sample contributes to audio output
- **WHEN** a sample is loaded into slot `id`
- **AND** `AudioEngine.play_sample(id, velocity)` is called
- **THEN** the sample begins playback in the audio callback
- **AND** the rendered output buffer is not forced to silence

#### Scenario: Triggered sample loops continuously
- **WHEN** a sample is loaded into slot `id`
- **AND** `AudioEngine.play_sample(id, velocity)` is called
- **AND** playback reaches the end of the active loop region (or end of sample if no region)
- **THEN** playback continues from the loop start (or sample start if no region) without requiring a new trigger

#### Scenario: Sample id is out of range
- **WHEN** `AudioEngine.play_sample(id, velocity)` is called with `id >= 36`
- **THEN** the call fails with a Python exception
- **AND** no playback is triggered

#### Scenario: Missing sample ID is handled safely
- **WHEN** `AudioEngine.play_sample(id, velocity)` is called for an `id` with no loaded sample
- **THEN** the trigger is ignored (or dropped)
- **AND** the audio callback continues without panic or blocking
