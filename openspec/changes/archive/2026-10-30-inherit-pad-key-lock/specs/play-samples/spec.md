## MODIFIED Requirements

### Requirement: Lock Mode Changes Affect Playback
The system SHALL propagate BPM lock and resolved per-pad Key Lock requests from Python to the
audio engine so accepted modes affect playback. A global Key Lock change SHALL alter only
inheriting loaded pads' requests; explicit overrides SHALL retain their requested modes.

#### Scenario: Enabling key lock affects audio processing mode
- **GIVEN** a pad is playing
- **AND** the pad inherits the disabled global Key Lock default
- **WHEN** the performer enables the global default and Native accepts the resolved request
- **THEN** the audio engine updates its processing mode without stopping playback
