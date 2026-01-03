## ADDED Requirements

### Requirement: Per-pad gain control
The system SHALL provide a per-pad gain parameter for each pad sample slot id in the range `0..NUM_SAMPLES`.

The per-pad gain MUST be a finite floating-point value in the range `0.0..=1.0`.

The per-pad gain SHALL default to unity (`1.0`) for all pads.

#### Scenario: Default gain is unity
- **GIVEN** the application has started
- **WHEN** a pad has no explicitly changed gain
- **THEN** the pad’s effective gain is `1.0`

### Requirement: Per-pad gain is editable from the left sidebar
When a pad is selected, the system SHALL render a gain control in the left sidebar that edits the selected pad’s per-pad gain.

#### Scenario: Adjusting gain updates playback
- **GIVEN** pad `id` is selected and is currently playing
- **WHEN** the performer changes the pad gain
- **THEN** subsequent audio output for that pad reflects the new gain

### Requirement: Per-pad gain is applied during mixing
The audio engine SHALL apply the per-pad gain when mixing voices for a pad, in addition to per-trigger velocity and global master volume.

#### Scenario: Gain composes with velocity and master volume
- **GIVEN** pad `id` is playing with trigger velocity `v`
- **AND** master volume is `m`
- **AND** pad gain is `g`
- **WHEN** the mixer renders audio
- **THEN** the pad’s contribution is scaled by approximately `v * m * g`
