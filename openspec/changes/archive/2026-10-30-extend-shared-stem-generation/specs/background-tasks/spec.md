## ADDED Requirements

### Requirement: Committed Stem Admission Preserves Native Placement Pending
Committed stem residency admission SHALL distinguish an actual in-progress
Native content-placement publication from missing, stale, retired or invalid
source/selection authority. Read-only preflight SHALL use the existing Native
placement observation or a stable placement read token; an odd or changed token
SHALL defer the original demand through the existing bounded admission path.
No transient classification SHALL grant source, selection, publication or ACK
authority, and final admission SHALL retain every original currentness guard.

#### Scenario: Another PAD publishes while committed residency is requested
- **GIVEN** a PAD retains its own current saved selection and registration ACK
- **WHEN** another PAD holds or changes the Native placement publication marker during a read-only source or control preflight
- **THEN** admission reports the declared placement-pending protocol and the original demand remains pending
- **AND** it acquires no replacement target, loses no admitted receipt and adds no blocking wait on the control or audio thread
- **AND** a later fresh admission rechecks the original source, content and selection before requiring its own residency ACK

#### Scenario: Stable invalid authority remains terminal
- **WHEN** the same read-only preflight has a stable placement token but missing, stale, retired, foreign or unavailable source or selection authority
- **THEN** the original failure remains terminal rather than being classified from an ambiguous error string as transient pressure

#### Scenario: Own stopped residency ACK retires its admission-pressure feedback
- **GIVEN** the original explicit ALL demand retains a declared recoverable admission-pressure message
- **WHEN** normal residency polling observes its own current accepted registration and residency ACK for the exact source, content, saved set and mask
- **THEN** the matching admission-pressure feedback SHALL clear in that observation frame even if stopped audio still reports effective mode waiting
- **AND** the effective audio state SHALL remain waiting until its actual effect is observed
- **AND** this observation SHALL neither submit new work nor borrow a stale, rejected, foreign or mismatched ACK
- **AND** terminal failures and newer feedback owned by another operation SHALL remain visible
- **AND** unresolved preload withdrawal SHALL retain its pressure feedback, and renewed preload SHALL require its own enabled-preload ACK rather than borrowing a previous withdrawal ACK

#### Scenario: Queued pressure preserves a replacement set or mask desire
- **GIVEN** an existing accepted ALL receipt belongs to a previous set or mask
- **WHEN** the new original desire encounters declared queued admission pressure during request or later registration polling
- **THEN** the existing owner retains that exact desire and remains eligible for bounded automatic admission after pressure clears
- **AND** no additional performer gesture is required and the old receipt never satisfies the new set and mask
- **AND** wrapped unknown failures remain terminal while PCM budget deferral keeps its existing bounded controller policy

#### Scenario: Shared mask publication does not restore a withdrawn direct demand
- **GIVEN** a previous direct ALL demand has ended and retains no explicit retry interest
- **WHEN** a shared mask publisher encounters declared admission pressure
- **THEN** it reports that failure without creating a new automatic direct retry interest
- **AND** a later explicit performer gesture may retain its own exact demand through the existing bounded controller

### Requirement: Pair-delayed duplicate generation retains a live completion route
The existing shared generation owner SHALL retain each exact source/model/content subscriber while Pair settlement temporarily prevents canonical admission. It SHALL resolve that retained interest through a compatible completed set, an actually live transferable canonical job or the existing fair waiting/inference queue, without recapturing changed intent or creating a second job registry.

#### Scenario: Canonical completion or removal while a duplicate waits for Pair
- **GIVEN** a duplicate generation job retains original unresolved subscribers and a canonical job with the same complete fingerprint
- **WHEN** Pair settlement delays their admission and the canonical completes, fails, cancels, disappears or retains a terminal delivery
- **THEN** the duplicate SHALL NOT transfer interest into a terminal delivery whose participant image was already captured
- **AND** while the actual prior worker or fenced settlement owns work, it SHALL retain the original candidate without starting overlapping inference
- **AND** once those actual owners settle, it SHALL reuse compatible completed data or admit the same retained candidate through the existing queue exactly once
- **AND** removing an overwritten target SHALL retire only its original subscriber after own Copy acceptance and SHALL preserve other subscribers, actual reader leases and terminal settlement owners

### Requirement: E11-06 Shared Stem Jobs Deduplicate Material And Model Intent
The system SHALL use one physical offline job for concurrent verified equal
material/model/configuration/shape/transform requests while retaining each
pad/direct/batch interest's independent identity and publication authority.

Verified full original content, full output shape/transform and the common
`effective-stem-v3` identity SHALL determine equality; filename, path, size and
mtime alone SHALL NOT deduplicate. Actual full sealed producer lineage SHALL be
captured during the first existing bounded non-realtime worker phase and handed
to the authoritative control owner. Before inference that owner SHALL resolve
coalescing or `Stems already present` against current verified selection and jobs,
using the captured effective identity and current Settings/material intent
fences. Workers SHALL NOT create a second authoritative job/selection registry.
Model/source hashing and producer preparation SHALL NOT run in UI rendering or
the realtime callback.

A captured fingerprint and actual producer inputs SHALL remain immutable through
inference. Different fingerprint replacements SHALL serialize per verified
material and obsolete completions SHALL NOT replace newer intent. Each joined
interest SHALL retain its own source ticket/version, content-instance identity,
request identity and monotone Settings/material revisions. Removing an origin pad
SHALL NOT revoke surviving subscriber interests or copy another pad's
source/timing/history ACK.

Verified cross-root copying or rebinding SHALL preserve effective producer
identity while acquiring each subscriber's own current native ticket/source
version. Artifact marker authority SHALL bind the sealed artifact generation
and SHALL NOT substitute for subscriber source or publication authority. Equal
WAV content SHALL NOT coalesce different known producer fingerprints.

#### Scenario: Same material occupies multiple banks
- **GIVEN** equal verified source bytes assigned independently to slot labels 1 and 216
- **WHEN** both request the same effective model fingerprint
- **THEN** one physical job serves two independently owned interests
- **AND** each later preparation and adoption requires its own current permits and ACK

#### Scenario: Equal name or shape has different bytes
- **WHEN** two loaded sources have equal filenames/size/shape but different full content digests
- **THEN** their jobs remain distinct and neither completion authorizes the other source

#### Scenario: Worker capture discovers an equal producer job
- **GIVEN** equal verified material requests reach the bounded worker producer-capture phase
- **WHEN** their sealed actual producer identities are handed to the control owner before inference
- **THEN** the authoritative decision joins their independent interests to one physical inference job
- **AND** a current complete matching selection instead returns Stems already present without inference or artifact writes

#### Scenario: Verified cross-root subscriber joins
- **GIVEN** equal original content is verified under different contained locators with the same sealed producer fingerprint
- **WHEN** the copied or rebound assignment joins shared generation or reuses its complete artifacts
- **THEN** effective identity remains locator-independent and its own current native ticket/source version governs publication
- **AND** another subscriber's ticket or the artifacts' sealed marker never grants authority for the copied assignment

#### Scenario: Actual producer differs from a requested label
- **GIVEN** requests use the same model label but resolve different complete checkpoint, configuration or producer implementation inputs
- **WHEN** the worker captures actual sealed producer lineage
- **THEN** the effective fingerprints differ and replacement serializes per verified material
- **AND** neither equal labels nor equal WAV hashes allow selected-model compatibility or duplicate coalescing

### Requirement: E11-06 All Loaded Pads Generate Through A Bounded Fair Batch
The system SHALL generate stems for warning-confirmed loaded pads across all
banks and all 216 slots through a bounded immutable inventory and the existing
two-worker/32-queued-job pool with material/model deduplication and fair admission.

The confirmed snapshot SHALL bind slot/content/material/model identities. Newly
loaded or changed targets SHALL be skipped visibly rather than mutated by stale
confirmation. Admission SHALL feed distinct jobs incrementally without enlarging
worker/queue/resource bounds or starving existing direct requests. Aggregate and
per-item queued/running/completed/skipped/failed/cancelled states, progress,
partial results and resource errors SHALL remain visible and bounded. Previously
completed items SHALL survive a later item failure or batch Cancel.

#### Scenario: Complete loaded inventory spans all banks
- **WHEN** the confirmed batch contains 216 loaded slots including labels 1 and 216
- **THEN** every still-current captured target receives a result while verified duplicate material/model work shares one physical job
- **AND** actual simultaneous workers never exceed two and queued jobs never exceed 32

#### Scenario: Direct work competes with a large batch
- **WHEN** a batch has more distinct jobs than available capacity and direct work is admitted concurrently
- **THEN** bounded incremental fair feeding gives both pending batch and direct interests progress without starvation
- **AND** unsupported real RAM/disk/retirement admission failures are reported without losing successful items or stopping playback

#### Scenario: One fair admission seam covers every offline phase
- **GIVEN** direct and batch resolve, inference and Pair-preparation work share the existing two-worker/32-queued pool
- **WHEN** current candidates remain ready while new interactive candidates continue arriving
- **THEN** one control-owned arbiter rotates over the six direct/batch-by-phase lanes with FIFO readiness order within each lane
- **AND** it grants at most six submissions per control poll and each persistently eligible lane receives a grant within six successful grants, conditional on actual workers and external guards eventually settling
- **AND** no production submission bypasses this seam to repeatedly consume all returned capacity
- **AND** no second executor, retained task-closure queue or audio scheduler is created

#### Scenario: Capacity refusal leaves exact pending ownership untouched
- **WHEN** the same pool cannot reserve capacity for an original pending candidate
- **THEN** that candidate remains pending without new source leases, retirement reservations, private jobs, Pair readers or manufactured errors
- **AND** a granted reservation is revalidated and either submitted or returned within the same control attempt, never held across polls, writer or Native ACK waits
- **AND** existing continuation inference and preparation remain eligible when the finite physical-generation WIP bound is reached

#### Scenario: A complete shared result has216 preparation subscribers
- **GIVEN** one complete generation or compatible reuse result retains216 original target subscribers
- **WHEN** preparation capacity is admitted incrementally
- **THEN** the existing terminal delivery retains each exact unsubmitted subscriber and its promotion/private owners until that subscriber is admitted or terminally settled
- **AND** identity binding, promotion and subscriber delivery occur once without repeated admission appends, premature state clearing or closing deferred retirement reservations
- **AND** submitted and returned writer-held preparations also obey a finite documented bound independently of executor occupancy
- **AND** prepared results retained after a refused Native discard remain inside that same bound until their actual discard succeeds

#### Scenario: An original target becomes stale after capacity was reserved
- **WHEN** source, content or intent changes between the offered candidate and its post-reservation revalidation
- **THEN** the original holder receives an explicit terminal stale outcome and unused capacity returns without new readers
- **AND** replacement content receives no error or publication from that obsolete request

#### Scenario: Equal work arrives before the original result is durably selected
- **GIVEN** complete successful artifacts with the same full source and sealed producer fingerprint remain protected by an existing promotion, Pair preparation or durable-settlement owner
- **WHEN** another current original subscriber resolves that fingerprint after physical inference has ended and before the original Config selection becomes visible
- **THEN** it reuses those retained artifacts through the existing admission and preparation path instead of starting the same physical inference again
- **AND** it acquires its own artifact custody, verifies actual current producer/artifact/source inputs and obtains its own Native publication receipts
- **AND** released custody or a metadata-only reference grants no reuse authority, and generic consumed-terminal joins remain rejected

### Requirement: E11-06 Stem Batch Cancel Detaches Only Its Own Interests
The system SHALL cancel only the requesting batch's pending inventory and owned
job interests, preserving independent direct/pad/other-batch subscribers and
their actual process/source/artifact leases.

Only the final interested job SHALL request cooperative abort of its own owned
process, with bounded teardown and truthful pending cancellation until actual
read end/exit. Queued cancelled work SHALL perform no source reads. A cancellation
flag around blocking `subprocess.run` SHALL NOT count as proof of prompt inference
abort. Cancel SHALL NOT terminate foreign processes, overwrite/delete leased
artifacts or report committed successes as undone.

#### Scenario: Batch and direct interest share a job
- **GIVEN** a batch and a direct pad action independently subscribe to the same physical job
- **WHEN** the batch is cancelled
- **THEN** its interest detaches and the direct subscriber still completes normally
- **AND** the worker/process retains its leases until its actual read ends

#### Scenario: Same PAD direct and batch interests share one publication owner
- **GIVEN** one current PAD has an exact source/content/request/intent publication subscriber and direct plus independently captured batch interests
- **WHEN** one batch interest joins or is cancelled before terminal delivery
- **THEN** only that interest token changes, preserving the exact subscriber and its source, request, intent and retirement owners
- **AND** other interests keep the same single physical job and later publication route
- **AND** token-only membership changes do not invalidate prepared Pair/Bank participant images

#### Scenario: Original interest is relocated or its content is removed
- **GIVEN** a batch owns an interest in an original publication subscriber
- **WHEN** intent resolution, canonical job transfer or an accepted Pair move relocates that subscriber
- **THEN** its stable interest ownership follows the original content and cancellation detaches only that original token
- **AND** accepted Bank removal or source invalidation may detach all interests belonging to removed original content
- **AND** an unrelated replacement occupant never inherits the old token or cancellation authority

#### Scenario: Late token has no terminal outcome route
- **GIVEN** a physical job has settled and its terminal participant image may already have delivered an original subscriber
- **WHEN** a new independent interest requests a generic job join
- **THEN** the generic join is rejected unless the authoritative controller has a still-current explicit outcome route
- **AND** an accepted token never waits indefinitely for an event already consumed

#### Scenario: Final interest ends during publication handoff
- **GIVEN** an original generation interest holder follows a completed worker into its existing Pair preparation and durable selection route
- **WHEN** the final token is removed while the returned Pair request is temporarily outside pending containers, before the existing config replacement wins
- **THEN** preparation eligibility and the final before-replace check reject its empty original holder without depending only on an earlier cancellation Event
- **AND** queued preparation whose final interest has ended performs no Native preparation read
- **AND** cancellation retains only its original physical reader and returned-result routes until they actually settle, never foreign readers

#### Scenario: Final batch Cancel crosses an admitted Pair or Bank
- **GIVEN** an admitted arrangement retains exact original generation, preparation or settlement participants through its Native ACK and control mirror
- **WHEN** a global batch Cancel or application close withdraws the last batch interest
- **THEN** its token is detached immediately while fenced physical subscriber, generation-table and preparation-cancellation state remains unchanged until the arrangement settles
- **AND** an empty-interest queued worker performs no artifact read and preserves the fenced original request's cancellation event and custody for that mirror
- **AND** cancellation remains pending while its exact deferred original custody or final physical reader remains, then removes or cancels the settled original placement through the existing owners
- **AND** foreign surviving work and replacement content are neither cancelled nor counted as this batch's pending readers

#### Scenario: Config replacement already won before Cancel
- **WHEN** actual config replacement succeeds before final-interest Cancel completes
- **THEN** the original holder latches its committed result before later owner settlement
- **AND** Cancel only detaches its token while the exact committed selection and existing preferred-mode/adoption/settlement route continue
- **AND** it does not cancel predecessor or foreign residency/control owners or report committed output as undone

#### Scenario: Promotion fails for an immediate or Pair-delayed subscriber
- **GIVEN** original subscribers share a completed physical worker and its retained terminal delivery
- **WHEN** artifact promotion or integrity verification fails before publication
- **THEN** every delivered original holder latches the original precommit failure before job disposal
- **AND** an original subscriber delivered after an accepted Pair move receives the same retained failure independently of transient slot-local messages
- **AND** an already-present no-op or a fixture without a real persistence callback never claims a new successful config replacement

#### Scenario: Final batch interest cancels running inference
- **WHEN** Cancel removes the last interested subscriber from a running job
- **THEN** only that job's owned inference process receives cooperative abort/teardown
- **AND** cancellation completes only after actual process/read settlement, then contained private artifacts retire off-thread

## MODIFIED Requirements

### Requirement: Stem Tasks Respect Per-Pad Concurrency
The system SHALL prevent conflicting per-pad source/task intent while admitting
independent immutable-source offline stem work for playing or paused pads after
the E11-05 safety proof.

Loading, unloading, conflicting analysis and superseded generation intent SHALL
remain rejected or deferred. Duplicate equal material/model work SHALL join the
existing shared job with its own interest or return an already-present no-op,
without a second physical inference. Current playback alone SHALL NOT constitute
an offline-job conflict or authorize active new-set adoption.

#### Scenario: Loading remains a conflict
- **GIVEN** a pad currently loading source audio
- **WHEN** the performer requests stem generation
- **THEN** the conflicting request is rejected or deferred with the existing load intact

#### Scenario: Playing alone is not an offline conflict
- **GIVEN** a playing pad with a stable immutable source and no conflicting intent
- **WHEN** a safe background generation request is admitted
- **THEN** playback and offline inference coexist with independent ownership and no callback generation

### Requirement: Stem Task Completion Is Revalidated Before Publication
The system SHALL revalidate material/model/subscriber identity and monotone
validated Settings/material intent revisions at each worker event and
immediately before current disk-selection mutation, and each pad's fresh native
source/request/timing/window/voice/history authority before effective publication
of completed stem work.

An unloaded, replaced, cancelled or otherwise stale subscriber SHALL NOT mutate
newer pad intent. Playing completion SHALL NOT bypass new-set adoption proof;
its complete verified disk selection MAY remain ready while effective audio
continues on the old set pending its own native ACK. Different subscribers SHALL
retain independent completion and acceptance status.

#### Scenario: Completed task is stale
- **GIVEN** work admitted for pad source A
- **WHEN** source B replaces A before completion
- **THEN** A completion cannot publish into B or consume B's job
- **AND** surviving current A subscribers may still finish their own work

#### Scenario: Verified disk result is not yet effective
- **WHEN** a current playing subscriber has a complete disk result but no accepted own new-set transaction
- **THEN** it reports disk-ready/pending/error separately from effective stems
- **AND** old effective FullMix/stem audio remains usable

### Requirement: Performer Stem Generation Uses Background Tasks
The system SHALL route performer per-pad and all-loaded generation requests
through the authoritative non-realtime source-leased background path with
bounded progress, shared-job deduplication and independent publication status.

Controller/direct/batch admission SHALL enforce the same source/model duplicate
and conflict rules. Activity-only offline gates SHALL be superseded only after
the safety proof, while native new-set adoption retains its own guarded ACK.
UI rendering SHALL neither run inference nor block on completion or long-file
verification.

#### Scenario: Playing UI request starts safe background work
- **GIVEN** a loaded playing pad with admitted immutable source ownership and no conflicting task
- **WHEN** the performer requests Generate after the safety gates pass
- **THEN** the bounded background job reports progress while UI and current playback remain responsive

#### Scenario: Duplicate direct request bypasses the button
- **GIVEN** a valid current selected-model fingerprint or equal in-flight physical job
- **WHEN** a caller requests generation directly despite the disabled UI
- **THEN** authoritative controller admission returns already present or joins one independent interest without starting duplicate inference
