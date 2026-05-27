## MODIFIED Requirements

### Requirement: Per-pad EQ is editable from the left sidebar
The system SHALL preserve the selected-pad left-sidebar low, mid, and high EQ controls while
routing their accepted live targets to the Rust DSP-chain isolator node.

Manual EQ value entry SHALL accept finite typed values in the supported performer-facing EQ range,
including valid negative values such as `-6`, `-6.0`, and `-60`. The input filter SHALL allow an
optional leading `-`, digits, `.`, and `,`; typed comma SHALL be converted to `.` before insertion.
The input filter SHALL reject invalid characters before they appear in the edit field. Committed
manual values SHALL be clamped to the supported EQ range before they are stored or published to
Rust.

The UI SHALL preserve the existing neutral reset gesture for each band. Keyboard and MIDI mappings
for per-pad EQ SHALL continue to use stable action semantics outside the audio callback, derive
bounded controller-owned target changes, and publish accepted targets through the bounded
parameter path.

#### Scenario: Manual EQ entry accepts negative values
- **GIVEN** the performer is manually editing the selected pad's high EQ value
- **WHEN** the performer types `-6.0` and commits the field
- **THEN** the selected pad's high EQ value becomes `-6.0 dB`
- **AND** the accepted live target is published through the existing bounded parameter path

#### Scenario: Manual EQ entry accepts full kill
- **GIVEN** the performer is manually editing an EQ value
- **WHEN** the performer types `-60` and commits the field
- **THEN** the committed value is accepted as the band kill value

#### Scenario: Manual EQ entry rejects invalid characters before insertion
- **GIVEN** the performer is manually editing an EQ value
- **WHEN** the performer types `U+00DC`
- **THEN** the character is rejected before it appears in the field
- **WHEN** the performer types `,`
- **THEN** `.` appears in the field instead

