## ADDED Requirements

### Requirement: Real-time Time-stretch And Pitch-shift In Mixer
The system SHALL perform time-stretch and pitch-shift in real-time inside the Rust mixer for each active voice, using Signalsmith Stretch via the `signalsmith_dsp` crate.

#### Scenario: Mixer produces output using stretch processing
- **GIVEN** a pad is playing a looping sample
- **WHEN** the audio callback renders output
- **THEN** the voice’s contribution is generated through the time-stretch/pitch-shift processor
- **AND** the callback remains real-time safe (no blocking)

### Requirement: Global Speed Updates Affect All Active Voices In Real-time
The system SHALL apply changes to the global speed control to all currently active voices without requiring retriggering.

#### Scenario: Slider drag updates tempo for active voices
- **GIVEN** two pads are currently playing
- **WHEN** the performer changes global speed from 1.0× to 1.25×
- **THEN** both pads’ playback tempo changes audibly
- **AND** no pad requires retriggering to adopt the new tempo

### Requirement: Key Lock Preserves Pitch During Tempo Changes
When Key lock is enabled, the system SHALL prevent tempo changes from changing the perceived musical pitch of playing audio.

#### Scenario: Tempo increases without pitch increase
- **GIVEN** a pad is playing
- **AND** Key lock is enabled
- **WHEN** global speed increases
- **THEN** the pad’s perceived pitch remains approximately constant

### Requirement: BPM Lock Tempo-matches Pads Using BPM Metadata
When BPM lock is enabled and a master BPM has been selected, the system SHALL tempo-match pads using their effective BPM metadata.

#### Scenario: Pads with different BPM follow a common global BPM
- **GIVEN** Pad A has effective BPM 120
- **AND** Pad B has effective BPM 90
- **AND** BPM lock is enabled with master BPM 120
- **WHEN** global speed is 1.0×
- **THEN** Pad A uses a tempo ratio near 1.0×
- **AND** Pad B uses a tempo ratio near 120/90

### Requirement: BPM Lock Degrades Gracefully Without BPM Metadata
If BPM lock is enabled but a pad lacks BPM metadata, the system SHALL continue playback and fall back to non-BPM-matched behavior.

#### Scenario: Missing BPM falls back to global speed
- **GIVEN** BPM lock is enabled
- **AND** a pad is playing with unknown BPM
- **WHEN** global speed changes
- **THEN** the pad continues playing
- **AND** the pad follows the global speed multiplier as a fallback

### Requirement: No Heap Allocations In Audio Callback
The system SHALL NOT perform heap allocations during the audio callback while applying time-stretch and pitch-shift.

#### Scenario: Callback remains allocation-free
- **WHEN** the mixer renders audio with time-stretch enabled
- **THEN** no heap allocations occur
