## ADDED Requirements

### Requirement: Stopped stem candidate preflight matches the stopped effect
A valid stopped PAD's own new committed stem set SHALL be validated against the transition-free selection that the callback will install. An absent active voice SHALL NOT create an outgoing ramp-reader requirement. Source, candidate, publication, launch and complete-set authority SHALL remain mandatory. Paused voices SHALL retain their real ramp readers and Native continuity requirements.

#### Scenario: A stopped selected V1 admits its own V2
- **GIVEN** a stopped PAD retains a valid V1 stem selection and a distinct current V2 candidate
- **WHEN** its own capture and component preparation reach callback adoption
- **THEN** V2 residency is accepted without requiring a nonexistent outgoing V1 ramp
- **AND** no voice starts or output acknowledgement is fabricated
- **AND** later playback consumes V2 through its own existing launch authority

#### Scenario: A paused V1 remains a real ramp participant
- **GIVEN** a paused voice retains V1 source and Native input
- **WHEN** its own V2 candidate is adopted
- **THEN** both required reader identities and retained Native continuity are validated
- **AND** the source-frame ramp waits for real consumption
