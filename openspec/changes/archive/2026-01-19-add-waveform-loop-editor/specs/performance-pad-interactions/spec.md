## MODIFIED Requirements

### Requirement: Trigger and Retrigger Pads
The system SHALL allow the performer to trigger or retrigger pads from the performance pad grid using left mouse button down.

When a pad has a configured loop region (see `loop-region`), triggering the pad SHALL start playback from the loop start.

When a pad has no configured loop region, triggering the pad SHALL start playback from the start of the sample.

#### Scenario: Left mouse down triggers a loaded pad
- **WHEN** a sample is loaded into the pad’s sample slot
- **AND** the performer presses the left mouse button down on the pad
- **THEN** the system triggers playback from the pad’s loop start when configured
- **AND** otherwise triggers playback from the start of the sample

#### Scenario: Left mouse down retriggers deterministically
- **WHEN** a sample is currently playing for a pad
- **AND** the performer presses the left mouse button down on the same pad
- **THEN** the system stops playback for that pad
- **AND** the system triggers playback again using the same start-point rules
