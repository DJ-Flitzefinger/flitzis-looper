## ADDED Requirements

### Requirement: E11-06 All Loaded Stem Actions Require Scoped Confirmation
The system SHALL expose Generate Stems of all loaded PADs and Delete Stems of
all loaded PADs in global Settings with warning confirmation binding their
all-bank captured content/material/set/model scope and affected pad counts.

Cancel before acceptance SHALL have no effect. A stale dialog SHALL NOT act on
new content, model or set versions. Generate SHALL show bounded progress, partial
success/error and a Cancel control whose status follows actual interest/process
settlement. Disk-ready, pending native adoption and effective stems SHALL be
distinguished; neither complete files nor another pad's ACK SHALL imply success
for an unacknowledged pad.

#### Scenario: Warning is cancelled or target changes
- **WHEN** the performer cancels the warning, or captured content/model/set identity changes before acceptance
- **THEN** Cancel mutates nothing and changed targets are skipped with explicit status
- **AND** newly loaded content is never a substitute target

#### Scenario: Metadata-only warning captures requested processing intent
- **WHEN** the performer opens the all-loaded generation warning
- **THEN** the immutable inventory contains all216 slot positions with exact captured loaded source, content, material, shape, path and current set identities, requested processing settings and monotone Settings revision
- **AND** it performs no source hashing, model preparation, source lease or retirement reservation, private-directory creation or job admission
- **AND** actual effective producer identity is resolved only in the existing bounded worker phase
- **AND** source/content/material/set/Settings changes including a generation or Settings ABA make the original target stale rather than retargeting it

#### Scenario: Batch completes only partly
- **WHEN** some targets succeed and others fail, remain pending or are cancelled
- **THEN** Settings reports their exact distinct states and retains successful committed results
- **AND** UI rendering performs no inference, long-file hashing or blocking wait

#### Scenario: One confirmed batch advances outside Settings
- **WHEN** the warning is accepted once and Settings is then closed
- **THEN** one transient batch owner continues offering bounded original items through the existing StemController and retains exact per-item and aggregate results
- **AND** opening, accepting, advancing or cancelling the batch preserves selected PAD and Bank
- **AND** every originally occupied target has exactly one queued, running, done, skipped, failed, cancelling or cancelled state; already-present is a visible no-op and Done means its stems were saved
- **AND** a terminal partial result remains available until deliberately dismissed or replaced by another confirmed batch

#### Scenario: Batch Cancel keeps original reader settlement visible
- **WHEN** Cancel withdraws the batch while original queued or running interests remain
- **THEN** untouched targets become Cancelled and only this batch's tokens are detached from shared operations
- **AND** Cancelling persists while its own original physical-read or returned-result receipt remains unsettled
- **AND** committed, already-present and failed outcomes remain truthful and surviving direct or foreign callers continue
- **AND** saved stems and own pending audio adoption remain distinct from effective audio

### Requirement: E11-07 All Loaded Stem Deletion Uses Confirmed Shared-Material Scope
The system SHALL delete stems for all still-matching warning-confirmed loaded
pad assignments across all 216 slots, deduplicating exact shared material/set
versions and selecting safe FullMix through the existing guarded native path.

Deletion SHALL revoke only targeted stem eligibility and pending interests, not
external originals, FullMix source PCM or newer/unknown/still-owned artifacts.
Active selections/transitions and old voices SHALL retain actual reader owners
through their bounded FullMix handover; physical cleanup SHALL wait for actual
last readers. Other non-target users SHALL remain unchanged, repeated deletion
SHALL be a truthful no-op and deferred cleanup failures SHALL remain visible.

#### Scenario: Shared stems are deleted during playback
- **GIVEN** multiple loaded pads in different banks share a current set and active or paused voices retain it
- **WHEN** the matching all-loaded deletion is confirmed
- **THEN** targeted current intent safely returns to FullMix without transport restart or source loss
- **AND** shared files remain until every actual voice/job/history/reader retires

#### Scenario: Old delete dialog races replacement
- **WHEN** a target receives a new source or stem generation after the deletion snapshot
- **THEN** deletion skips that new identity and preserves its current files and owners
- **AND** cleanup targets only exact contained retired versions with no recursive material overwrite

#### Scenario: Deletion warning observes pending work without granting new scope
- **WHEN** the deletion warning captures pending generation or a retained complete set
- **THEN** its metadata-only inventory binds original PAD, path, content, loaded source, current durable set, exact known retained sets and existing monotone generation admission identities
- **AND** it performs no source or model hashing, source acquisition, file traversal, retirement reservation or job admission
- **AND** an unrelated processing Settings revision, retained-owner ordering, transient availability or worker phase alone does not invalidate deletion
- **AND** a newly observed generation admission, source, current durable set or retained set makes that original item stale
- **AND** disappearance of a captured pending admission grants no permission to delete any uncaptured output
- **AND** completion that changes the current durable set conservatively skips the old warning even when that exact set was previously retained
- **AND** an arrangement-fenced original item is deferred before observing its intermediate image

#### Scenario: Confirmed deletion advances independently of Settings
- **WHEN** the all-loaded deletion warning is accepted once
- **THEN** one transient owner retains the exact original inventory and offers at most eight original items per application frame independently of Settings visibility
- **AND** acceptance withdraws only exact captured still-unadmitted batch caller tokens before generation can admit them
- **AND** logical withdrawal feedback uses the actual withdrawal result, preserves partial results and distinguishes Removed, No-op, Skipped, Failed and pending originals
- **AND** Removed confirms logical stem eligibility or request withdrawal while global pending cleanup and cleanup errors remain separate actual observations
- **AND** closing the application settles accepted logical deletion through the same bounded owner before shutting down its backend

#### Scenario: A logical-only original has already been withdrawn
- **GIVEN** exact currentness and edit guards pass for an original FullMix PAD with no current or retained set and no admitted or pending control work
- **WHEN** the deletion owner advances that original after acceptance withdrew its exact unadmitted caller token
- **THEN** the per-PAD backend returns No-op without retirement reservation or a full-project assignment rescan
- **AND** the operation may report Removed only from its actual accept-time withdrawal receipt
- **AND** an original retaining a saved stem assignment owner, set, admission, Pair, settlement, publication or verification route still uses the ordinary guarded withdrawal and retirement authority
