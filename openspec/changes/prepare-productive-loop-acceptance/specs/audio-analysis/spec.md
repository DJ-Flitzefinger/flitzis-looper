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

### Requirement: Productive Preparation Has Explicit Bounded PCM Admission
The system SHALL retain the normal 512 MiB constant-timing PCM limit and permit
the opt-in productive measurement route to request a source-specific preparation
budget derived from actual loaded geometry, bounded by 1 GiB. It SHALL reject
invalid or excessive budgets before advancing source requests, preserve source
ownership and complete evidence verification, and perform preparation and export
verification outside the realtime callback. The admitted budget SHALL remain
runtime policy for that captured source and SHALL NOT grant timing acceptance
or become authority from saved evidence.

#### Scenario: The complete diagnostic source exceeds the normal preparation budget
- **GIVEN** the unchanged ten-minute diagnostic source loaded at the actual output rate and channel count
- **WHEN** the explicit productive route prepares its actual native evidence
- **THEN** a finite geometry-derived budget can admit the complete source up to the hard ceiling
- **AND** genuine current-source export verifies the same complete PCM under that admitted budget
- **AND** the source is not truncated, replaced or granted acceptance by its memory budget

#### Scenario: A caller requests an invalid or excessive budget
- **GIVEN** an existing loaded source and pending preparation ownership
- **WHEN** a negative, non-integer or above-ceiling preparation budget is requested
- **THEN** preparation rejects it before mutating timing intent or advancing source requests
- **AND** ordinary preparation and saved restoration keep their existing default resource policy

### Requirement: Human Operator Guidance Distinguishes Readiness And Acceptance
The system SHALL explain the audible listening check and recorded timing
measurement, identify preparation readiness separately from an open app window,
and provide concrete manual next steps. It SHALL identify failed preparation
as unusable for acceptance and preserve earlier attempts. Guidance SHALL NOT
claim that restarting a session resets modified project settings or that casual
app use constitutes a completed human test.

#### Scenario: The operator opens the app without a prepared accepted route
- **GIVEN** a human-started measurement app whose preparation fails before READY
- **WHEN** preparation reports its failure
- **THEN** the operator is told that the attempt does not count as a completed test
- **AND** actual device and listening gates remain pending without automatic playback or capture

#### Scenario: The operator has no time for the human test
- **GIVEN** no actual uninterrupted listening or captured output has been assessed
- **WHEN** the operator closes the app without performing the test
- **THEN** guidance permits leaving the human checks pending
- **AND** automated preparation checks cannot replace those human gates

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
- **AND** continuing separately authorized preparation does not declare those gates passed
