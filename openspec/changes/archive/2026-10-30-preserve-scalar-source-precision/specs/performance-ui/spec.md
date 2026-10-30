## MODIFIED Requirements

### Requirement: BPM Display Manual Entry
The system SHALL allow the performer to double-click the right-side BPM display and type a
finite positive target BPM with fractional precision preserved by the binary64 control value.

The entry SHALL initialize from the full effective displayed tempo, retain digits and one
decimal separator, interpret comma as period and ignore disallowed typed characters.
An untouched entry SHALL NOT publish a speed update on Enter, close or focus loss.
A deliberate valid commit SHALL use the existing BPM reference and bounded speed multiplier;
invalid or non-positive entries SHALL NOT update speed. Compact display rounding SHALL be
identified as presentation, with full effective precision available in the edit/tooltip.

This control-plane behavior SHALL add no IO, GIL access, logging, blocking, allocation,
inference or new work to the Rust audio callback. Existing native speed/BPM representation
and bounds remain in force.

#### Scenario: Untouched target entry preserves speed
- **GIVEN** the effective displayed tempo has more precision than its compact display
- **WHEN** the performer opens and submits or leaves the entry without editing its text
- **THEN** no speed update is published
- **AND** the effective tempo is preserved

#### Scenario: Comma input retains fractional precision
- **GIVEN** the BPM entry is active
- **WHEN** the performer types `123,456abc`
- **THEN** the entry buffer becomes `123.456`
- **AND** a deliberate commit targets 123.456 BPM within the existing speed bounds

#### Scenario: Invalid target is ignored
- **WHEN** the performer commits an empty, zero or non-positive target
- **THEN** the speed multiplier is not updated
