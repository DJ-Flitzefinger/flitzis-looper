## ADDED Requirements

### Requirement: Stable placement and movable content have separate authority
The system SHALL distinguish fixed PadSlotId/slot-binding epoch, movable ContentInstanceId/nonreused lifetime, shared immutable material/analysis/StemSet versions and current source/timing/window/native action authority.

#### Scenario: Equal material does not grant copied current authority
- **WHEN** A is copied or its slot reused with equal source bytes
- **THEN** the new content gets fresh identity/lifetime/current guards and genuine native ACK
- **AND** material equality, copied ACK or historical path rewriting grants no live authority

### Requirement: Musical copies are independent stopped equal users
The system SHALL copy all musical source/analysis/selected-stems/loop/excerpt/grid/timing/manualTAP/correction/base/extra/KeyLock/playback/GainEQ/current mask/custom/preset/mutes/retrigger intent into fresh stopped content, sharing only verified immutable data.

#### Scenario: Copy and transposition preserve immutable sharing and independent state
- **WHEN** musical content is copied, transposed or independently played
- **THEN** the following complete normative contract applies:

The system SHALL copy all musical source/analysis/selected-stems/loop/excerpt/grid/timing/manualTAP/correction/base/extra/KeyLock/playback/GainEQ/current mask/custom/preset/mutes/retrigger intent into fresh stopped content, sharing only verified immutable data.

Voices/cursors/DSP state/meters/progress/pressed/hold/job/native tokens SHALL NOT
be copied. Suitable input ranges SHALL share RAM backing, varied ranges MAY need
separate views and all DSP/settings SHALL remain independent. Copy/transposition
SHALL NOT alone produce new original/PCM/stem files or full-source pitch PCM.

#### Scenario: Once prepared drums bass melody vocals workflow
- **WHEN** prepared A is copied to several pads and each selects its own stems/loops/pitch
- **THEN** copies play independently with no valid-data analysis/decode/separation or file duplication
- **AND** changing any copy leaves A and the others unchanged, including after origin-bank deletion/reopen

#### Scenario: Complete empty-target Copy has its own native transaction
- **GIVEN** a loaded source content and an empty target slot have matching current identities
- **WHEN** a complete musical CopySnapshot is prepared off-thread
- **THEN** its immutable source and suitable PCM/stem backing SHALL be shared through independent verified custody without import, complete decode, separation or file duplication
- **AND** the target SHALL receive a fresh durable content identity and nonreused runtime lifetime, distinct from the source's current permissions
- **AND** actual Native acceptance SHALL establish a stopped target with the complete captured source/timing/stem/mask/GainEQ/key intent before Copy reports completion
- **AND** the source's voices, cursor, DSP/history and hold effects SHALL remain unchanged
- **AND** preparation/admission rejection before claim SHALL preserve target occupancy/current epochs/config and release only prepared target owners
- **AND** irreversible claim without observed acknowledgement SHALL retain transaction owners and visibly fence target/config conflicts until actual reconciliation
- **AND** ordinary source-load acknowledgement alone SHALL NOT certify the complete CopySnapshot
- **AND** a known unsupported retained config schema SHALL reject Copy before Native capture or writer reservation

#### Scenario: Earlier target parameters cannot rewrite the copied image
- **GIVEN** Gain/EQ or other scalar target edits were queued before an empty-target Copy reservation
- **WHEN** the Native callback claims and installs the complete Copy image before draining those parameters
- **THEN** parameter admission and callback coalescing SHALL reject edits bound to the predecessor target epoch
- **AND** target edits admitted during a pending Copy SHALL fail before queue, epoch or durable intent mutation
- **AND** own Copy acknowledgement SHALL describe the captured complete target settings even when the config settlement runs concurrently

#### Scenario: Portable activation requires the complete Native Copy interface
- **WHEN** a newly built portable runtime is checked before activation
- **THEN** its Native class SHALL expose capture, preparation, publication, reconciliation, finish and cancellation of complete pad Copy
- **AND** missing operations SHALL reject activation without constructing an engine or opening an audio device

#### Scenario: Closing waits for real Copy settlement
- **WHEN** application close is requested while a complete Copy owns preparation or publication
- **THEN** new Copy admission SHALL close and an unclaimed Native transaction MAY be cancelled immediately
- **AND** controller assignment images, config writer and retirement reservations SHALL remain owned until the actual preparation worker has returned
- **AND** a claimed transaction SHALL settle through its own complete ACK and normal durable commit before dependent controllers, assignment custody or the audio engine shut down
- **AND** an unobserved ACK or failed durable commit SHALL keep close pending with a visible error rather than imply cancellation or successful completion

### Requirement: Physical waveform holds retain an always reachable release owner
The waveform Controller SHALL own bounded opaque Native Hold tickets and the physical press latch. Native SHALL decide whether a pause caused an effect independently of Session flags. Release SHALL consume the same ticket without a slot, fresh timestamp, revision or permit. True release admission MUST retain the owner until Native reports a terminal outcome; is_current False MUST NOT be treated as cleanup proof.

#### Scenario: Release remains reachable through hidden UI and Pair settlement
- **WHEN** a hold is pressed and the editor is closed, retargeted, hidden, loses focus, unloads or receives global RightButtonUp during a pending Pair
- **THEN** the same sticky release obligation SHALL remain reachable through the existing global after_swap control tick before Close and Hidden early returns
- **AND** each requested unadmitted owner SHALL receive at most one Native release attempt per tick; admitted release SHALL be observed until terminal
- **AND** repeated Up, release before PauseACK and True before callback settlement SHALL NOT drop or duplicate the original owner
- **AND** both Move/Swap and release orderings SHALL preserve unrelated or prepaused cohorts and reject stale telemetry as Hold authority
- **AND** intentional Pause SHALL supersede the old Hold transport revision even when Session already projects paused

### Requirement: Pair mutation preserves living playback and selection
The system SHALL apply Move to empty or Swap to occupied target as one guarded native/content/project transaction preserving both surviving lifetimes, voices, cursor, DSP/history/FIFO/filter/ramps and selected content without restart.

#### Scenario: Occupied Copy requires the exact original target and one stopped successor
- **GIVEN** independent loaded contents A and B and their exact current Native bindings
- **WHEN** an occupied-target Copy prepares A's complete musical snapshot for B's slot
- **THEN** capture SHALL require B's original opaque engine-owned binding, lifetime, physical placement, source, timing, preparation, authority, runtime and selection guards; a boolean overwrite flag or equal material SHALL NOT substitute for B
- **AND** one existing Copy mutation claim SHALL retire only B's living cohort and caused Hold effects, advance the target binding epoch and install a fresh stopped C with independent mutable state and shared immutable A backing
- **AND** A's voices, history, DSP, cursor, holds and selection SHALL remain unchanged
- **AND** queued actions and feedback for B SHALL remain bound to B and SHALL NOT control C after replacement
- **AND** all voice, source, outgoing-stem and receipt owners removed by the claim SHALL leave through prepared bounded retirement capacity outside realtime
- **AND** transient Pending or insufficient retirement capacity SHALL retain the identical complete payload for bounded retry; stale B, failed capture or preclaim cancellation SHALL leave both contents unchanged
- **AND** until C's own complete Native acknowledgement and control mirror are reconciled, target observers and conflicting admissions SHALL remain fenced, old/new custody SHALL remain owned and no durable success or rollback SHALL be reported
- **AND** only after that own acknowledgement SHALL the original target's controller owners and job subscriber be retired; shared jobs with other subscribers SHALL remain alive and C SHALL receive its own copied registration rather than B's receipt
- **AND** the existing complete config and asset transaction SHALL preserve unrelated newer intent and retained before/after owners through failed postclaim settlement, with restart uncertainty remaining explicitly fenced until verified recovery and fresh Native acceptance

#### Scenario: Current content permissions survive placement without relabeling
- **GIVEN** a living content owns current Native source, timing, window, preparation, selection and playback permissions
- **WHEN** a complete guarded Move or Swap changes only its placement
- **THEN** those permissions SHALL remain bound to the same engine-owned nonreused content lifetime and the same actual guard owners
- **AND** the transaction SHALL NOT reconstruct current authority from equal PCM, a former slot number, copied epochs or historical acknowledgements
- **AND** existing admitted voice/history owners SHALL retain their actual state rather than be re-created through load, unload or a new voice birth

#### Scenario: Slot snapshot and admitted content action have distinct authority
- **GIVEN** slot s contains A and an input route has observed a slot snapshot but has not admitted a content action
- **WHEN** A moves to t and B later occupies s
- **THEN** that old slot snapshot SHALL NOT admit a new action for A or B
- **AND** a fresh fixed-slot MIDI or keyboard gesture SHALL resolve the current occupant of s through its existing authoritative admission path
- **AND** an action already accepted for A before the Move SHALL retain A's lifetime and its own captured effect/cohort guards rather than resolve B

#### Scenario: Equal PCM and acknowledgement scalars cannot substitute a current timing record
- **GIVEN** two independent contents share PCM and equal source generation, digest and numeric timing acknowledgement values
- **WHEN** Native placement is complete while the control mirror is still settling
- **THEN** a current timing lookup SHALL also match the record's captured Native owner and content key to the actual occupant
- **AND** the other content's real acknowledgement SHALL NOT promote a foreign record as current
- **AND** record retirement SHALL follow the record's own surviving content acknowledgement rather than the new occupant of its former slot
- **AND** current metadata SHALL project the resolved slot without rewriting the immutable accepted evidence or its historical job identity

#### Scenario: Pair preparation fences admission without withdrawing live permission
- **GIVEN** a playing or paused participant has an accepted stem receipt and current Native history
- **WHEN** pair preparation reserves its placement against conflicting new admissions
- **THEN** that reservation alone SHALL NOT revoke the participant's current source, timing, selection, history or owned pause effect
- **AND** incomplete control projection SHALL NOT be reported as a successful removal, failed Native preparation or replacement effective receipt
- **AND** a preclaim failure SHALL release only the reservation and prepared transaction owners while preserving the latest actual participant state

#### Scenario: Genuine intent change still rejects an obsolete prepared pair
- **GIVEN** a pair transaction captured both participants' exact current guards
- **WHEN** an already admitted source, timing, window or selection change advances one of those guards before pair claim
- **THEN** the obsolete pair SHALL reject before changing placement or claiming either participant
- **AND** it SHALL NOT roll back that newer intent, revive an obsolete acknowledgement or transfer an earlier action to another content

#### Scenario: Placement publication preserves bounded lookup and monotone slots
- **WHEN** the callback claims a prepared pair image
- **THEN** it SHALL publish both matching content placements and their complete pair acknowledgement at one ordered commit
- **AND** content-bound realtime guard lookup SHALL use fixed bounded Native owner state without locks, allocation, waiting or a search through content slots
- **AND** fixed-slot request and binding epochs SHALL remain monotone and SHALL NOT be replaced by a smaller counter from another slot
- **AND** removal or equal-material replacement SHALL fence the removed lifetime so that an old content or slot token cannot become current again through ABA reuse

#### Scenario: Complete internal pair claims require their original participants
- **GIVEN** a pair preparation retains both original occupied or empty slot snapshots, complete source/window/timing/authority and control projections, and its own two-slot reservation
- **WHEN** the callback attempts Move or Swap
- **THEN** it SHALL validate both original participants, actual acknowledgement and preparation owners, intent/runtime/key-lock/resident/launch/transport/hold/copy guards and complete mixer occupancy before one irreversible claim
- **AND** a foreign reservation, exhausted counter, changed participant, failed capacity check or rejected claim SHALL leave both Native images and placements unchanged
- **AND** reservation SHALL fence new participant admissions without revoking surviving current source, history, timing or pause-effect permission
- **AND** the successful claim SHALL publish the complete pair placement and its own acknowledgement, preserve the actual surviving voice/DSP/FIFO/history owners, and retire displaced owners outside realtime
- **AND** Move SHALL install independent prepared empty defaults at the former source rather than inherit the empty target's previous settings
- **AND** an interrupted claim or unobserved acknowledgement SHALL retain its exact work and reservation without declaring rollback or completion
- **AND** controller exposure SHALL remain unavailable until the complete Native control mirror, durable finish, input routing and owned HoldRelease semantics have been integrated and proven

#### Scenario: Admitted parameter effects follow their living content
- **GIVEN** a Gain or EQ edit was admitted for a loaded content before pair publication
- **WHEN** Move or Swap completes before the parameter queue drains
- **THEN** coalescing and application SHALL resolve and revalidate that edit's original Native content key and parameter epoch
- **AND** it SHALL affect only the same surviving content at its resolved placement, even when another content has equal numeric parameter epochs
- **AND** an edit admitted for an empty slot SHALL require its exact original empty snapshot rather than gain authority over a later occupant

#### Scenario: Own pair acknowledgement controls the complete Native control mirror
- **GIVEN** a living pair has published its own complete Native acknowledgement while control caches still name the original placements
- **WHEN** control reconciles that same owning transaction
- **THEN** it SHALL validate both actual post-claim keys, lifetimes, physical epochs and reservation owners and move all currently owned source, lease, digest, timing-record, resident-selection, PCM-history and MIDI-content rows in one shared mutation claim
- **AND** preparation epochs, acknowledged timing owners and per-content authority counters SHALL remain at their original stable cells while physical request counters SHALL remain monotone at their physical slots
- **AND** moved logical source generation SHALL remain valid even when the destination's physical request counter is zero or differs from that generation
- **AND** the mirror SHALL preserve legitimate newer same-content window, timing and intent state rather than reinstall an older captured cache image
- **AND** Move SHALL install independently prepared canonical empty control defaults and release displaced owners after leaving the mutation marker
- **AND** interrupted, pending or failed reconciliation SHALL retain original custody and reservations without rolling Native back, repeating the pair mutation or declaring durable completion

#### Scenario: Admitted source work waits for a coherent control placement
- **GIVEN** an admitted worker or held source lease retains the original Native content key and the source, window, timing, intent and preparation owners required by that operation
- **WHEN** Move or Swap changes Native placement before the complete control mirror is visible
- **THEN** control placement resolution SHALL report Pending and retain the same work, result and owners until that mirror is coherent
- **AND** after the mirror it SHALL resolve only the same surviving content and apply its original complete operation guards at that placement without obtaining permission from a fresh ticket, equal PCM or equal counters
- **AND** source-only complete readers SHALL retain immutable source custody across pure placement, timing and resident-window maintenance while removal or source replacement SHALL revoke that original source permission
- **AND** a delayed prepared-stem result or loader event SHALL route by its admitted Native owner and SHALL NOT affect a different content sharing the same PCM, timing or numerical counters at the former slot
- **AND** prepared results blocked by control placement or command capacity SHALL retain the exact result, original ticket and file leases in bounded control-owned storage and retry without rerunning preparation or acquiring a new ticket
- **AND** background task admission and terminal cleanup SHALL follow the original stable content cell and exact task owner so an older task cannot clear a successor's activity
- **AND** a previously queued resident result acknowledged after Pair audio publication but before its control mirror SHALL reconcile its own acknowledged window into the original custody row under the existing mutation boundary before Pair mirror validation, retaining external finish for later coherent placement
- **AND** legitimate original-key control-Pending time SHALL NOT consume a private preparation's existing work deadline; original capture, queued Native adoption and queue-capacity bounds SHALL remain effective at their actual stages
- **AND** the complete MIDI content mirror SHALL preserve original source, authority, timing and runtime guards while updating the bound resident descriptor to the same source's actually acknowledged current window before publishing the control placement

#### Scenario: Accepted transport intent and caused pause are distinct
- **GIVEN** a physical hold press has an accepted Native action bound to one surviving content
- **WHEN** the callback applies that action
- **THEN** only actual unpaused-to-paused voice lanes SHALL create its owned pause effect with exact Native cohort generations
- **AND** later accepted stop, retrigger, explicit resume or intentional pause SHALL fence that effect through a monotone transport-intent revision distinct from scheduler launch-cancellation permission
- **AND** exhausted action or cohort generations SHALL fail closed rather than reuse pause authority
- **AND** placement continuity SHALL preserve the same transport intent and owned effect while removed lifetimes SHALL retire it
- **AND** successive accepted Pause, Seek, and Resume commands for the same surviving content SHALL execute in queue order; a later transport-intent revision SHALL revoke old Hold-release authority without discarding an earlier distinct accepted command

#### Scenario: Reused authority cell rejects old asynchronous writers
- **GIVEN** a removed content's Native authority cell is reused under a fresh nonreused lifetime
- **WHEN** an old acknowledgement, timing clear, revocation, key-lock result or other asynchronous publication arrives
- **THEN** the writer SHALL validate its captured engine, cell and lifetime before modifying current state and SHALL NOT resolve a new occupant from its former numeric slot
- **AND** an earlier incarnation's acknowledgement or revocation counter SHALL NOT certify or permanently invalidate the new content's own lower counter
- **AND** an explicit revocation accepted for the new incarnation before its first timing ACK SHALL remain effective after that ACK arrives

#### Scenario: Source publication and pair claim share one bounded mutation boundary
- **GIVEN** a prepared source assignment and a prepared pair name the same captured Native placement
- **WHEN** their control or callback writers attempt to publish concurrently
- **THEN** one bounded Native mutation claim SHALL serialize the complete writes and the other writer SHALL observe Pending or stale state before changing any source, request, lifetime or placement
- **AND** no writer SHALL wait or spin in the callback, infer rollback after irreversible claim, or publish a partly changed image
- **AND** an actual occupant replacement SHALL check and advance that physical slot's binding epoch and return the complete committed snapshot with its fresh lifetime
- **AND** source or window maintenance for the same content SHALL preserve its content lifetime and occupant binding epoch while retaining all existing source and window guards
- **AND** a queued source payload, its own acknowledgement and any permitted rollback SHALL retain the captured Native owner and committed incarnation through the existing retirement path
- **AND** a transient publication marker alone SHALL NOT withdraw already effective permissions for unchanged content

#### Scenario: Explicit timing edits use the same complete mutation boundary
- **GIVEN** a control edit captures a participant's slot, content incarnation, source guard, prepared epoch and timing authority
- **WHEN** an intent, legacy BPM or legacy origin edit competes with a source, window or pair publication
- **THEN** it SHALL publish ACK revocation, timing authority, declared intent and prepared epoch together under the same bounded mutation claim
- **AND** Pending, a stale captured placement or changed epoch or authority SHALL reject before any of those writes or queue publication
- **AND** a successful same-content edit SHALL preserve the content lifetime and slot-binding epoch and queue the actual committed snapshot
- **AND** an empty slot edit SHALL NOT inherit the replaced content's timing ACK

#### Scenario: Finished timing evidence survives a temporarily unpublished placement
- **GIVEN** an admitted preparation has completed actual raw backend evidence for its original content
- **WHEN** an unrelated mutation marker or a Move/Swap control mirror is Pending at preparation finish
- **THEN** finish SHALL retain and return the same immutable evidence ticket only after checking the original Native engine, nonreused content key/lifetime, logical request incarnation, direct preparation epoch owner/value, source/PCM/rate/extent/custody and Automatic intent
- **AND** Pending SHALL NOT create a publication permit, accepted record, queue command, acknowledgement or new source request
- **AND** removal, equal-PCM refill, changed request/epoch/source or Manual/TAP/Legacy intent SHALL remain stale and SHALL NOT be accepted merely because placement is Pending
- **AND** later publication SHALL require complete current placement and one shared same-content mutation claim; all fallible adoption checks, allocation, resource and queue preparation SHALL precede that claim and displaced large owners SHALL retire after the marker closes

#### Scenario: A dispatched timing publication retains its original retry owner
- **GIVEN** explicit timing publication already owns its original evidence ticket, independent assessment and actual worker Future
- **WHEN** intrinsically valid placement Pending prevents publication before claim
- **THEN** Native SHALL report a distinct typed transient result without consuming adoption authority or changing epoch, record, permit, queue or acknowledgement
- **AND** the existing Controller SHALL retain the same publication owner, original worker outcome, ticket, request metadata and immutable assessment through Move/Swap and retry the same arguments at most once per existing control poll after the owning placement is observable
- **AND** retry SHALL NOT recapture a slot/source, rerun raw backend analysis, invent a new request or grant authority to equal numeric material
- **AND** an own Pending placement SHALL defer publication and derived-refresh observer interpretation rather than treat a temporarily unavailable lookup as removal
- **AND** terminal worker errors, stale/removal/refill and superseding intent SHALL remain failures; a retry or completed worker alone SHALL NOT certify acceptance or derived timing without the existing genuine Native acknowledgements

#### Scenario: Pending closes before an unavailable reader is interpreted
- **GIVEN** a control poll owns an original publication or waiting/active derived-refresh observer
- **WHEN** a Native read returns unavailable during a marker that closes before later Boolean Pending probes
- **THEN** a checked Native observation generation SHALL preserve that marker crossing for the entire control read interval
- **AND** both successful and rejected claims SHALL change that observation generation even if rejected layout revision is restored unchanged
- **AND** the poll SHALL retain its original observer without consuming an admission-failure budget, committing saved grid/master state or invoking completion callbacks
- **AND** the next readable poll SHALL freshly check the original ownership and retire genuine stale state rather than infer authority from the observation token
- **AND** observation generation SHALL NOT wrap or grant source/timing authority; already exhausted generation SHALL yield no certifiable read token and prevent further content mutation

#### Scenario: An admitted empty-slot BPM survives an unrelated pending source write
- **GIVEN** an admitted legacy BPM names an empty slot's Native owner, stable cell, binding epoch and empty lifetime
- **WHEN** an unrelated source writer holds the global publication marker during dequeue, coalescing or parameter application
- **THEN** the callback SHALL preserve and apply that edit while all of its own captured fields and existing source and parameter guards remain current
- **AND** a real replacement or empty-slot ABA SHALL still reject that old edit before changing BPM, prepared epoch or a later content's ACK

#### Scenario: MIDI runtime replacement publishes all captured cells together
- **GIVEN** a complete runtime replacement captures all 216 slot bindings, original runtime revisions and loaded source permissions under the existing runtime owner
- **WHEN** a concurrent Native publication is Pending or a captured source, window, timing, placement or runtime guard changes before claim
- **THEN** replacement SHALL reject without changing any runtime cell or the previously published runtime state
- **AND** all allocations, locks, array validation and revision overflow checks SHALL finish before one shared same-content mutation claim
- **AND** a successful claim SHALL publish only the prepared revisions into their captured cells and exchange the complete runtime state without a later numeric-slot lookup
- **AND** unchanged entries SHALL preserve their existing runtime revisions and admitted permissions
- **AND** semantic comparison of an unchanged binding SHALL ignore an unrelated global publication revision while retaining its exact Native owner, slot, cell, lifetime, binding epoch and complete source, window, timing and authority projection
- **AND** the old runtime state SHALL be retired outside the publication marker

#### Scenario: KeyLock intent and resident arming publish one captured content image
- **GIVEN** a KeyLock control edit or ready resident window captures the actual slot snapshot, complete source binding, original intent and request counters
- **WHEN** Pending, replacement, changed source/window/timing, exhausted counters or unavailable queue/resource/lock capacity prevents publication
- **THEN** the edit SHALL reject before changing the current intent, request, resident owner or command queue
- **AND** all payload allocation, geometry, ACK, resource and proposal preparation SHALL precede the shared same-content mutation claim
- **AND** a successful claim SHALL publish the prepared intent, request, resident owner and command together through the captured stable cells, preserving the content lifetime and slot-binding epoch
- **AND** empty saved intent SHALL retain its actual empty slot snapshot and SHALL reject replacement or empty-slot ABA, while an unrelated Pending marker SHALL NOT discard its queued effect
- **AND** an admitted loaded KeyLock action SHALL resolve its captured content key before applying its original complete source/window/timing/request guards, rather than operate on a later occupant of its former slot
- **AND** asynchronous resident failure and external finish SHALL settle only their captured content and request under an outer mutation claim before terminal state is visible; transient Pending SHALL retain settlement for retry
- **AND** Cold and resident callback completion already inside that mutation claim SHALL use direct captured cells without a nested claim

#### Scenario: Launch and cancellation publish the complete captured participants
- **GIVEN** a launch, ordered stop, pure cancellation or migration fence captures its actual slot snapshots and original launch flags, revisions and hold owners
- **WHEN** placement is Pending, a captured occupant or scalar changes, or required queue, lock or revision capacity is unavailable
- **THEN** the operation SHALL reject before changing any participant flag, revision, hold or queue payload
- **AND** bulk cancellation and global playback SHALL validate every captured participant before one shared same-content mutation claim and SHALL NOT perform sequential partial claims
- **AND** successful apply SHALL write only the prepared scalars through their captured stable cells and publish the already prepared command
- **AND** pure cancellation SHALL retain admitted flags until an actual ordered stop, while ordered stop SHALL clear only its captured ownership flags
- **AND** cancellation SHALL never wrap a terminal launch revision; a newly assigned lifetime SHALL NOT inherit an earlier incarnation's exhausted launch permission
- **AND** a fresh source lifetime SHALL clear the predecessor's resident-control fence within that birth claim, while later cleanup of the retired lifetime SHALL NOT clear an equal-valued successor control
- **AND** shutdown SHALL disable admission and settle the complete launch fence before reporting successful shutdown; transient publication SHALL be retried off realtime and unresolved settlement SHALL be reported without claiming revocation
- **AND** voice birth SHALL retain the admitted original source key, lifetime, complete source/window/timing guards and launch permission through its actual Native mutation claim; a later equal-PCM assignment SHALL NOT manufacture replacement permission
- **AND** a due launch delayed by Pending SHALL retain the identical owned command, original target, received time and ordering in preallocated callback state, retry at most once on the next callback and prevent later work from overtaking it without schedule reinsertion or realtime allocation
- **AND** queued global effect or feedback capacity failure SHALL preserve atomic rejection when its payload can retire safely; with no payload-retirement slot the exact owned command SHALL be retained before any stale rejection, without leaking or destroying its final owner on realtime
- **AND** the deferred slot SHALL be allocated during control startup and SHALL preserve the existing normal Windows thread-stack initialization bound
- **AND** already admitted immediate and quantized launches and per-content stops SHALL resolve their original content keys at dequeue and execution while preserving their original source, window, authority, timing and applicable runtime guards
- **AND** new fixed-slot admission SHALL still reject an old snapshot after Move, Swap, replacement or empty-slot ABA

#### Scenario: Committed stem arming and failure retain their own control authority
- **GIVEN** a committed-stem worker captures complete source and timing permissions, original intent, resident control and request counters
- **WHEN** it reaches final publication or fails before actual callback acknowledgement
- **THEN** all resources, locks, proposal prerequisites, displaced-owner preparation and complete queue payload SHALL precede the shared same-content claim
- **AND** successful apply SHALL publish the captured intent, resident owner and command together without a numeric-slot re-resolution or final PCM-owner destruction inside the marker
- **AND** an armed but never published proposal SHALL NOT settle a newer resident owner with an equal counter
- **AND** external failure SHALL retain its captured cleanup during Pending and SHALL settle it before publishing terminal rejection or discarding its retry owner
- **AND** ordinary withdrawal or supersession SHALL preserve the established public ownership-change error, while actual worker and deadline failures SHALL retain their detailed diagnostics
- **AND** an armed preparation whose captured control has not yet been published SHALL remain current and pending through its own valid proposal, without projecting a transient rejection to the public controller
- **AND** cancellation or supersession of that unpublished proposal SHALL still reject it, while a published receipt SHALL require its actual control epoch
- **AND** external cancellation SHALL serialize with final control arming so terminal rejection cannot precede settlement of a control published by that same arming transaction
- **AND** callback adoption SHALL claim the same mutation boundary, retry the identical queued payload on Pending and complete only its captured cells without nested claims

#### Scenario: Cold cancellation retains its actual published incarnation
- **GIVEN** a cold or migration ticket retains its original private adoption state and eventual actual source publication
- **WHEN** cancellation is requested before source arming or races callback adoption
- **THEN** unpublished cancellation SHALL prevent its worker from arming the source
- **AND** published cancellation SHALL claim its captured content incarnation and atomically settle its original adoption and native cold row before reporting success
- **AND** Pending SHALL retain that exact cancellation for retry without changing another occupant or terminalizing a still unresolved owner
- **AND** actual callback claim or acknowledgement SHALL prevent cancellation from implying rollback
- **AND** preparation rollback and worker timeout SHALL retain artifacts and leases whenever actual claim or unresolved published cancellation may still own them
- **AND** a private claim phase SHALL NOT authorize source suspension or revocation after the Native callback may already have completed its acknowledgement; unresolved workers SHALL preserve that authority and report a nonterminal diagnostic
- **AND** worker shutdown SHALL finish promptly when a Pending mutation prevents exact cancellation, retaining the unresolved publication and artifacts without inventing a terminal rejection or native acknowledgement

#### Scenario: Source cleanup respects later timing intent
- **GIVEN** a tagged unload or failed Cold publication still names its exact Native owner, source, cell, lifetime and slot-binding epoch
- **WHEN** a later legitimate timing edit updates that same participant's authority or intent
- **THEN** structural unload, cancellation or rollback SHALL retain their complete source-identity guards while preserving that newer timing intent
- **AND** that timing edit alone SHALL NOT strand the admitted unload or prevent disposal of the failed source
- **AND** an initial empty-slot legacy Cold load without an analysis timing guard SHALL survive that edit, while a load captured with a timing guard SHALL still reject a changed epoch or authority

#### Scenario: Transient publication contention retains the queued Native payload
- **GIVEN** a valid queued source, Copy or resident publication retains its complete captured guards and owners
- **WHEN** a concurrent writer acquires the Native mutation marker before the callback can claim that publication
- **THEN** Pending alone SHALL NOT reject or retire that payload as if its source had changed
- **AND** the callback SHALL retain the same bounded payload for a later guarded attempt without spinning, allocating, requeueing or consuming later work out of order
- **AND** each attempt SHALL recheck its complete guards and existing feedback and retirement capacity before any effect
- **AND** a successful retry SHALL perform the effect once, while a genuine stale guard SHALL reject before effect and retire through the existing lane
- **AND** shutdown SHALL retain or retire that deferred owner's actual pins through the same ordered containment path

#### Scenario: Stream closure transfers deferred and backlog owners off realtime
- **GIVEN** the callback retains a deferred publication or retirement backlog and the ordinary retirement lane is full
- **WHEN** the device backend destroys its callback closure on any thread
- **THEN** a separately reserved bounded close lane SHALL transfer the callback's complete owned state to the existing off-realtime retirement worker without waiting, allocating or destroying final payload owners on that backend thread
- **AND** that close lane SHALL remain available independently of ordinary retirement capacity and SHALL carry the complete callback owner at most once
- **AND** worker shutdown SHALL wait off realtime for closure-owner handoff and finish releasing its deferred, queued and backlog pins before reporting completion
- **AND** failure to start that worker SHALL reject stream construction before the backend can own the callback

#### Scenario: Loader delivery retains an event already dequeued during source publication
- **GIVEN** a current loader event precedes later events in the actual loader channel
- **WHEN** a source writer acquires the Native publication marker before its epoch lookup, including after that event was dequeued
- **THEN** polling SHALL retain that same event before later channel work until the complete source publication is visible
- **AND** replay SHALL deliver it at most once in original order while preserving the existing request, timing-epoch and source filters
- **AND** this temporary wait SHALL NOT alter the participant's source, request, prepared epoch or accepted timing

#### Scenario: Move and swap two running contents
- **WHEN** left drag moves A to emptyT or swaps A with B
- **THEN** moved source becomes empty or both contents exchange placement atomically
- **AND** live playback and paused cohorts follow their content, selection/editor follows selected content
- **AND** fixed slot MIDI/controller layout stays unchanged

#### Scenario: Selected content follows one owning Pair projection
- **GIVEN** one acknowledged Move or Swap retains its complete original participants and controller mirror
- **WHEN** its current selected content and project image are projected or retried after an interrupted store
- **THEN** the saved Config candidate and applied controller selection SHALL use the same owning selection projection, following surviving A or B exactly once without swapping an already projected selection again
- **AND** selection of an empty Move target SHALL retain that target, while unrelated navigation SHALL retain its chosen pad
- **AND** every accepted GUI or mapped selection, including a repeated same-slot choice, SHALL retain a fresh bounded process-local intent revision and the content chosen at admission
- **AND** newer navigation SHALL follow that chosen surviving content or preserve its unrelated or empty target through partial mirror settlement, even when its numeric pad equals an earlier before or after value
- **AND** the process-local selection record SHALL NOT be persisted or grant audio, timing, Hold or future pitch authority

#### Scenario: Complete controller settlement retains current independent owners
- **GIVEN** a request owns both original Native participants and its own opaque Pair acknowledgement
- **WHEN** the corresponding controller mirror settles Move or Swap
- **THEN** it SHALL project Session, transport, residency, global playback, accepted timing, loader, waveform, registered UI ranges and telemetry through one owning plan preserving exact Futures, tickets, callbacks and original timestamps
- **AND** it SHALL merge the latest unrelated rows, navigation and global intent before stores and retain each store's original before and recognized own-after image across an interrupted retry
- **AND** retry SHALL reuse the same applied plan and own acknowledgement without swapping an afterimage again, recapturing a slot or cancelling current content work
- **AND** the current Stem mirror SHALL be prepared at final settlement and thereafter retain the same exact projected entries, subscribers and applied plan
- **AND** participant observers SHALL defer unresolved participant rows while polling unrelated work
- **AND** a dequeued loader result SHALL retain its opaque original Native delivery owner through a Python fence and SHALL resolve that owner again before dispatch; changed source/task ownership SHALL discard obsolete results without granting the former numeric slot permission
- **AND** progress coalescing SHALL preserve original admission identity and exact terminal results while unrelated subscriber delivery continues through the existing shared-job route

#### Scenario: Logical source requests and delayed input retain original authority
- **GIVEN** a surviving source request or an admitted MIDI trigger waits through Move, Swap or resident window preparation
- **WHEN** it is compared or retried at the resolved original content placement
- **THEN** a source request SHALL compare its engine, original key and lifetime, stable content request incarnation, source generation, version, rate and backing while physical request counters remain monotone placement diagnostics
- **AND** analysis supersession SHALL change the logical request incarnation while legitimate same-source resident window refresh SHALL preserve it
- **AND** delayed input SHALL retain the original opaque mapping and TriggerPad binding, timestamp, exclusive or multi-loop intent and complete source, window, timing, runtime, launch and transport guards
- **AND** its controller readiness retries SHALL preserve the existing eight-attempt admission bound and the original thirty-second deadline without resetting either through queue pressure or content placement changes; an exhausted owner SHALL revoke only its original window launch and retain that exact window until its own cleanup settles
- **AND** repeated delivery of that exact opaque trigger SHALL enter its existing controller owner before any direct dispatch; an exhausted trigger without a window SHALL remain in bounded original custody until its Native source or gesture becomes terminal, and SHALL NOT acquire a fresh readiness budget
- **AND** a numeric callback or telemetry slot SHALL NOT grant authority to a later occupant
- **AND** captured explicit timing, saved timing restore and saved timing export SHALL resolve their original Native source and logical request after placement changes while rejecting a superseded request, timing intent, source, backing or publication epoch
- **AND** callback feedback SHALL retain its actual callback content key and transport intent even inside its own publication marker, and an armed but unaccepted successor SHALL NOT inherit the predecessor's telemetry
- **AND** playback effect feedback SHALL capture its originating transport revision inside the same Native effect claim; later control admission between effect and feedback SHALL NOT retag the earlier outcome as that newer gesture

### Requirement: Copy overwrite removes only the old target
The occupied Copy transaction SHALL have its own strict versioned durable journal,
separate from Move/Swap lineage. Its immutable evidence SHALL retain original
source A and predecessor B, one fixed fresh stopped successor C, resolved KEY LOCK,
the captured complete project image, actual Config binding/digest/revisions and
successor lineage. Runtime tokens SHALL NOT be persisted or reconstructed as authority.
Later precommit images and actual receipts SHALL retain legitimate surviving-source
and unrelated intent; the fixed C seal SHALL NOT be regenerated from newer A.

#### Scenario: Occupied Copy records its irreversible boundaries
- **GIVEN** one complete captured occupied-target Copy owns its before/after source and stem leases
- **WHEN** it prepares, publishes, observes its own Native acknowledgement, writes Config and settles
- **THEN** the distinct schema1 kind pad_copy_overwrite SHALL record captured before preparation, publication_pending before enqueue and native_ack_observed before predecessor-controller retirement
- **AND** config_committed SHALL require the actual same-owner ConfigCommitReceipt and settled SHALL follow complete model, asset and Native finish
- **AND** cancellation SHALL record terminal cancellation only after proven preclaim cancellation and worker return
- **AND** uncertain append SHALL retain the exact original bytes and same journal owner for reopen/retry
- **AND** independent original/stem leases for A, B and fixed C SHALL survive through actual terminal append, even if old-target retirement is already queued

#### Scenario: Copy restart distinguishes original input from its written successor
- **GIVEN** complete strict Copy journal history and actual Config bytes are retained across process exit
- **WHEN** startup reconciles the original captured digest, recorded pre-C intent or deterministic fixed-C afterimage
- **THEN** recognized C SHALL NOT be recopied or assigned a new durable identity
- **AND** the actual target SHALL obtain fresh current Native source/selection/timing acceptance before durable settlement, without claiming currentness from historical tokens or ordinary source enqueue
- **AND** original-input replay SHALL require the restored original A and B seals rather than copy changed material from their numeric slots
- **AND** source A changes or deletion in a recognized afterimage SHALL be preserved while only C is reserved for recovery
- **AND** different Config, unknown schema/history, missing original/stem custody, missing ancestor, fork, cycle, ABA or incompatible unfinished Material/Pair/Copy owners SHALL retain evidence and visibly fence conflicts without stealing a writer or declaring success/rollback

#### Scenario: Recovered Copy waits for its actual selected stems
- **WHEN** recognized fixed C contains a saved immutable StemPair selection
- **THEN** recovery SHALL keep ordinary source, timing and stem observers reachable until the existing preparation and registration path has verified exactly that selection against C's fresh own source
- **AND** verification of an unchanged already saved selection MUST retain its prepared registration and disk readers without replacing metadata, generating another set, committing another disk selection, or competing with the recovery Config writer
- **AND** the existing selection owner MUST prove its current own Native registration, content, source, path and full immutable pair binding before recovery captures C
- **AND** saved ALL STEMS desire SHALL additionally require the current own component and effective-mask acknowledgement; enabled startup preload SHALL require its own accepted preload result
- **AND** FullMix with preload disabled MUST require the selected disk registration without adding a component or preload request
- **AND** missing or stale selections, Pending and rejected component results SHALL retain the recovery fence and exact journal; a stable empty selection epoch or ordinary source ACK alone MUST NOT certify the selected C

#### Scenario: Recovery owns custody before ordinary restore and cleanup
- **WHEN** startup finds unresolved occupied Copy evidence for the current Config
- **THEN** it SHALL reconstruct the original before/after pins before loader restore and artifact cleanup
- **AND** startup gesture fences SHALL NOT block the genuine fresh loader/selection acceptance that recovery requires
- **AND** close, status and conflicting global KEY LOCK edits SHALL observe the same recovery owner until terminal settlement

#### Scenario: Portable relocation recognizes complete transaction kinds
- **GIVEN** sealed Native journal stores contain valid Legacy Material, schema1 Pair and schema1 occupied Copy evidence
- **WHEN** Native portable capture and same-FileID relocation validate those journals
- **THEN** each kind SHALL use its own exact schema, immutable seals, complete history and successor checks while preserving original bytes and Config references
- **AND** unknown kinds, changed kinds, malformed or incomplete records SHALL reject before relocation output rather than weaken Legacy exact-field validation
- **AND** relocation SHALL NOT grant runtime acceptance or inspect a known foreign Config's contents

The system SHALL leave source content/playback/holds unchanged when copying to empty or occupied target and SHALL stop/fence only the removed target lifetime in the same atomic commit creating a fresh stopped copy.

#### Scenario: Playing source copied over playing target
- **WHEN** right drag copies A over B
- **THEN** A continues unchanged, B stops/retires and newC remains stopped with independent settings
- **AND** later B action/release/telemetry cannot affectC

### Requirement: Whole bank mutation is one complete guarded image
The system SHALL copy or clear exactly36 target slots including empty source slots through one prepared atomic native/project/config/journal transaction with all refs/capacity reserved before any old reference is released.

#### Scenario: Mixed source bank and live target bank
- **WHEN** confirmed BankCopy commits all36 from sourceS to otherT
- **THEN** nonempty copies are fresh stopped and empty source slots remove corresponding targets
- **AND** only removed target playback/holds retire; source voices/holds/selection stay unchanged

#### Scenario: Cancel failure or irreversible claim without ACK
- **WHEN** operation cancels or fails before native claim
- **THEN** occupancy/epochs/selection/voices/holds/owners/config stay unchanged
- **WHEN** claim becomes irreversible without observedACK
- **THEN** old/new/action pins remain and conflicts are fenced until genuine terminal recovery
- **AND** no guessed success/rollback, early release or36 partial unloads is reported

### Requirement: Arrangement failures and no-op gestures preserve state
The system SHALL treat cancelled/self/invalid/empty-source drops and failed pre-claim mutations as complete no-ops and SHALL reject unsafe contained paths or exhausted native/action/feedback/retirement capacity without partial mutation.

#### Scenario: Invalid drop while source is held
- **WHEN** a cancelled or invalid drop is attempted on heldA
- **THEN** layout/selection/playback/hold/ownership remains unchanged and normal release still settlesA
