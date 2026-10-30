## ADDED Requirements

### Requirement: Saved startup controls precede asynchronous restore admission
Ordinary project reopening SHALL finish saved runtime control and timing initialization before admitting restored source workers. Each restore SHALL capture its original source request, epoch, proposal, initial pitch, resolved Key Lock and resident selection after that initialization. Startup SHALL NOT invalidate an admitted restore by replaying its own saved BPM, origin or timing intent. Genuine later replacement, epoch/request/proposal invalidation, explicit cancellation and shutdown MUST remain authoritative. This ordering SHALL remain on the existing control path without callback I/O, blocking or allocation. Before that projection, startup SHALL reuse the existing loader path-validation policy to neutralize missing ordinary assignments. Recovery-held missing references and their durable intent SHALL remain protected, but SHALL NOT contribute saved PAD controls or a BPM/master anchor until a valid source candidate exists. The validated path classification SHALL NOT replace later Native path/source checks or confer new source authority. Standalone restore SHALL retain the same validation and cleanup policy. The original restore payload SHALL carry its complete validated saved gain and EQ scalars, including neutral values, into the existing Native source claim before acknowledgement. Mixing initialization MUST NOT rely on delivery of earlier empty-slot parameter messages or recapture a later replacement source. These optional public load settings SHALL require explicit restore intent and a complete finite in-range gain and three-band EQ tuple; malformed or partial options MUST reject before work reservation or authority mutation. Current-source PAD gain, EQ and other gestures using the shared readiness guard SHALL remain unavailable while that PAD's own restored assignment receipt is pending. Rejection SHALL be visible through the existing GUI or mapped-input error path before durable intent changes, and SHALL NOT queue a later replay. A fresh gesture after receipt projection SHALL use the normal current-source path.

#### Scenario: Manual, tap or legacy timing initializes before cold preparation

- **GIVEN** a saved original with legacy BPM/grid or manual/tap timing intent
- **WHEN** the project reopens and the cold worker may run immediately upon admission
- **THEN** the saved control and timing initialization has already finished
- **AND** the same original restore can prepare, publish and receive its actual Native acknowledgement without startup-generated cancellation
- **AND** no relaxed epoch or source guard is needed

#### Scenario: Missing ordinary or recovery-held original at startup

- **GIVEN** a saved original path is missing or unsupported when the project reopens
- **WHEN** startup classifies source paths before saved control projection
- **THEN** an ordinary missing assignment is neutralized before its old gain, EQ, timing or master anchor can be projected
- **AND** a recovery-held missing assignment retains its original saved intent and deferred ownership without contributing PAD controls or the master anchor
- **AND** valid unrelated sources can initialize their controls and enter normal Cold admission

#### Scenario: Source-bound pitch and Automatic timing keep their original owners

- **GIVEN** a restored PAD with saved pitch, resolved Key Lock and a finite resident selection
- **WHEN** startup initializes runtime controls and admits that source
- **THEN** the source load still initializes the saved pitch and resolved Key Lock atomically with its original selection
- **AND** Automatic timing remains unresolved until genuine source acknowledgement and fresh saved-evidence verification
- **AND** no stale manual BPM or grid is applied to replacement content

#### Scenario: Cold publication precedes saved empty-slot parameter consumption

- **GIVEN** saved gain and EQ initialization was queued for the original empty slot
- **WHEN** Cold source publication advances the lifetime before those parameter messages are consumed
- **THEN** the successful original source claim initializes the exact saved mixing tuple before its real acknowledgement
- **AND** stale empty-slot parameters cannot override that source or replacement content
- **AND** neutral saved gain and EQ also replace earlier effective mixing

#### Scenario: A PAD mixing gesture arrives while its original restore is pending

- **GIVEN** a restored PAD still awaits projection of its own source acknowledgement
- **WHEN** the performer changes its gain or EQ through GUI or mapped input
- **THEN** the existing readiness boundary reports a pending-source refusal
- **AND** saved intent and the original restore payload remain unchanged without a delayed gesture replay
- **AND** a fresh gesture after that receipt is projected can use the current acknowledged source

#### Scenario: A genuinely superseded restore cannot revive an old source

- **GIVEN** an original restore captured after startup initialization
- **WHEN** a later source, epoch, request or proposal invalidates it or shutdown occurs
- **THEN** that original restore remains rejected through the existing authority checks
- **AND** startup ordering does not recapture newer authority or retry stale work
