## MODIFIED Requirements

### Requirement: Lock Mode Changes Affect Playback
The system SHALL propagate BPM lock and Key Lock state changes from Python to the audio engine so that these modes affect active playback without stopping or retriggering voices.

When Key Lock changes while voices are active, the Rust audio path SHALL update bounded mode and backend state only. The change SHALL NOT reload samples, regenerate stems, reanalyze audio, reset transport scheduling, or perform disk I/O in the audio callback.

#### Scenario: Enabling Key Lock affects active audio processing mode
- **GIVEN** a pad is playing
- **WHEN** the performer enables Key Lock
- **THEN** the audio engine updates its Rubber Band Key Lock processing mode without stopping playback
- **AND** the pad is not retriggered

#### Scenario: Disabling Key Lock returns to varispeed without retriggering
- **GIVEN** a pad is playing with Key Lock enabled
- **WHEN** the performer disables Key Lock
- **THEN** the audio engine returns the active voice to varispeed playback
- **AND** the pad continues from its current source-frame playhead

### Requirement: Trigger Sample Playback By ID
The system SHALL provide a Python API to trigger playback of a previously loaded sample by integer `id` in the range `0..NUM_SAMPLES`, with a floating-point `velocity` in the range `0.0` to `1.0`.

Triggered playback SHALL loop continuously until stopped via `AudioEngine.stop_sample(id)` or until the sample is unloaded. If Key Lock is enabled when a voice starts or restarts, that voice SHALL use isolated Rubber Band state so prior voice audio cannot leak into the new trigger.

#### Scenario: Retrigger resets Rubber Band voice state
- **GIVEN** a pad has already played with Key Lock enabled
- **WHEN** the pad is triggered again
- **THEN** the new active voice uses reset or newly isolated Rubber Band state
- **AND** old shifted output from the previous trigger is not mixed into the new playback

#### Scenario: Triggered sample loops continuously under Rubber Band Key Lock
- **WHEN** a sample is loaded into slot `id`
- **AND** Key Lock is enabled
- **AND** `AudioEngine.play_sample(id, velocity)` is called
- **AND** playback reaches the end of the active loop region
- **THEN** playback continues from the loop start without requiring a new trigger
- **AND** Rubber Band processing remains bounded across the loop wrap

### Requirement: Stop Sample Playback By ID
The system SHALL provide a Python API to stop playback of a previously triggered sample by integer `id` in the range `0..NUM_SAMPLES`.

Stopping or unloading a sample SHALL clear or retire active per-voice Rubber Band state without leaking stale shifted output into future playback and without performing heap allocation, disk I/O, logging, blocking waits, Python/GIL access, or plugin work in the audio callback.

#### Scenario: Stop clears active Rubber Band output
- **GIVEN** a sample is playing with Key Lock enabled
- **WHEN** `AudioEngine.stop_sample(id)` is called
- **THEN** all active voices for `id` stop contributing to audio output
- **AND** their Rubber Band shifted-output buffers are no longer audible in later triggers

#### Scenario: Unload clears active Rubber Band output
- **GIVEN** a sample is playing with Key Lock enabled
- **WHEN** the sample is unloaded
- **THEN** active voices for that sample stop
- **AND** their Rubber Band state is cleared, reset, or retired without callback-unsafe work
