## ADDED Requirements

### Requirement: Global Speed Multiplier Changes Playback Rate
The system SHALL apply the stored global speed multiplier to playback such that changing `AudioEngine.set_speed(speed)` changes audible playback rate for all active voices.

#### Scenario: Speed change is audible for active pads
- **GIVEN** a pad is playing
- **WHEN** Python calls `AudioEngine.set_speed(0.8)`
- **THEN** the pad’s playback tempo slows down audibly

### Requirement: Lock Mode Changes Affect Playback
The system SHALL propagate BPM lock and Key lock state changes from Python to the audio engine so that these modes affect playback.

#### Scenario: Enabling key lock affects audio processing mode
- **GIVEN** a pad is playing
- **WHEN** the performer enables Key lock
- **THEN** the audio engine updates its processing mode without stopping playback

### Requirement: Per-pad BPM Metadata Is Available To The Audio Engine
The system SHALL publish per-pad effective BPM metadata to the audio engine so it can tempo-match pads when BPM lock is enabled.

#### Scenario: Audio engine receives BPM updates
- **GIVEN** a pad has updated effective BPM metadata
- **WHEN** the controller publishes metadata to the audio engine
- **THEN** subsequent playback processing uses that BPM when BPM lock is enabled
