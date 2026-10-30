## ADDED Requirements

### Requirement: Complete capture survives additive accepted interpretation
The system MUST support an additive version-2 `source-beatmap-pwa-v2` accepted
envelope containing the existing source and accepted sections and a Raw section
with its canonical wrapper revision and the complete original Raw capture.
Original ODF binary64 bits, downbeat indices, detector/config/input provenance,
original capture revision and observations outside the source MUST be retained.
The original capture, Raw-wrapper and complete Map revisions SHALL remain
distinct and validated. Map identity SHALL bind the entire capture and accepted
state. Existing strict version-1 records and refusals MUST remain compatible.

Version-2 editing, undo, storage, Copy/Bank and fresh verified restore MUST retain
the complete capture without inference or a new persistent Raw reference. Source
and accepted numeric/physical guards and existing size/array limits SHALL remain.
Initial interpretation MUST refuse outside Raw observations as anchors. Later
explicit translations and accepted virtual references SHALL retain the existing
version-1 geometry contract. Physical-domain checks MUST remain independent;
virtual coordinates SHALL NOT grant readable source indices or SYNC eligibility.

#### Scenario: A capture includes additional and outside-source detections
- **WHEN** an explicit subset is interpreted, stored, edited and restored
- **THEN** every original detection bit and downbeat index remains in the embedded capture
- **AND** only physically usable explicitly labelled anchors define the accepted map

#### Scenario: An older strict record is loaded
- **WHEN** a version-1 record is parsed or edited
- **THEN** its version and existing numeric, provenance and physical refusals remain valid

#### Scenario: Explicit alignment translates a V2 map across the physical origin
- **WHEN** the existing whole-map translation moves accepted anchors outside physical source bounds
- **THEN** V2 retains the same explicit virtual geometry and exact Undo as V1 without changing its complete capture
- **AND** out-of-source queries cannot gain physical source or playback authority

### Requirement: Explicit interpretation uses original observations without count invention
The system MUST construct initial accepted geometry from a genuine validated
capture and explicit quarter-coordinate labels through one Native builder and
the existing map validator/evaluator. The bounded review intent SHALL contain
only its schema, original capture revision and labelled Raw indices with exact
binary64 quarter bits. It SHALL NOT accept supplied source positions or musical
verification flags. At least two chronological physically usable detections and
strictly increasing finite quarter coordinates MUST be required.

No ordinal index arithmetic, implicit neighboring label or default 4/4 SHALL
invent quarter count or musical qualification. Initial intervals MUST be
uncertain, meter unknown and phase unavailable; position estimates MUST remain
distinguishable from reviewed positions. Independent count, position, coverage,
meter and phase edits SHALL remain separate. The shared data builder SHALL
remain usable by later genuinely qualified automatic evidence; this review
client MUST NOT replace or claim that separate full-Goal gate.

#### Scenario: Explicit labels skip extra detector events
- **WHEN** the user supplies non-ordinal quarter labels for a chronological subset
- **THEN** Native uses those exact coordinates and original source positions without filling labels
- **AND** unselected evidence is preserved and unknown meter/coverage is not promoted

#### Scenario: A label is missing, nonfinite, repeated or reversed
- **WHEN** interpretation lacks a valid strict pair of coordinate sequences
- **THEN** no accepted selection is published and the previous state remains valid

### Requirement: Accepted build owns independent original custody after admission
The system MUST admit interpretation only from the actual opaque current Raw
candidate under matching engine, original content/placement, source generation
and capture revision. The controller SHALL additionally fence its exact Raw
attempt. Digest equality, a replaced source or mutable Python data MUST NOT
substitute original authority.

A successfully admitted accepted job MUST retain the genuine Raw artifact reader
and lease until its embedded accepted bytes and independent parse own the data.
It SHALL capture a fresh original source/cancellation owner, use the existing
bounded worker/offline reservations and freshly verify actual source custody.
After admission, old Raw discard/cancel MUST NOT revoke the new accepted job or
delivery. Accepted results SHALL retain neither old Raw cancellation authority
nor a shared Raw allocation without the corresponding genuine lifetime.

Failure, queued/running cancel, arm/submit refusal, source supersession and handle
drop MUST retire actual original readers and reservations once off realtime.
The new build's simultaneous allocations MUST be checked before growth, including
retained input, serialization and reparse. Existing immutable artifact, receipt,
private completion and durable-before-selection paths SHALL remain authoritative.

#### Scenario: Raw is discarded after valid accepted admission
- **WHEN** the accepted worker still owns the genuine Raw reader
- **THEN** Raw retirement waits for that reader while the new job can finish and select independently
- **AND** later source replacement still invalidates the job through its own original owner

#### Scenario: A source or identical-digest attempt is replaced before admission
- **WHEN** an old review action reaches the builder
- **THEN** original candidate and attempt fences refuse without accepted intent or reservation changes
