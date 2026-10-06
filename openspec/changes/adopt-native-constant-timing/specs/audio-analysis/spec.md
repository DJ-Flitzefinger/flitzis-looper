## ADDED Requirements

### Requirement: Explicit Native Acceptance Uses Actual Loaded Pad Evidence
The system SHALL capture actual loaded source/PCM/timebase/generation/request in
an opaque ticket and execute lossless QM preparation outside realtime processing.
It SHALL freshly construct acceptance with explicit independent quarter counts,
timing-error, origin and acceptance assertions, without inferring these from fit
quality or promoting existing timing numbers.

#### Scenario: The performer has not supplied independent musical acceptance
- **GIVEN** an ordinary loaded pad with legacy BPM or a prepared QM raw sequence
- **WHEN** no independent count and named acceptance assertion is supplied
- **THEN** no accepted timing record is published
- **AND** ordinary loading and manual/TAP behavior remain available

#### Scenario: Actual loaded PCM differs from a caller snapshot
- **GIVEN** a caller retains matching metadata for an earlier source
- **WHEN** the native source has been replaced before preparation or publication
- **THEN** native current ownership controls validity
- **AND** the caller snapshot cannot authorize the earlier result

### Requirement: Native Timing Adoption Rejects Retired Ownership
The system SHALL connect TimingAdoptionGuard to actual native source/request/
intent and recheck ownership after preparation, at enqueue and mixer adoption.
Requests, source changes, unload, cancellation and timing edits SHALL revoke
pending work. Manual/Tap/Legacy SHALL block automatic adoption; returning to
Automatic SHALL NOT revive tickets. Failure/full queues SHALL preserve effective
state and counters SHALL reject wrap.

#### Scenario: A timing edit occurs while preparation runs
- **GIVEN** accepted-timing preparation for an actual loaded pad
- **WHEN** a manual, TAP, legacy or same-value timing edit supersedes its intent
- **THEN** the prepared result cannot publish
- **AND** the previously effective audio state is retained

#### Scenario: A newer request wins after enqueue
- **GIVEN** a valid pending accepted-timing publication
- **WHEN** a new request or source change occurs before mixer adoption
- **THEN** the callback rejects the pending publication using bounded checks
- **AND** acknowledgement reports rejection rather than availability

#### Scenario: A legacy result was queued before a new accepted request
- **GIVEN** a completed normal analysis or current-source load with retired timing
- **WHEN** native polling delivers its terminal event after a newer request
- **THEN** matching source/task bookkeeping can settle without replaying legacy timing
- **AND** automatic grid/loop initialization and restore replay cannot revoke the newer timing
- **AND** replaced-source events are skipped without stopping the remaining event drain

#### Scenario: Publication cannot reserve queue capacity
- **GIVEN** a valid current preparation ticket and previously effective timing
- **WHEN** the command ring is full or a required identity cannot advance
- **THEN** the publication fails without changing effective timing or wrapping identities

### Requirement: Native SourceGrid Consumes Acknowledged Exact Accepted Timing
The system SHALL publish fixed full-revision/binary64-period/signed-origin
metadata and use the period directly in live SourceGrid after mixer acceptance.
It SHALL distinguish pending from accepted, prevent older legacy updates from
overriding later acceptance, retire large owners outside the callback, and
preserve physical endpoints, source zero and active source progression.

#### Scenario: Fractional timing cannot be represented through binary32 BPM
- **GIVEN** explicitly accepted source-bound timing with a fractional binary64 period
- **WHEN** the mixer adopts it and evaluates the native source grid
- **THEN** the grid uses the accepted period and signed origin directly
- **AND** complete accepted revision identity is retained even for equal-valued records

#### Scenario: Legacy metadata was queued before precise publication
- **GIVEN** an older BPM or origin update awaiting callback application
- **WHEN** a later accepted publication becomes effective
- **THEN** the older update cannot replace its accepted period or origin
- **AND** a successful later explicit timing edit revokes accepted authority

#### Scenario: A publication has only reached the command ring
- **GIVEN** a current accepted record enqueued for callback adoption
- **WHEN** the callback has not yet accepted it
- **THEN** feedback reports pending status
- **AND** it does not report the new timing as effective

### Requirement: Current Accepted Timing Is Native Acknowledged Authority
The system SHALL resolve current accepted timing from actual native loaded-source
ownership and callback acknowledgement, retaining the complete accepted revision,
exact period and signed origin. Historical ticket feedback SHALL NOT substitute
for current pad authority. Pending replacement SHALL leave the previous effective
record observable; successful source or timing-intent replacement SHALL revoke it.

#### Scenario: An accepted ticket becomes historical
- **GIVEN** a callback-acknowledged accepted record for a loaded pad
- **WHEN** the source is unloaded or replaced, or a Manual, Tap or Legacy edit succeeds
- **THEN** current-pad resolution no longer exposes that record
- **AND** historical ticket feedback cannot revive it

#### Scenario: A newer automatic record is still pending
- **GIVEN** a current acknowledged accepted record and a valid replacement
- **WHEN** the replacement is enqueued but has not been acknowledged
- **THEN** current resolution retains the previous effective revision
- **AND** it exposes the replacement only after actual callback adoption

### Requirement: Native Period And Rate Consumers Preserve Binary64 Timing
The system SHALL use acknowledged accepted seconds-per-quarter directly for
native source timing, transport anchoring/bootstrap, output-clock quantization
and BPMLOCK, converting legacy public BPM only at admission without a binary32
roundtrip. It SHALL derive the playback target once as source period divided by
master output period, preserving binary64 rates and fractional source epochs.

#### Scenario: Accepted timing differs from a binary32 BPM projection
- **GIVEN** an acknowledged nonintegral binary64 source period and master output period
- **WHEN** production bootstrap, quantization and BPMLOCK rendering consume them
- **THEN** transport and the shared output clock retain the exact master period
- **AND** SourcePlayback uses one binary64 source-to-output-period ratio
- **AND** callback partitioning does not introduce a second rate or source epoch

#### Scenario: The requested BPMLOCK rate exceeds its supported range
- **GIVEN** a source period and master output period whose ratio requires clipping
- **WHEN** the native renderer applies its supported speed limit
- **THEN** the master period remains the requested authoritative period
- **AND** the clipped physical progression does not claim sustained musical SYNC

### Requirement: Pitch ABI And Physical Wrap Preserve The Shared Trajectory
The system SHALL use explicit Rubber Band pitch conversion at its ABI boundary
without changing the shared source trajectory or existing physical-loop wrap
policy. Rate clipping SHALL NOT redefine the authoritative master period.

#### Scenario: Key Lock follows the shared binary64 source rate
- **GIVEN** an acknowledged period-driven source trajectory
- **WHEN** the native Key Lock adapter receives its inverse-rate pitch scale
- **THEN** the existing native double ABI retains binary64 pitch conversion
- **AND** pitch update thresholds, source epoch and physical wrap policy remain unchanged
