## ADDED Requirements

### Requirement: Device Evidence Preparation Uses Productive Current Authority
The system SHALL provide an explicitly invoked human-run evidence preparation
route using actual loaded sources, existing accepted publication and genuine
current native acknowledgement. It SHALL preserve complete source and accepted
revision identities, loop/rate/control configuration and output/device clock
observations, and SHALL identify unavailable or estimated fields explicitly.
Comparison authority SHALL require actual effective voice/source/loop/rate state
matching current accepted ownership; Python intent SHALL NOT replace that state.

#### Scenario: Normal analysis has not established accepted timing
- **GIVEN** a loaded pad with legacy automatic analysis but no current accepted acknowledgement
- **WHEN** measurement evidence is prepared
- **THEN** the pad is not labeled as using the accepted musical loop trajectory
- **AND** explicit existing preparation/publication or verified restoration must establish current authority before accepted-path evidence is usable

#### Scenario: The human starts an isolated measurement session
- **GIVEN** private workspace configuration and explicit independently assessed evidence
- **WHEN** the human invokes the measurement route and operates the application
- **THEN** the existing productive application controllers and native ownership checks execute
- **AND** evidence observation performs no automatic playback or capture and introduces no realtime I/O, unbounded scans, hashes, logging or Python access
- **AND** an explicit observation request permits only a fixed-capacity voice lookup and bounded scalar publication in the callback

#### Scenario: A playing voice retains an older source than the current bank
- **GIVEN** a pinned playing voice and replacement current pad metadata
- **WHEN** a demand-triggered bounded effective-state observation is collected
- **THEN** comparison cannot relabel the old voice using the replacement current accepted revision
- **AND** unavailable, multiple or mismatched effective voices are explicit evidence blockers

### Requirement: Recorded Features Retain Independent Clock And Error Domains
The system SHALL compare actual captured signal features with explicit feature
associations and independently declared clock calibration, preserving capture,
source, accepted revision, physical/musical loop and processing identities.
Unwrapped musical error SHALL retain the one-loaded-frame limit at 75 and 1000
cycles separately from output sampling, seam offsets and audible DSP alignment.

#### Scenario: Recorder and output clocks have no established relationship
- **GIVEN** a real capture with nominal WAV sample rate but no independent clock relationship
- **WHEN** recorded recurrence is compared with the intended productive period
- **THEN** the result remains inconclusive for source-equivalent drift
- **AND** nominal rate equality does not certify musical or audible alignment

#### Scenario: Synthetic capture fixtures validate tooling
- **GIVEN** hardware-free synthetic audio and an explicitly synthetic run identity
- **WHEN** feature extraction and comparison pass their checks
- **THEN** the report retains synthetic provenance and open actual device/listening gates
- **AND** schema validity or a numerical result cannot become human acceptance

### Requirement: Sustained Listening Evidence Remains A Human Gate
The system SHALL provide a private human listening record binding the actual
productive run and capture, observer, uninterrupted listening duration of at
least 1800 seconds, audible observations and explicit human decision. Tooling
SHALL validate record consistency without inventing listening or accepting G3.

#### Scenario: No actual listening record is supplied
- **GIVEN** passing hardware-free and recorded feature checks without sustained human evidence
- **WHEN** the preparation packet is delivered
- **THEN** device and listening acceptance remain explicit open gates
- **AND** cache/residency planning and implementation cannot begin on that evidence alone
