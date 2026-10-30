## ADDED Requirements

### Requirement: Scheduled Launch Messages Retain Bounded Input Time
The system SHALL retain optional captured input timestamps as fixed-size fields through ordered
play commands and their scheduled launch events.

Timestamp propagation SHALL preserve command ordering, bounded ring and scheduler capacity,
existing atomic loop-region-plus-play publication checks, and caller-visible queue failures.
Native mapped loop-region-plus-play publication failure SHALL NOT partially publish that sequence.
Timestamp retries SHALL NOT recapture input time. The audio
callback SHALL NOT acquire locks, allocate, access Python or perform unbounded work to retain time.

#### Scenario: Scheduler keeps captured time
- **WHEN** a stamped play command enters the output-frame scheduler
- **THEN** its scheduled launch event retains the same optional timestamp
- **AND** its scheduled execution frame follows the unchanged current launch policy

#### Scenario: Full queue preserves fallback provenance
- **GIVEN** the command queue cannot accept a native mapped launch sequence
- **WHEN** native dispatch reports failure
- **THEN** no partial loop-plus-play sequence is published
- **AND** the fallback event retains the captured input timestamp
