## MODIFIED Requirements

### Requirement: Trigger Sample Playback By ID
The system SHALL provide a Python API to trigger playback of a previously loaded sample by integer `id` in the range 0..36, with a floating-point `velocity` in the range 0.0 to 1.0.

#### Scenario: Triggered sample contributes to audio output
- **WHEN** a sample is loaded into slot `id`
- **AND** `AudioEngine.play_sample(id, velocity)` is called
- **THEN** the sample begins playback in the audio callback
- **AND** the rendered output buffer is not forced to silence

#### Scenario: Sample id is out of range
- **WHEN** `AudioEngine.play_sample(id, velocity)` is called with `id >= 36`
- **THEN** the call fails with a Python exception
- **AND** no playback is triggered

#### Scenario: Missing sample ID is handled safely
- **WHEN** `AudioEngine.play_sample(id, velocity)` is called for an `id` with no loaded sample
- **THEN** the trigger is ignored (or dropped)
- **AND** the audio callback continues without panic or blocking

## ADDED Requirements

### Requirement: Stop Sample Playback By ID
The system SHALL provide a Python API to stop playback of a previously triggered sample by integer `id` in the range 0..36.

#### Scenario: Stop ends active voices for the sample id
- **WHEN** a sample is playing due to one or more prior `play_sample(id, ...)` calls
- **AND** `AudioEngine.stop_sample(id)` is called
- **THEN** all currently active voices for `id` stop contributing to the audio output

#### Scenario: Stop sample id is out of range
- **WHEN** `AudioEngine.stop_sample(id)` is called with `id >= 36`
- **THEN** the call fails with a Python exception

#### Scenario: Stop missing sample id is handled safely
- **WHEN** `AudioEngine.stop_sample(id)` is called for an `id` with no loaded sample
- **THEN** the stop request is ignored (or dropped)
- **AND** the audio callback continues without panic or blocking
