## ADDED Requirements

### Requirement: NRPN Data Entry retains selected parameter identity
The system SHALL extend the earlier Increment/Decrement-only NRPN scope with
seven-bit Data Entry values. Under the existing fresh per-channel NRPN parameter
selection, CC6 SHALL emit one event for that complete fourteen-bit parameter
number with the unchanged seven-bit MSB value and original timestamp. Selection
SHALL remain available for later Data Entry and existing Increment/Decrement.
An optional CC38 under that same fresh selection SHALL be consumed without a
second Learn input, button edge, encoder movement or pitch attack. This bounded
path SHALL NOT assemble or claim fourteen-bit Data Entry value resolution.

Absent, incomplete, expired or RPN-cleared NRPN selection SHALL retain plain CC6
and CC38 behavior. Existing per-channel selection, freshness timeout, unrelated-CC
clearing and value-keyed standalone Inc/Dec rules SHALL remain intact. Existing
Inc/Dec normalized values 65/63 and saved action encodings SHALL NOT be changed;
the existing offset-binary encoding remains the matching Inc/Dec step format.
Data Entry values SHALL use the explicitly learned button or encoder encoding
through the original Native capture, controller, worker and sole scheduler.

#### Scenario: Independent NRPN parameters do not collapse into CC6
- **WHEN** fresh NRPN parameters 130 and 131 send Data Entry high/release values on their selected channels, with optional low bytes
- **THEN** Learn and saved mappings SHALL distinguish their full parameter identities
- **AND** fixed piano and Original Key buttons SHALL capture only their own fresh high edges, without an extra low-byte or release attack
- **AND** saved/reloaded encoder bindings SHALL retain their explicit variants and original selected content through productive effect and PCM

#### Scenario: Data Entry selection guards preserve existing CC controls
- **WHEN** Data Entry arrives without a complete fresh NRPN selection, on another channel, after expiry, after RPN selection or after an unrelated CC
- **THEN** it SHALL retain its ordinary CC binding instead of borrowing a stale or foreign parameter
- **AND** established NRPN and value-keyed CC Increment/Decrement mappings SHALL retain their previous binding identities and values

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

#### Scenario: Successful custody cycles coexist with productive independent voices
- **GIVEN** one, two, four or six productive voices own declared shared or distinct immutable sources and independent Native and transport state
- **WHEN** genuine GUI and routed MIDI retriggers fill and reuse the existing 64-work limit over several controlled cycles through bounded production-sized queues and the existing scheduler
- **THEN** every successful original action SHALL retain its own sequence, timestamp, content, T and frozen E, with its own attack feedback and independently matched mixed PCM between distinct effects
- **AND** capacity refusal SHALL allocate no additional work or Native, and fresh input SHALL recover only after real off-callback work retirement
- **AND** retaining terminal receipts SHALL NOT retain heavy work claims or grant another source or voice their authority
- **AND** controlled worker waits, source sharing, output format, process-counter boundaries and any verification-oracle cost SHALL be declared explicitly
- **AND** process CPU, working-set, private-commit and peak observations SHALL NOT be relabelled as performer-deadline, complete P6/V0, 216-content, device or Human acceptance

#### Scenario: Original SET callback work reuses allocated storage
- **GIVEN** an original live SET action has allocated its work and history storage off realtime while its preparation worker remains independent
- **WHEN** the actual callback captures that action's history, adopts its prepared candidate, continues rendering or refuses its stale source
- **THEN** those callback phases SHALL perform zero Rust heap allocation, zeroed-allocation or reallocation attempts and SHALL retain the original content, timing, effect and retirement guards
- **AND** a calibrated current-thread allocator observation SHALL distinguish callback operations from concurrent worker allocations without treating Rust-only observation as proof about Native/C++ allocations, device deadlines or whole-process resources

#### Scenario: Native custody survives retirement pressure and callback-owner close
- **GIVEN** independently productive nonneutral voices and original prepared pitch work retain their own Native and source owners
- **WHEN** the existing retirement storage is full, terminal work awaits destruction, or the actual complete callback owner closes
- **THEN** original work custody SHALL remain held until its storage and displaced or unadopted Native/source owners are actually destroyed off the callback
- **AND** a successful effect SHALL retain its original T/E and own adopted Native, while cancelled, stale or unavailable effects SHALL produce no partial attack or peer reset
- **AND** callback rendering and callback-owner close SHALL NOT construct or finally destroy any Native instance, including when normal retirement storage is full
- **AND** eventual worker completion SHALL destroy each required Native lifetime exactly once and release the final callback-owned source readers without terminal receipts retaining heavy work
- **AND** lifecycle evidence SHALL distinguish individual Native lifetimes despite opaque-address reuse and SHALL observe completed actual deletion and execution context rather than inferring destruction from a receipt or aggregate live count
- **AND** bounded test observation SHALL NOT introduce callback allocation, locks, logging or unbounded work, or certify Native/C++ allocation absence, device deadlines, acoustic quality or complete P6/V0/Human acceptance

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

#### Scenario: Public live stem replacement fences an original prepared piano attack
- **GIVEN** a current selected V1 voice with nonneutral piano pitch and an original GUI or routed MIDI SET_RETRIGGER already prepared for its frozen future effect E
- **WHEN** public Generate and a public separator change adopt a distinct V2 set with its own effective acknowledgement before E while the content remains the same
- **THEN** the original V1 action SHALL terminate fenced without retuning or attacking the voice or restoring V1 selection, Native or readers over V2
- **AND** its original content identity, timestamp, numeric target, T and frozen E SHALL remain unchanged without recapture or requantization
- **AND** living V2 and independent other-pad PCM, source phase, Native, FIFO and filter chronology SHALL continue through that rejected application
- **AND** actual V1 readers SHALL remain protected until their original action and all other real readers reach terminal retirement
- **AND** a later fresh V2 SET_RETRIGGER SHALL use its own capture, current selection, preparation and scheduler acknowledgement, producing one own attack with independently matched selected-source Native output
- **AND** declared timing or inference fixtures SHALL NOT certify device deadlines, neural execution or complete musical/resource/Human acceptance

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

#### Scenario: Pitch releases coexist with a real waveform pause owner
- **GIVEN** an actual mouse waveform Hold owns the paused native cohort and a mapped pitch SET or SET_RETRIGGER retains its original captured action
- **WHEN** its routed MIDI NoteOff or NoteOn velocity0 is received before the matching physical mouse release
- **THEN** those pitch-key releases SHALL create no attack, pitch or highlight reset, or waveform release
- **AND** SET SHALL preserve the original paused cohort until its matching release, while an effective SET_RETRIGGER SHALL supersede that old cohort claim only at the original action's frozen E
- **AND** the original waveform controller SHALL settle the same physical owner against actual native acknowledgement rather than derive release authority from the selected slot or requested pitch

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
