## ADDED Requirements

### Requirement: Stem work observes explicit host headroom
On Windows the controller MUST use distinct measured caller and system available
commit and available physical bytes before original stem resolve/infer/Pair
admission. It MUST require >=2 GiB for both commit measures and >=512 MiB physical;
after pressure or query failure it MUST require >=3 GiB and >=768 MiB to recover.
Sampling SHALL be cached for at most250 ms with fresh worker/command entry checks.
These margins SHALL NOT be reported as model memory estimates or OOM guarantees.
Query failure before admission MUST defer existing metadata without consuming pool
capacity or advancing fairness. Query failure or pressure at queued entry MUST
settle that attempt through existing completion/error ownership before heavy reads.
Unsupported platforms MUST retain the bounded pool with explicit unsupported status.

#### Scenario: Waiting requests retain their original authority
- **WHEN** a valid direct/batch request encounters low headroom or unavailable queries
- **THEN** the same original request remains pending without new source readers or reservations
- **AND** stale/cancel and result/publication/cleanup processing continue
- **AND** recovery resumes original six-lane admission without a second queue or retry registry

#### Scenario: Pressure arrives after pool submission
- **WHEN** queued resolve/infer/Pair work begins with inadequate fresh headroom
- **THEN** it returns an explicit resource failure before source/model/PCM reads
- **AND** existing finally/collector paths retain and release actual custody normally

### Requirement: Resource stops preserve actual process settlement
Active owned stem commands MUST stop when either available commit measure is
<=512 MiB or available physical is <=128 MiB. Failed monitoring MUST stop after
a fixed1-second deadline from the first consecutive failure, without sliding it.
User cancellation MUST retain precedence, and an already completed command result
MUST be observed before a new pressure decision. Resource stops MUST use existing
cooperative abort and owned tree/read retirement, holding worker capacity and source
readers through actual settlement. They MUST bypass Demucs and BS CPU fallback
without setting user subscriber cancellation or killing foreign processes. Actual
secondary read/supervision failure SHALL retain its established diagnostic precedence.

#### Scenario: A running GPU attempt encounters critical pressure
- **WHEN** the monitor observes critical headroom during an owned GPU attempt
- **THEN** the original command settles through its owned process tree and readers
- **AND** its resource failure reaches the original job without a CPU attempt or automatic retry

#### Scenario: Monitoring remains unavailable
- **WHEN** repeated query failures follow the first active failure for at least1 second
- **THEN** the command stops as resource_unavailable with the original fixed deadline
- **AND** a valid intervening observation resets that deadline without weakening recovery margins
