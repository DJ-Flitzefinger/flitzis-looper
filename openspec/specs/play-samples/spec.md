# play-samples Specification

## Purpose
To enable real-time-safe triggering, looping playback, and mixing of previously loaded samples by ID (with velocity) from Python, with bounded polyphony to avoid allocations in the audio callback.
## Requirements
### Requirement: Trigger Sample Playback By ID
The system SHALL provide a Python API to trigger playback of a previously loaded sample by integer `id` in the range 0..36, with a floating-point `velocity` in the range 0.0 to 1.0. Triggered playback SHALL loop continuously over the full sample buffer until stopped via `AudioEngine.stop_sample(id)` (or the sample is unloaded; see `load-audio-files`).

#### Scenario: Triggered sample contributes to audio output
- **WHEN** a sample is loaded into slot `id`
- **AND** `AudioEngine.play_sample(id, velocity)` is called
- **THEN** the sample begins playback in the audio callback
- **AND** the rendered output buffer is not forced to silence

#### Scenario: Triggered sample loops continuously
- **WHEN** a sample is loaded into slot `id`
- **AND** `AudioEngine.play_sample(id, velocity)` is called
- **AND** playback reaches the end of the sample buffer
- **THEN** playback continues from the start of the sample buffer without requiring a new trigger

#### Scenario: Sample id is out of range
- **WHEN** `AudioEngine.play_sample(id, velocity)` is called with `id >= 36`
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

### Requirement: Stop All Sample Playback
The system SHALL provide a Python API `AudioEngine.stop_all()` that stops all currently active voices, regardless of sample ID.

#### Scenario: Stop-all ends all active voices
- **WHEN** one or more samples are playing due to prior `play_sample(...)` calls
- **AND** `AudioEngine.stop_all()` is called
- **THEN** all currently active voices stop contributing to the audio output

#### Scenario: Stop-all is safe when nothing is playing
- **WHEN** no samples are currently playing
- **AND** `AudioEngine.stop_all()` is called
- **THEN** the call succeeds

#### Scenario: Stop-all before engine initialization fails
- **WHEN** an `AudioEngine` has not been initialized via `run()`
- **AND** `AudioEngine.stop_all()` is called
- **THEN** the call fails with a Python exception

