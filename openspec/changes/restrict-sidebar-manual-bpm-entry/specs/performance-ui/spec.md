## MODIFIED Requirements

### Requirement: Manual BPM Entry In Selected-Pad Sidebar
The system SHALL provide a manual BPM entry control in the left sidebar when a pad is selected and
has audio loaded.

The manual BPM entry control SHALL accept only ASCII digits `0` through `9`, `.`, and `,` at input
time. The control SHALL normalize `,` to `.` before it appears in the field, and disallowed typed
characters SHALL leave the field unchanged. Committed numeric BPM values below 0.5 SHALL apply as
0.5 BPM, and committed numeric BPM values above 400 SHALL apply as 400 BPM. Clearing the field SHALL
remove the manual BPM override.

This manual entry SHALL remain Python/UI control-plane behavior and SHALL NOT add disk I/O,
Python/GIL access, logging, blocking work, heavy allocation, neural inference, or any new work to
the Rust audio callback.

#### Scenario: Invalid character is ignored
- **GIVEN** a pad is selected and has audio loaded
- **AND** the manual BPM entry contains `120`
- **WHEN** the performer types `Ü`
- **THEN** the entry still contains `120`

#### Scenario: Comma input is normalized
- **GIVEN** a pad is selected and has audio loaded
- **WHEN** the performer types `120,5` in the manual BPM entry
- **THEN** the entry buffer becomes `120.5`
- **AND** committing the entry sets the selected pad's manual BPM to 120.5

#### Scenario: Manual BPM entry clamps below minimum
- **GIVEN** a pad is selected and has audio loaded
- **WHEN** the performer commits `0` in the manual BPM entry
- **THEN** the selected pad's manual BPM becomes 0.5

#### Scenario: Manual BPM entry clamps above maximum
- **GIVEN** a pad is selected and has audio loaded
- **WHEN** the performer commits `500` in the manual BPM entry
- **THEN** the selected pad's manual BPM becomes 400.0

#### Scenario: Clearing the BPM removes the manual override
- **GIVEN** a pad is selected and has audio loaded
- **AND** the pad currently has a manual BPM
- **WHEN** the performer clears the BPM value in the sidebar control
- **THEN** the selected pad's manual BPM becomes unset
