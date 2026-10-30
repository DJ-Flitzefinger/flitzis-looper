## ADDED Requirements

### Requirement: Long remix residency uses actual available memory
The system SHALL admit live resident PCM according to the actual incremental
allocation, measured host headroom and outstanding allocation promises, with
automatic admission as the default. It SHALL NOT reject a sealed-reader live
range solely because its duration, bar count, final PCM or retained old/new PCM
exceeds the former fixed 512-MiB default or 1-GiB transient scratch cutoff.

#### Scenario: Sparse long vocal remix has sufficient memory
- **GIVEN** a long vocal range and independently repeating short bass/drum pads
- **WHEN** the actual prospective allocation fits available physical and caller/system commit headroom after operating margin and pending promises
- **THEN** automatic residency admission SHALL permit that range through the existing source/component preparation and own-ACK path
- **AND** immutable backing, real old readers and selected physical geometry SHALL remain truthful
- **AND** the number of hypothetically occupied pads SHALL NOT impose an unrelated cap

#### Scenario: Real memory pressure or unavailable observation
- **WHEN** a positive new allocation exceeds measured usable headroom or its required host observation fails
- **THEN** admission SHALL return a recoverable reason before allocating or changing current audio authority
- **AND** the previous usable source, loop, components and history SHALL remain valid
- **AND** no active owner SHALL be evicted and no shorter loop SHALL be substituted

### Requirement: One reservation owner separates physical allocation and scoped ownership
The existing resident ledger SHALL atomically retain checked scope-local unique
ownership costs and simultaneous pending allocation promises. Host admission
SHALL consider shared promises across engine scopes and SHALL charge only actual
new physical PCM, independently of the existing scope-local ownership cost.

#### Scenario: Concurrent workers and exact shared reuse
- **WHEN** parallel workers reserve new ranges or another scope adopts compatible existing immutable PCM
- **THEN** outstanding new allocations SHALL NOT independently consume the same observed available headroom
- **AND** exact reuse SHALL be resolved before charging hypothetical new component arrays
- **AND** already allocated reused PCM SHALL NOT be charged again as new host allocation
- **AND** zero-allocation reuse SHALL remain possible under pressure without bypassing scope or owner-capacity validation

#### Scenario: Reservation settlement and real last reader
- **WHEN** a request cancels, fails, becomes stale or transfers its reservation to actual readers
- **THEN** its pending promise SHALL settle exactly once through the existing lifetime owner
- **AND** actual old/new/history readers SHALL remain accounted until their distinct final use
- **AND** arithmetic overflow and capacity failures SHALL refuse without changing current authority

### Requirement: Final live PCM is separate from bounded preparation scratch
Sealed complete-reader live range preparation SHALL account final source and
component arrays as resident ownership. Encoded chunks, streaming scratch and
genuine legacy whole-conversion temporary PCM SHALL retain their separate
bounded admission. Existing two workers,32 queued jobs, startup capacity,
timing/analysis limits and realtime constraints SHALL remain effective.

#### Scenario: Large declared extent and legacy converter
- **WHEN** a sealed disk reader describes a range larger than the former transient cutoff
- **THEN** checked final geometry and actual resident admission SHALL decide the request without allocating the whole disk asset as scratch
- **AND** a genuine legacy complete-array converter SHALL NOT gain unbounded temporary allocation from this change
- **AND** no callback disk access, blocking, host query or new worker lane SHALL be introduced
