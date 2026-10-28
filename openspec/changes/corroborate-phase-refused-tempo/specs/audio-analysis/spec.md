## ADDED Requirements

### Requirement: Same-period acoustic support may supply a phase-refused scalar
The system SHALL preserve successful neural scalar results unchanged. An
otherwise eligible, complete, nonconflicting and unambiguous neural method failing
only its existing phase check MAY use an independently passing same-source QM
scalar with explicit joint provenance. It SHALL retain the failed neural check,
use no octave folding and preserve existing numerical guards.

#### Scenario: Near-complete neural lattice with phase-only refusal
- **WHEN** only the neural phase condition fails and independent same-source QM passes its own unchanged checks with at most5ms whole-common-extent period drift
- **THEN** the system MAY publish the exact admitted QM scalar through schema6
- **AND** it SHALL preserve the neural failure and identify QM CPU execution separately from neural inference

#### Scenario: Incompatible or unavailable corroboration
- **WHEN** neural evidence is ambiguous, locally conflicting, limited, ineligible, or fails another check, or QM fails source/period/support/budget checks
- **THEN** this feature SHALL NOT replace the neural result
- **AND** optional acoustic failure SHALL NOT erase key outcomes or become a fabricated successful method

#### Scenario: Source changes during opaque acoustic work
- **WHEN** the request/source becomes stale or cancelled while QM runs
- **THEN** the existing source/request publication fences SHALL refuse late metadata
- **AND** cancellation SHALL remain distinct from unavailable analysis
- **AND** the system SHALL NOT claim cooperative cancellation inside QM

### Requirement: Joint evidence remains bounded scalar metadata
The system SHALL admit all simultaneous source, preparation, fitting and
serialization capacities within512MiB. It SHALL preserve complete source extent
and existing source custody, frame/rate, OwnACK, grid and realtime authority.

#### Scenario: Copy or restore of joint evidence
- **WHEN** schema6 is parsed, copied or restored
- **THEN** validation SHALL independently replay each backend's declared numeric evidence and recompute source/extents, phase-only admission, exact selected value and same-period drift
- **AND** it SHALL NOT rerun neural inference, QM DSP or discovery
- **AND** old schemas SHALL reject the joint field even when null and retain identical omitted-field serialization
- **AND** joint tempo evidence SHALL grant no musical-quarter, source-phase or playback acknowledgement
