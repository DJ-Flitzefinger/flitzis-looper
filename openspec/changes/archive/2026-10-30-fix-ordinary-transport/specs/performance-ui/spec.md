## ADDED Requirements

### Requirement: Independent Lock and ordinary transport controls

GUI and MIDI SHALL expose the same independent BPM Lock intent with MultiLoop
OFF or ON. Ordinary START/STOP SHALL follow the authoritative effective explicit
target; editor focus SHALL NOT substitute for that history. Mode requests SHALL
distinguish refused admission and accepted pending work from completed effects.

#### Scenario: Lock remains available in either mode
- **WHEN** MultiLoop is OFF or ON
- **THEN** BPM Lock SHALL remain independently toggleable through GUI and MIDI
- **AND** changing MultiLoop SHALL NOT silently replace its established reference

#### Scenario: Recording retains its group start
- **WHEN** the performer starts recording through its existing group gate
- **THEN** that group SHALL retain its source-bound global capture semantics
- **AND** sorted group iteration SHALL NOT create an explicit PAD target

## MODIFIED Requirements

### Requirement: MultiLoop Toggle Control
The system SHALL provide a MultiLoop toggle control in the right performance sidebar directly below Key Lock. The control group SHALL appear in the order Key Lock, Multi Loop, BPM Lock, retain the stable MultiLoop tag and use the original controller/Learn actions. BPM Lock SHALL remain independently available with MultiLoop OFF or ON through GUI and MIDI. Source SYNC, REALTIME BPM and their editor controls SHALL be absent.

#### Scenario: MultiLoop control is visible
- **WHEN** the UI is started
- **THEN** a MultiLoop control is visible in the performance view

#### Scenario: Timing controls follow the requested hierarchy
- **WHEN** the UI is started
- **THEN** Key Lock, Multi Loop and BPM Lock SHALL appear in that order in the right sidebar
- **AND** the old bottom-bar MultiLoop button SHALL NOT be duplicated

#### Scenario: MultiLoop control indicates enabled state
- **WHEN** MultiLoop mode is enabled
- **THEN** the MultiLoop control is visually indicated as enabled
- **WHEN** MultiLoop mode is disabled
- **THEN** the MultiLoop control is visually indicated as disabled

### Requirement: BPM Lock Anchors Master BPM To The Current Pad When Enabled
When the performer explicitly enables BPM lock, the system SHALL capture the currently selected qualified source as the lock reference and derive the master BPM from that source when available. Selection, ordinary starts/retriggers and MultiLoop transitions SHALL preserve that same source owner and the established master tempo. Hierarchy-only requalification SHALL NOT re-anchor the master timeline. An empty captured reference MAY bind its first qualified content; later replacement SHALL NOT rebind a successor merely by slot.

#### Scenario: Enabling BPM lock captures master BPM from selected pad and speed
- **GIVEN** Pad 1 is selected
- **AND** Pad 1 has an effective BPM value
- **AND** the current global speed is 1.25×
- **WHEN** the performer enables BPM lock
- **THEN** the system sets the master BPM to approximately `Pad1_bpm * 1.25`
