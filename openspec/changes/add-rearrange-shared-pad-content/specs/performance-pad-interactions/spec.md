## MODIFIED Requirements

### Requirement: Trigger and Retrigger Pads
The system SHALL trigger or retrigger loaded pads on left mouse-down using existing loop-start or sample-start rules when Re-Arrange is off, and SHALL let arrangement gesture ownership consume pad-surface presses first when it is on.

#### Scenario: Normal left mouse down starts or restarts
- **WHEN** Re-Arrange is off and left mouse-down targets loadedA
- **THEN** normal trigger/retrigger starts at loop start when configured, otherwise sample start

#### Scenario: Normal playing-pad retrigger retains deterministic stop then start
- **GIVEN** Re-Arrange is off and A already plays
- **WHEN** left mouse-down targets A
- **THEN** existing bounded semantics stop its prior playback and trigger again from normal loop/sample start

#### Scenario: Arrangement left mouse down never triggers before drag
- **WHEN** Re-Arrange is on and left drag begins onA
- **THEN** no normal trigger/stop occurs, even before movement threshold
- **AND** empty target means Move and occupied target means Swap with clear preview

### Requirement: Stop Pads Quickly
The system SHALL stop a pad promptly on right mouse-down when Re-Arrange is off and SHALL consume right arrangement drag before normal stop when it is on.

#### Scenario: Normal right click stops
- **WHEN** Re-Arrange is off and playingA receives right mouse-down
- **THEN** A stops through existing native behavior

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
