## ADDED Requirements

### Requirement: Ordinary attacks inherit the actual audible tuple on their first block
The system SHALL retain each ordinary GUI, MIDI, resident and global start as an original content-bound attack before residency preparation. The existing audio callback SHALL capture the ordinary transport target T once before an attack's own window publication. Later readiness SHALL retain T and SHALL expose a separate execution frame E without quantizing again. The callback SHALL freeze the actual applied pitch and Key Lock tuple for preparation; an ordinary attack MUST NOT write a requested tuple into the bank or falsely acknowledge another pending pitch action.

#### Scenario: A saved nonneutral pad starts after its own window acknowledgement
- **GIVEN** a loaded pad has a saved applied nonneutral pitch tuple and needs its own resident window
- **WHEN** its original ordinary attack receives its own WindowACK and the exact Native source preparation becomes ready
- **THEN** the one transport scheduler executes that original attack at its recorded E with the exact prepared source and FIFO from its first audible block
- **AND** the original T, content lifetime, source authority, input timestamp, volume and exclusive intent remain owned by that attack
- **AND** a stale, cancelled, mismatched or late preparation produces no attack and an observable rejection

#### Scenario: A global start has one prepared atomic effect
- **GIVEN** a global start captures several original pad attacks
- **WHEN** all original participants and their exact prepared sources are validated under the existing complete mutation claim
- **THEN** all participants start at the same recorded E through one scheduler event
- **AND** a failed participant causes no partial audio start
- **AND** preparation and retirement perform no Native processing, allocation, lock acquisition or final heavy owner destruction on the callback

### Requirement: A deferred global gesture retains its original native owners
The system SHALL capture all GLOBAL START participants, source lifetimes, timing authorities, requested loop regions and one input timestamp in a native group before waiting for residency. The existing callback SHALL capture that group's original transport target T once. Publication after readiness MUST NOT recapture participants, replace their source authority, recompute their loops or quantize T again. Each residency dependency SHALL retain the actual native ticket present at gesture capture and SHALL resolve only that ticket's genuine accepted publication and adopted binding. A matching later ticket or matching numeric metadata MUST NOT grant readiness.

#### Scenario: An existing resident edit finishes after the global gesture
- **GIVEN** a GLOBAL START participant has an already admitted resident edit with a genuine native ticket
- **WHEN** GLOBAL START captures that edit's requested loop and original native source before its WindowACK
- **THEN** the exact dependency SHALL retain that original participant through its own WindowACK and placement changes
- **AND** the complete group SHALL use its original T and one common E through the existing scheduler
- **AND** Stop, source replacement, a failed dependency or a failed participant SHALL reject the entire group without partial starts

#### Scenario: A pending edit has no durable native request owner
- **GIVEN** a participant's pending resident edit has no native ticket, including an unaccepted queue-pressure retry
- **WHEN** GLOBAL START requests that participant
- **THEN** the complete gesture SHALL report bounded readiness unavailability before accepting a captured group
- **AND** polling SHALL NOT attach that gesture to a later ticket by comparing fields
- **AND** this rejection SHALL NOT be presented as preservation of an accepted gesture through queue pressure

#### Scenario: A published global start is rejected before its effect
- **GIVEN** the complete original group has been published and its batch ticket is still pending
- **WHEN** Native rejects the group before its atomic effect, including after a placement change
- **THEN** the controller SHALL expose the original rejection on the group's current diagnostic pads after the owning placement mirror is ready
- **AND** the rejected ticket SHALL NOT fabricate active playback or discard the preceding remembered restore intent

### Requirement: Ordinary attacks share exact physical readiness independently
The system SHALL allow equal original ordinary attacks to register distinct Native memberships in one exact source/timing/loop/context readiness request. Each attack SHALL retain its own owner, Work, sequence, input time, target T, ticket association and cancellation. Cancelling one member MUST NOT cancel another member or an independently requested controller edit. A controller edit dependency SHALL be retained as its exact existing Native ticket before polling; its original mode request SHALL receive its own real WindowACK and MUST NOT be superseded by preparing the dependent ordinary attack. A matching later request MUST NOT substitute for that dependency.

#### Scenario: Equal attacks share real physical readiness
- **GIVEN** two ordinary attacks capture the same living source, timing and loop before readiness
- **WHEN** they register with one genuine Native window request and receive its actual callback ACK
- **THEN** both SHALL retain distinct Work and scheduled effects in original input order
- **AND** cancelling either member SHALL leave the other member's request and attack valid
- **AND** a different region, source, timing or cancelled request SHALL fail the shared membership check

#### Scenario: An ordinary start depends on an existing loop and mode edit
- **GIVEN** a controller loop/mode edit already owns a Native request and its own request ID
- **WHEN** an ordinary start retains that exact ticket before waiting
- **THEN** its distinct member ticket SHALL resolve the same real WindowACK and actual mode request ID
- **AND** the independently requested edit SHALL remain accepted when the start is cancelled

#### Scenario: The original dependency commits after the ordinary target is captured
- **GIVEN** an ordinary start retains an already admitted loop edit's exact native ticket and the callback captures its original target and source-control revision before that edit commits
- **WHEN** that same edit performs its own successful callback loop commit and receives its genuine accepted acknowledgement
- **THEN** the original start SHALL retain its immutable target and captured revision while accepting only that dependency's own proven before/after source-control revision transition
- **AND** a prepared nonneutral start and an atomic global participant SHALL retain that same exact dependency association through admission, late preparation and the final effect claim
- **AND** a deferred, failed, cancelled, rejected, foreign or storage-only publication SHALL NOT provide that permission

#### Scenario: A separate edit cannot borrow a dependency's continuity receipt
- **GIVEN** an original start retains an exact loop dependency's accepted callback acknowledgement
- **WHEN** an unrelated source-control edit occurs before or after that dependency's own commit, including a change followed by restoration
- **THEN** the original start SHALL reject its stale continuity proof without an attack or a partial global effect
- **AND** matching numeric loop endpoints, current counters or another ticket's acknowledgement SHALL NOT replace the original dependency's commit provenance

#### Scenario: A fresh member follows benign input projections
- **GIVEN** an original attack is scheduled against its genuine accepted physical readiness
- **WHEN** input routing projections change and a fresh attack captures the unchanged source, timing, window and loop
- **THEN** the fresh attack SHALL reuse that same physical readiness without superseding the scheduled attack
- **AND** each member SHALL still validate its own original input/runtime guards; an older unpublished capture SHALL NOT acquire fresh permission from another member

### Requirement: A timing edit preserves its coupled resident acknowledgement
The system SHALL finish a timing edit's legacy BPM/origin publications and final Manual or Tap intent declaration before admitting its coupled resident loop/Key Lock refresh. Intermediate grid-offset reclamping SHALL NOT publish a resident refresh that the same edit subsequently revokes. The coupled refresh SHALL retain its own original source, window, timing authority and request ID through the actual callback acknowledgement; a later request ID alone MUST NOT acknowledge its effect.

#### Scenario: Manual or Tap timing refreshes a saved Key Lock pad
- **GIVEN** a loaded finite pad has acknowledged saved Key Lock ON and an existing callback mode receipt
- **WHEN** a Manual or Tap BPM edit, or a refresh of that declared timing, also reclamps its grid offset and refreshes its loop
- **THEN** the final Native timing declaration SHALL precede the one coupled resident request
- **AND** that edit SHALL NOT invalidate its own request before its callback acknowledgement
- **AND** a subsequent prepared ordinary attack SHALL expose actual wet readiness under the acknowledged request without replacing its ownership guards with the latest counter

### Requirement: Selected Normal attacks may consume exact endpoint readiness
The system SHALL automatically prepare at most one currently selected Normal-start endpoint from the actual settled Native source, window, timing, loop, stem selection, output format and applied pitch/Key Lock tuple, for both stopped and already playing content. The Normal endpoint SHALL remain independent of a live voice's evolving chronology. Selection SHALL publish only preparation interest; SourceACK and WindowACK SHALL remain distinct from actual Native endpoint readiness. Background preparation SHALL use the existing worker at lower priority without occupying any of the 64 accepted action slots or action request cells.

The same unique endpoint MAY serve an original pitch SET_RETRIGGER whose captured
tuple exactly matches the actual applied tuple and complete Normal endpoint. Its
cache request MUST NOT substitute for that action's original content owner,
sequence, timestamp, T, frozen E, own epoch or fresh source/timing permit. SET and
an unseen or different tuple SHALL retain their existing preparation semantics.

#### Scenario: Same applied pitch retriggers consume ready Normal state
- **GIVEN** stopped or playing selected content has a genuinely prepared exact Normal endpoint for its actual applied nonneutral tuple
- **WHEN** an in-time original GUI or MIDI SET_RETRIGGER captures that same tuple and endpoint
- **THEN** it SHALL consume the unique prepared Native/FIFO with E=T through the existing scheduler and its complete original action claim
- **AND** an already playing voice SHALL retain its Native/FIFO, source trajectory and PCM before T
- **AND** the candidate SHALL retain the fresh action's own request and permit while the displaced cache request retires outside realtime without Native reset, construction or data processing on the callback

#### Scenario: Ordinary and pitch attacks compete for one consumable endpoint
- **WHEN** ordinary and same-target pitch retriggers, or several pitch retriggers, are admitted against one ready endpoint
- **THEN** at most one action SHALL consume that actual Native/FIFO and each accepted action SHALL retain a distinct original record, tuple, sequence, target and effect
- **AND** a consumed or mismatched endpoint SHALL leave the remaining actions on their existing preparation or rejection path without borrowing another action's ACK or epoch
- **AND** a rejected capacity or invalid action SHALL NOT consume an otherwise valid reserve, while a moved candidate SHALL never remain advertised as available even if cancellation wins before application

#### Scenario: Fresh pitch authority remains mandatory after a hit
- **WHEN** selection or the background cache epoch changes after the original pitch action consumes its exact endpoint
- **THEN** its fresh original action permit SHALL remain independent of the retired cache request
- **AND** real source, lifetime, authority, request-incarnation, window, timing, loop, stem, rate, Stop or cancellation changes SHALL still reject unsafe application without partial pitch or attack

#### Scenario: A prepared retrigger cannot restore a superseded live stem generation
- **GIVEN** an original future pitch retrigger owns prepared V1 selected-source history while the same content continues playing
- **WHEN** a separately acknowledged public live Generate/model replacement makes V2 current before the original effect frame
- **THEN** retained V1 lifetime pins SHALL NOT substitute for current V2 effect authority and the unsafe old attack SHALL be rejected as a whole
- **AND** V2 playback and unrelated voices SHALL remain continuous, with no old selection rollback, early pitch application or extra START
- **AND** a fresh V2 attack SHALL require its own original identity, T/E, current-source permit and matched prepared selected Native history rather than relabel the rejected V1 candidate

#### Scenario: A ready selected endpoint serves a fresh ordinary attack
- **GIVEN** the selected content has one genuinely prepared exact Normal endpoint
- **WHEN** a fresh ordinary attack is admitted at its immutable original target T
- **THEN** it SHALL consume that unique Native/FIFO state with E=T when admitted in time, using its own sequence, epoch, original owner and freshly captured source/timing permit
- **AND** the old cache request SHALL retire off realtime without relabelling its permit, resetting Native state, changing the physical endpoint or bypassing the existing complete effect claim

#### Scenario: An already playing pad waits for its fresh Normal attack
- **GIVEN** selected content is playing and an independent exact Normal endpoint is ready
- **WHEN** a fresh ordinary repeat waits for its original target T
- **THEN** its existing live Native/FIFO owner and source cursor SHALL remain unchanged by preparation before T
- **AND** the repeat at T SHALL adopt a distinct prepared Native/FIFO with its own original action permit

#### Scenario: A genuine own WindowACK preserves the physical endpoint
- **GIVEN** an original attack captures before its own genuine resident preparation and an exact physical endpoint is already ready
- **WHEN** that original ticket receives its actual WindowACK while only its input RuntimeProjection advances
- **THEN** the same original owner and ticket MAY consume the unchanged physical endpoint after their fresh original guards and action permit succeed
- **AND** a changed physical window or timing ACK SHALL still miss or reject; a later matching ticket or numeric observation SHALL NOT supply the original association

#### Scenario: The endpoint is stale, absent or already consumed
- **WHEN** the actual source/incarnation, window/ACK, timing, loop, selected stems, rate or applied tuple differs, or another attack already consumed the endpoint
- **THEN** that attack SHALL retain its own original T and the existing explicit preparation E or rejection
- **AND** selection changes and stale background work SHALL NOT revoke accepted actions, coalesce repeated attacks, consume their bounded queue capacity or claim readiness for other pads or unseen tuples

#### Scenario: Preparation availability does not certify acoustic causality
- **WHEN** an exact endpoint is available at T
- **THEN** its complete initial Native/FIFO PCM SHALL be preserved
- **AND** cache availability alone SHALL NOT assert the separately required first-sound, acoustic latency, click, phase, causal or Human acceptance predicates

### Requirement: Set and retrigger preserve distinct playback semantics
The system SHALL keep SET position-preserving without starting stopped content and SHALL execute SET_RETRIGGER as a fresh normal start-point trigger, including for stopped content and repeated already-active values.

#### Scenario: Same target repeated on stopped and playing content
- **WHEN** SET or repeated SET_RETRIGGER is admitted by mouse or MIDI
- **THEN** SET never starts/seeks; each SET_RETRIGGER starts/restarts from normal loop/sample start
- **AND** retrigger never toggles Play/Stop or disappears because target equals current value

### Requirement: Pitch tuple and attack use one existing scheduled event
The system SHALL apply pitch and retrigger atomically through the existing scheduler.

#### Scenario: Existing timing and failure rules apply to the complete event
- **WHEN** a pitch tuple and SET_RETRIGGER attack are admitted
- **THEN** the following complete scheduling and atomic failure contracts SHALL apply:

The system SHALL apply an admitted pitch tuple and its SET_RETRIGGER attack atomically at the same existing trigger dueframe with current guarded permits and original Quantize/SYNC intent.

Quantized waiting SHALL NOT retune currently playing audio early. Unquantized
offbeats/swing/free attacks SHALL stay immediate. Allfour Quantize/SYNC and
MultiLoop/exclusive semantics SHALL be preserved, with no parallel scheduler or
additional grid. Unsupported/unready/capacity failure SHALL expose atomic pending/
reject without partial pitch/start/stop or rerounding captured input target.

#### Scenario: Quantized pitch waits with its attack
- **WHEN** a pitch+retrigger waits for existing quantized targetT
- **THEN** old effective pitch stays untilT and the same event applies tuple+attack all-or-none
- **AND** unquantized equivalent uses its original free input time

### Requirement: Preparation sharing never replaces accepted attacks
The system SHALL keep each admitted repeated pitch attack as a distinct bounded scheduler record with its own frozen tuple and prepared permit while allowing only preparation work to share or coalesce.

#### Scenario: Rapid equal and opposing attacks while preparation waits
- **WHEN** several attacks are accepted before ACK, including equal values
- **THEN** every accepted event remains ordered and a later pitch request does not globally stale its tuple
- **AND** removed lifetime/source authority still fences unsafe execution; existing unrelated stem-start coalescing remains intact

#### Scenario: A later control projection cannot relabel an original action
- **GIVEN** two separately admitted actions for the same living content with frozen numeric tuples and original targets
- **WHEN** a later requested pitch or GUI/MIDI projection changes the mutable control row
- **THEN** each action SHALL retain its own preparation epoch, source generation, actual window and timing ACK owner
- **AND** the mutable row revision SHALL NOT alone revoke the earlier action or present its prepared state as the later tuple

#### Scenario: Mixed-origin opposing attacks consume one selected reserve
- **GIVEN** living playing content and one prepared selected Normal endpoint for its current pitch tuple
- **WHEN** GUI-origin and routed MIDI attacks with equal and opposing frozen tuples are admitted for the same future T before preparation ACK, and later publication order differs from capture order
- **THEN** at most one attack SHALL consume that prepared endpoint and every other accepted attack SHALL retain its own original work, permit, tuple and T/E
- **AND** old productive PCM SHALL remain unchanged before T, while the existing scheduler SHALL execute every accepted attack in original sequence order with its own complete pitch-and-start effect
- **AND** the final productive Native and PCM SHALL belong to the final original action rather than a later mutable control projection or reused consumed endpoint

#### Scenario: Cancellation races with the complete native effect claim
- **WHEN** cancellation and an original action compete for its complete native effect
- **THEN** cancellation before the final claim SHALL leave pitch and transport unchanged
- **AND** a completed claim SHALL preserve its complete effect and own ACK without a partial pitch, start or stop

#### Scenario: Stop cancels a prepared attack without cancelling a nonstarting SET
- **GIVEN** an original SET_RETRIGGER and an independent SET waiting for their captured target
- **WHEN** an explicit native Stop or launch cancellation changes the living content's existing cancellation revision
- **THEN** the original attack SHALL reject without changing pitch or restarting playback
- **AND** the nonstarting SET SHALL retain its transport-preserving semantics; later accepted attacks and Move/Swap alone SHALL NOT revoke earlier attacks

### Requirement: Expanded pitch catch-up preserves its original history and worker isolation
The system SHALL prepare the actual required Native history at the declared output coordinate without fabricating a constant past from a wrapped endpoint or a newly settled rate. An unavailable source trajectory or finite context SHALL reject only its own preparation and SHALL NOT disable another voice's preparation worker.

#### Scenario: A settled extreme pitch requires more history than the ordinary catch-up interval
- **GIVEN** a constant-rate source with valid original source, lifetime, window and timing authority
- **WHEN** its requested pitch requires an expanded Native history
- **THEN** preparation SHALL retain its original output target and reconstruct only the proved source trajectory, including any traversed Intro or Tail
- **AND** insufficient history after rate or domain changes SHALL remain an explicit local failure without a ready candidate or a shared-worker failure
