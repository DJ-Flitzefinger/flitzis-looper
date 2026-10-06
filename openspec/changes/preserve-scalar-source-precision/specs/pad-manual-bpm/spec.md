## ADDED Requirements

### Requirement: BPM Presentation Preserves Unedited Effective Value
The system SHALL preserve the full effective BPM when its edit field is opened, submitted,
closed or loses focus without an intentional text edit.

Editable presentation SHALL disclose available effective precision rather than implying that
a compact rounded value is the exact tempo. Deliberate manual edits, TAP and clear SHALL retain
their existing authority, including genuine fractional tempos and legacy restored metadata.

#### Scenario: Untouched rounded BPM field
- **GIVEN** effective BPM 120.00128936767578 and no manual override
- **WHEN** the performer enters and leaves the edit field without changing its text
- **THEN** the effective BPM remains 120.00128936767578
- **AND** no manual 120 override is introduced

#### Scenario: Intentional fractional override
- **WHEN** the performer intentionally enters manual BPM 119.999 or 123.45
- **THEN** the effective control value retains the entered fractional tempo
- **AND** clearing manual BPM restores the detected value
