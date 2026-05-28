## MODIFIED Requirements

### Requirement: Lock Mode Changes Affect Playback
The system SHALL make Key Lock state changes affect the Rust audio processing mode for active and
future playback without stopping active voices.

Changing Key Lock SHALL update only bounded audio-thread state. It SHALL preserve the active voice
playhead, loop region, trigger scheduling, BPM-lock tempo ratio, prepared-stem source selection,
gain/EQ state, metering, and playhead reporting. Key Lock SHALL NOT trigger sample reloads, stem
generation, cache inspection, retriggering, transport reanchoring, or Loop Editor grid changes.

#### Scenario: Enabling Key Lock changes processing mode without retrigger
- **GIVEN** a pad is already playing
- **WHEN** the performer enables Key Lock
- **THEN** the Rust audio engine switches that voice to master-tempo processing
- **AND** the voice remains active at its current loop playhead

#### Scenario: Disabling Key Lock returns to varispeed without retrigger
- **GIVEN** a pad is already playing with Key Lock enabled
- **WHEN** the performer disables Key Lock
- **THEN** the Rust audio engine switches that voice to varispeed processing
- **AND** the voice remains active at its current loop playhead
