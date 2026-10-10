## MODIFIED Requirements

### Requirement: Trigger and Retrigger Pads
The system SHALL trigger or retrigger loaded pads on left mouse-down using existing loop-start or sample-start rules when Re-Arrange is off, and SHALL let arrangement gesture ownership consume pad-surface presses first when it is on.

#### Scenario: Left mouse down triggers a loaded pad
- **GIVEN** Re-Arrange is off
- **WHEN** a sample is loaded into the pad's sample slot
- **AND** the performer presses the left mouse button down on the pad
- **THEN** the system triggers playback from the pad's loop start when configured
- **AND** otherwise triggers playback from the start of the sample

#### Scenario: Left mouse down retriggers deterministically
- **GIVEN** Re-Arrange is off
- **WHEN** a sample is currently playing for a pad
- **AND** the performer presses the left mouse button down on the same pad
- **THEN** the system stops playback for that pad
- **AND** the system triggers playback again using the same loop/sample start-point rules

#### Scenario: Arrangement left mouse down never triggers before drag
- **WHEN** Re-Arrange is on and left drag begins onA
- **THEN** no normal trigger/stop occurs, even before movement threshold
- **AND** empty target means Move and occupied target means Swap with clear preview

### Requirement: Stop Pads Quickly
The system SHALL stop a pad promptly on right mouse-down when Re-Arrange is off and SHALL consume right arrangement drag before normal stop when it is on.

#### Scenario: Right mouse down stops the pad
- **GIVEN** Re-Arrange is off
- **WHEN** a sample is currently playing for a pad
- **AND** the performer presses the right mouse button down on the pad
- **THEN** playback for that pad stops promptly

#### Scenario: Existing held-right stop behavior remains in normal mode
- **WHEN** Re-Arrange is off and the existing hovered held-right stop path observes A
- **THEN** it retains its existing bounded stop semantics
- **AND** the arrangement gesture owner consumes that path first only while its mode is active

#### Scenario: Right drag copies without individual overwrite dialog
- **WHEN** Re-Arrange is on and right dragA ends on a valid empty/occupied target
- **THEN** it creates stopped Copy or Copy-overwrite without extra single-pad confirmation
- **AND** source stays unchanged and preview identifies target/operation unambiguously

## ADDED Requirements

### Requirement: Re-Arrange mode has explicit transient state and gesture help
The system SHALL provide a Re-Arrange toggle preferably bottom-left with clear warning color and short gesture help, default it OFF after restart and keep MIDI performance usable while active.

#### Scenario: Mode and drag release boundaries
- **WHEN** mode enables, a drag cancels/self-drops/uses invalid or empty source, or releases over bank hover
- **THEN** no normal pad click/bank action leaks and no no-op drop changes state
- **AND** MIDI continues; restart returns modeOFF without altering musical assignments
