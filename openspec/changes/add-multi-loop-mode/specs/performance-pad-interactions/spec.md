## MODIFIED Requirements

### Requirement: Trigger and Retrigger Pads
The system SHALL allow the performer to trigger or retrigger pads from the performance pad grid using left mouse button down.

#### Scenario: Left mouse down triggers a loaded pad
- **WHEN** a sample is loaded into the pad’s sample slot
- **AND** the performer presses the left mouse button down on the pad
- **THEN** the system triggers playback from the start of the sample

#### Scenario: Left mouse down retriggers deterministically
- **WHEN** a sample is currently playing for a pad
- **AND** the performer presses the left mouse button down on the same pad
- **THEN** the system stops playback for that pad
- **AND** the system triggers playback again from the start

### Requirement: Stop Pads Quickly
The system SHALL stop a pad using a right mouse button down on the pad.

#### Scenario: Right mouse down stops the pad
- **WHEN** a sample is currently playing for a pad
- **AND** the performer presses the right mouse button down on the pad
- **THEN** playback for that pad stops promptly
