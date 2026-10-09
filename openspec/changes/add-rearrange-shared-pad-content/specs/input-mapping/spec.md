## ADDED Requirements

### Requirement: Fixed slot mappings and admitted content effects remain distinct
The system SHALL keep fixed pad trigger/stop/selection MIDI bindings at their controller slots while already admitted selected-content actions carry their captured immutable lifetime through Move/Swap or are fenced on removal.

#### Scenario: Future fixed input versus old admitted action
- **WHEN** A moves from slotS toT and B occupiesS
- **THEN** future fixedS input targets its current occupantB but prior admittedA action followsA
- **AND** Copy neither moves nor duplicates layout bindings or waveform hold tokens
