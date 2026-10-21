## ADDED Requirements

### Requirement: Asynchronous source map verification retains original queued custody
The system MUST provide nonblocking map verification admission through the
existing cold worker lane and offline map reservation. Lane reservation SHALL
precede accepted map admission. Queue-full/stopped refusal MUST preserve source,
selection and playback request state. No additional worker pool or job registry
SHALL be introduced.

Queued and running work MUST retain its exact original source readers and shared
offline reservation until actual native retirement, including caller Drop,
explicit cancellation, source supersession, failed submission, worker failure and
shutdown/discarded queued closures. Cancellation requested MUST NOT be reported as
terminal retirement. Concise status/error polling SHALL perform no parsing,
hashing, inference, complete map serialization or joining work.

The existing lane's retained admission SHALL be released exactly once. Running
cleanup SHALL finish on its owning worker before that worker can claim another
job; never-started discarded work MAY retire off-thread while retaining its queued
admission. A terminal status SHALL require both actual reader cleanup and the
original lane reservation's retirement. Observation receipts MUST NOT retain or
create another admission, and the two-worker/32-queued bounds MUST remain valid.

Map work SHALL preserve source numeric requests, logical source-request
incarnations and preparation epochs. Existing direct map and ordinary analysis
contracts SHALL remain valid. Heavy verification and retirement SHALL remain off
the audio callback and heavy parsing/hashing SHALL not run on the UI thread.

#### Scenario: Both workers are occupied
- **WHEN** map verification is admitted to the same existing queued lane
- **THEN** admission returns before either occupied worker is released
- **AND** cancellation or handle release retains the original readers and offline reservation until the queued work truly retires

#### Scenario: Existing worker capacity is exhausted or stopped
- **WHEN** asynchronous map admission cannot reserve the original lane
- **THEN** it refuses without consuming offline capacity or advancing source authority
- **AND** existing source and scheduled playback remain valid

#### Scenario: Queued or active work is cancelled during shutdown
- **WHEN** cancellation/shutdown removes or completes its original worker closure
- **THEN** the exact captured readers and both existing reservations retire once after actual work ends
- **AND** no stale map completion is delivered or model/export work started

### Requirement: A map job resolves only its own genuine completion
The system MUST bind map completion resolution to the original map job's exact
NativeAnalysisOwner admission and current source/request/cancellation ownership.
The existing LoaderEventDelivery SHALL retain the genuine immutable map payload
privately. Resolution SHALL return that payload only for the matching map job and
actual completion kind, respecting pending placement and later invalidation.

Mutable Python dictionary fields, equal numeric requests, map revisions, source
digests or an unrelated genuine delivery MUST NOT authorize a substituted map.
Resolution SHALL grant data only, with no project selection or audio authority.

#### Scenario: Sequential map jobs share a source request
- **WHEN** one job is given another job's genuine completion delivery
- **THEN** resolution refuses it even if numeric requests, source and map revision match
- **AND** each uncancelled current job can resolve only its own private payload

#### Scenario: A caller substitutes mutable event data
- **WHEN** Python event fields are replaced with another map or identity
- **THEN** original-job resolution returns only the privately retained genuine payload or refuses
- **AND** a progress/non-map delivery cannot supply a map

#### Scenario: The original content moves or becomes stale
- **WHEN** resolution overlaps actual Move/Pair acknowledgement and control mirroring
- **THEN** it waits or refuses according to the existing original owner rather than a destination slot
- **AND** later cancellation, request supersession, equal-byte replacement or another engine cannot revive the old payload
