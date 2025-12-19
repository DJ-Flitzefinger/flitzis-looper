## ADDED Requirements

### Requirement: Trigger Sample Playback By ID
The system SHALL provide a Python API to trigger playback of a previously loaded sample by integer `id` in the range 0..32, with a floating-point `velocity` in the range 0.0 to 1.0.

#### Scenario: Triggered sample contributes to audio output
- **WHEN** a sample is loaded into slot `id`
- **AND** `AudioEngine.play_sample(id, velocity)` is called
- **THEN** the sample begins playback in the audio callback
- **AND** the rendered output buffer is not forced to silence

#### Scenario: Sample id is out of range
- **WHEN** `AudioEngine.play_sample(id, velocity)` is called with `id >= 32`
- **THEN** the call fails with a Python exception
- **AND** no playback is triggered

#### Scenario: Missing sample ID is handled safely
- **WHEN** `AudioEngine.play_sample(id, velocity)` is called for an `id` with no loaded sample
- **THEN** the trigger is ignored (or dropped)
- **AND** the audio callback continues without panic or blocking

### Requirement: Fixed-Capacity Voice Mixing
The system SHALL mix sample playback using a fixed-capacity voice list with `MAX_VOICES = 32` to avoid heap allocations in the real-time audio callback.

#### Scenario: Polyphony limit is enforced
- **WHEN** more than 32 voices are triggered in a short interval
- **THEN** additional triggers are dropped (or replaced) deterministically
- **AND** the audio callback continues without allocating memory

### Requirement: Real-Time Safety During Playback
The system SHALL ensure that triggering playback and mixing audio in the real-time callback performs no blocking operations and no heap allocations.

#### Scenario: Playback processing stays real-time safe
- **WHEN** the audio callback drains `PlaySample` messages and renders audio
- **THEN** no blocking operations are performed
- **AND** no heap allocations occur