## ADDED Requirements

### Requirement: Pitch work owns bounded custody before allocation
Each audio engine SHALL reserve one of 64 non-cloneable pitch-work claims before
allocating its work envelope or callback history snapshot. The existing control
producer SHALL have space before that reservation. Exhaustion SHALL reject only
the original action with capacity and SHALL NOT allocate Native or work storage.
The claim SHALL follow the same original work through control publication,
scheduler admission, worker preparation, ready storage, application, cancellation
and retirement. Rejection, cancellation or an effective receipt alone SHALL NOT
release capacity: release SHALL occur only after that work's storage and displaced
Native/source owners are destroyed off the callback. Publication clones SHALL NOT
duplicate a claim or an attack. This bound SHALL NOT be presented as a whole-process
RAM, ordinary-start, 216-content or performer-deadline guarantee.

Native construction and adapter/FIFO allocation SHALL occur only on the existing
preparation worker after the original request's current source and permit checks.
Stopped SET, actual scalar/no-op work and pre-preparation refusals SHALL construct
no Native. Callback history storage SHALL already exist when callback capture
begins. The existing scheduler, DSP path, original content/lifetime/timestamp and
T/E policy SHALL remain authoritative, and equal-target attacks SHALL stay distinct.

#### Scenario: Publication pressure precedes callback admission
- **GIVEN** 64 original pitch-work owners remain queued, preparing, ready or awaiting retirement
- **WHEN** another GUI or MIDI pitch action is published
- **THEN** it SHALL receive capacity refusal without allocating work or Native
- **AND** cancellation alone SHALL NOT free another owner's storage claim
- **AND** actual off-callback retirement SHALL permit a later original action to proceed

#### Scenario: Scalar and rejected actions avoid Native construction
- **WHEN** stopped SET, actual scalar work or an action refused before preparation is processed
- **THEN** its numeric intent and original receipt SHALL use the existing guarded path without constructing Native
- **AND** wet work SHALL construct its own candidate only after worker validation

#### Scenario: Distinct original attacks survive custody reuse
- **WHEN** repeated equal or different GUI/MIDI retriggers saturate and reuse custody over several cycles
- **THEN** every admitted action SHALL retain its own sequence, timestamp, content, original T and frozen E
- **AND** actual PCM and attack feedback SHALL prove each successful effect without coalescing
- **AND** all terminal work SHALL release its claims exactly once after off-callback destruction

#### Scenario: Original SET callback work reuses allocated storage
- **GIVEN** an original live SET action has allocated its work and history storage off realtime while its preparation worker remains independent
- **WHEN** the actual callback captures that action's history, adopts its prepared candidate, continues rendering or refuses its stale source
- **THEN** those callback phases SHALL perform zero Rust heap allocation, zeroed-allocation or reallocation attempts and SHALL retain the original content, timing, effect and retirement guards
- **AND** a calibrated current-thread allocator observation SHALL distinguish callback operations from concurrent worker allocations without treating Rust-only observation as proof about Native/C++ allocations, device deadlines or whole-process resources

### Requirement: Pitch Learn stores stable immutable action variants
The system SHALL provide separate stable SET and SET_RETRIGGER semantic actions for every supported absolute Key and extra integer including0 and SHALL persist the variant selected during Learn.

Actions SHALL encode pitch class/mode or signed semitones rather than menu position.
Checkbox changes SHALL affect GUI and newly learned variant only. Both variants
for the same target SHALL be simultaneously learnable and restorable.

#### Scenario: Two bindings survive checkbox and pad changes
- **WHEN** SET0 and SET_RETRIGGER0 are learned on different keys and checkbox/selection later changes
- **THEN** each saved binding keeps its semantic target/variant through save/reopen

### Requirement: Pitch input captures selected content at authoritative admission
The system SHALL capture and guard each pitch action at authoritative admission.

#### Scenario: Immutable action envelope and lifetime fencing
- **WHEN** GUI or MIDI pitch input reaches authoritative admission
- **THEN** the following complete capture and lifetime contracts SHALL apply:

The system SHALL capture explicit GUI content or authoritative native selected ContentInstance/lifetime for MIDI with unique accepted action sequence, original timestamp, full numeric/render tuple, source/timing/native/prepared permit and existing trigger intent.

Direct native, controller fallback and pending retries SHALL use the same captured
envelope and selection-to-pitch ordering across existing ports/channels/MultiLoop.
Merely queued polling/preparation SHALL NOT be described as accepted action.
Selection after admission SHALL NOT redirect the event. Move/Swap SHALL carry live
identity; removal/reassignment/overwrite SHALL fence its old lifetime.

#### Scenario: Selection from controller then pitch from keyboard
- **WHEN** one routed device selects A and another admits a pitch action, followed by selecting B
- **THEN** the admitted action belongs to A with original timestamp in direct/fallback/pending paths
- **AND** existing device/channel collision rules are tested rather than invented per-device bindings

#### Scenario: Routed selection order survives delayed pitch publication
- **GIVEN** existing MIDI bindings select A, choose a nonneutral pitch and then select B
- **WHEN** those messages pass through the existing dispatcher before the captured pitch owner is published and while its accepted preparation remains pending
- **THEN** the pitch action SHALL retain A's original content lifetime, timestamp, tuple and T/E through its actual scheduled effect in either MultiLoop mode
- **AND** later selection and NoteOff or velocity-zero messages SHALL neither retarget that owner nor add an attack or reset
- **AND** capacity refusal before admission SHALL visibly reject that original owner without replaying it against B; a later fresh input SHALL capture the then-current content independently

#### Scenario: GUI and MIDI independently retain Quantize and SYNC
- **WHEN** either GUI or MIDI admits a pitch retrigger under each of the four Quantize/SYNC combinations and either Key Lock mode
- **THEN** that route SHALL retain its own original T, separate preparation E, captured content, timestamp and numeric tuple despite later control projections
- **AND** its actual source phase and rendered pitch SHALL follow the captured rate and Key Lock contract without a second scheduler

#### Scenario: Native effect precedes the project source association
- **GIVEN** an original opaque pitch capture belongs to Native content A while its project source association remains pending and a later selection points to B
- **WHEN** the original action is published through the real Controller handoff and reaches its Native effect before project association can complete
- **THEN** polling SHALL retain the same original receipt until its living source can be associated and its chosen numeric intent and effective status can be projected onto A exactly once
- **AND** association completion SHALL NOT recapture, republish, retarget B or reconstruct the original timestamp, sequence, content or T/E
- **AND** a temporarily pending Move/Swap control mirror SHALL NOT count as a dead source lifetime; a known project content identity SHALL NOT release the receipt before its numeric projection is delivered or superseded
- **AND** a rejected action or a source lifetime that can no longer be associated SHALL still terminate safely without changing replacement content

#### Scenario: Move or overwrite after admission
- **WHEN** A moves/swaps or its lifetime is removed before the event applies
- **THEN** it follows live A or terminates fenced, never targets the former slot's replacement

#### Scenario: Occupied Copy fences an original target pitch attack
- **GIVEN** living source A has a nonneutral piano and offset tuple and occupied target B owns an original captured and published pitch retrigger
- **WHEN** a real occupied Copy replaces B with fresh stopped C before that retrigger's effect, including an already admitted/prepared candidate or either queue order
- **THEN** the original B action SHALL terminate fenced without attacking or retuning C at its original frame
- **AND** an admitted action SHALL retain its frozen execution frame while Copy-first processing SHALL reject stale B before scheduler admission
- **AND** C SHALL retain A's copied numeric tuple and its own fresh lifetime until a fresh C action is admitted
- **AND** A SHALL retain its original Native owner, source, FIFO, DSP history and independent PCM chronology through Copy claim and settlement
- **AND** a fresh C action SHALL use C's own content, source preparation and Native owner through the existing scheduler
- **AND** host preparation waits SHALL NOT be represented as device deadline or Human acceptance

#### Scenario: Selected components retain their own Copy and pitch adoption
- **GIVEN** nonneutral A renders a nonzero selected-component mask while occupied B owns a different selected mask and independent living Native state
- **WHEN** occupied Copy replaces B with stopped C and the original B pitch owner is fenced
- **THEN** A SHALL retain its selected readers and independent productive Native, PCM, FIFO and source chronology
- **AND** C SHALL obtain its own current selection registration and demanded component acknowledgement before its fresh pitch attack
- **AND** stopped component readiness SHALL NOT be reported as an audible effect
- **AND** C's fresh original action SHALL adopt its own nonneutral selected-source Native and actual wet output
- **AND** if a later launch or cancellation invalidates a mode receipt, that stale receipt SHALL NOT substitute for C's own current effective-mode acknowledgement

#### Scenario: Stop cancels a prepared selected replacement attack
- **GIVEN** occupied Copy has produced stopped selected C with its own accepted component receipt while nonneutral selected A continues independently
- **WHEN** a future original C pitch retrigger is prepared and public pad Stop or targeted launch cancellation revokes it before its frozen effect frame
- **THEN** the original action SHALL terminate as launch-cancelled without attacking or changing C's copied pitch, content lifetime or source assignment
- **AND** its captured identity, timestamp and original due and execution frames SHALL remain unchanged
- **AND** C's old component receipt SHALL lose current effective authority while A retains its own launch revision, readers, Native, PCM, FIFO and source chronology
- **AND** a later fresh C input SHALL obtain current own component readiness and adopt its own selected-source Native through the existing scheduler, without reviving the cancelled action or stale receipt

#### Scenario: Original pitch worker prepares before the Pair control mirror
- **GIVEN** A owns an admitted nonneutral pitch retrigger with its original content lifetime, timestamp, numeric tuple, T and frozen E
- **WHEN** a real Move or Swap publishes before that same action reaches its preparation worker and the Pair control mirror remains pending
- **THEN** the worker SHALL prepare the original immutable source through its stable content-cell guards without requiring the control mirror
- **AND** the same action SHALL retain exactly one work owner and one Native construction and SHALL apply at its unchanged E after coherent Native placement becomes available
- **AND** no former-slot occupant or independent living voice SHALL receive that action, restart or lose its existing Native history

#### Scenario: Prepared pitch crosses Pair and fresh former-slot loading
- **GIVEN** A already owns its prepared pitch candidate and B has independent nonneutral living Native state
- **WHEN** A moves to an empty slot followed by a genuine cold-source load into its former slot, or swaps with B
- **THEN** the original action SHALL adopt only A's exact candidate and pitch at its original E, preserving its captured source phase contract
- **AND** B SHALL retain its original source, generation, Native, pitch, phase and independent PCM progression
- **AND** the fresh former-slot content SHALL retain its own new lifetime, source and neutral stopped state without an attack from A's original action

### Requirement: Pitch note releases have no attack or reset semantics
The system SHALL treat MIDI NoteOff and NoteOn velocity0 as nonattack/nonreset for new pitch actions and SHALL preserve chosen pitch/highlights without resuming a mouse waveform hold.

#### Scenario: Release after repeated attacks
- **WHEN** repeated equal-target NoteOn attacks are followed by NoteOff or velocity0
- **THEN** attacks remain distinct and release neither adds attack nor resets pitch0/highlight/hold

### Requirement: Discarded unpublished pitch captures cannot become requested intent
The system SHALL cancel the exact original Native owner when disabled mapping,
Learn or an explicit Learn queue drain discards an unpublished pitch capture.
A later capture SHALL NOT inherit numeric intent solely from a discarded
capture, including when cancellations arrive in FIFO order or separate polls.
Cancellation SHALL preserve newer retained captures and authoritative projected
requested intent. Removal, replacement and timing refresh SHALL NOT restore an
old source's unresolved shadow into a new source; Move SHALL follow content.

#### Scenario: Several discarded captures have no surviving predecessor
- **GIVEN** two or three unpublished extra-pitch captures precede their first cancellation
- **WHEN** disabled mapping or Learn discards every capture in FIFO order, together or across separate polls
- **THEN** each exact original owner SHALL be cancelled without publication
- **AND** later Base SET composition SHALL use the last retained requested anchor and SHALL NOT resurrect any cancelled predecessor
- **AND** the same result SHALL hold for other cancellation orders

#### Scenario: An older discard preserves a newer chosen owner
- **WHEN** an older unpublished pitch capture is discarded while a newer capture remains retained or its intent is authoritatively projected
- **THEN** cancellation SHALL affect only the discarded owner and its predecessor links
- **AND** the newer requested tuple, sequence and eventual effect SHALL remain unchanged

### Requirement: Requested pitch rollback uses bounded control custody
The system SHALL bound unresolved requested-shadow bookkeeping to 64 scalar
records under the existing control owner. These records MAY retain lightweight
atomic status observations but SHALL NOT retain Native adapters, action-work
owners, PCM pins or an unbounded chain of original owners. Journal reconciliation
SHALL remain outside the audio callback and SHALL NOT advance unrelated source,
timing or RuntimeProjection authority.

#### Scenario: Requested-shadow capacity is exhausted and recovered
- **GIVEN** all 64 unresolved control records remain owned
- **WHEN** another pitch capture is attempted
- **THEN** it SHALL reject with capacity before allocating its original owner or changing requested intent
- **AND** cancellation or authoritative settlement SHALL release bounded bookkeeping so a later original capture can succeed

#### Scenario: A stale source cannot leave phantom intent after reconciliation
- **WHEN** an unresolved capture is fenced by a logical source or timing refresh and later cancelled
- **THEN** its record SHALL settle without overwriting the current source's authoritative requested anchor
- **AND** the next capture SHALL compose from that current anchor rather than an unresolved old-source tuple
