## ADDED Requirements

### Requirement: E11-06 Shared Stem Jobs Deduplicate Material And Model Intent
The system SHALL use one physical offline job for concurrent verified equal
material/model/configuration/shape/transform requests while retaining each
pad/direct/batch interest's independent identity and publication authority.

Content SHA and verified canonical material SHALL determine source equality;
filename, size and mtime alone SHALL NOT deduplicate. A captured fingerprint
SHALL remain immutable through inference. Different fingerprint replacements
SHALL serialize per material and obsolete completions SHALL NOT replace newer
intent. Removing an origin pad SHALL NOT revoke surviving subscriber interests
or copy another pad's source/timing/history ACK.

#### Scenario: Same material occupies multiple banks
- **GIVEN** equal verified source bytes assigned independently to slot labels 1 and 216
- **WHEN** both request the same effective model fingerprint
- **THEN** one physical job serves two independently owned interests
- **AND** each later preparation and adoption requires its own current permits and ACK

#### Scenario: Equal name or shape has different bytes
- **WHEN** two loaded sources have equal filenames/size/shape but different full content digests
- **THEN** their jobs remain distinct and neither completion authorizes the other source

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
The system SHALL revalidate material/model/subscriber identity before disk
commit and each pad's fresh native source/request/timing/window/voice/history
authority before effective publication of completed stem work.

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
