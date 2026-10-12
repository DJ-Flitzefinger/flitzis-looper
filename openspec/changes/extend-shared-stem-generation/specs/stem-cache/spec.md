## ADDED Requirements

### Requirement: E11-05 Offline Generation Is Independent Of Pad Playback
The system SHALL permit source-leased offline stem generation for a playing or
paused loaded pad after the new-path safety gates pass, while keeping disk
generation separate from that pad's fresh native adoption transaction.

Immutable source/job leases SHALL survive actual inference and complete
verification. Loading, unloading, conflicting analysis or generation intent and
stale source ownership SHALL remain guarded. Model loading/inference, file I/O,
JSON, hashing, Python/GIL/UI access, blocking locks, logging, unbounded work and
heavy allocation SHALL NOT occur in the realtime callback. The callback SHALL
continue using its existing available FullMix/stem source until own adoption.

An offline source lease SHALL retain the actual sealed managed-original readers
of its own loaded pad, including fully resident sources. Capture SHALL validate
current loaded source/request ownership and the exact own original reference
using bounded metadata operations; equal digests or shared PCM SHALL NOT confer
another root's file authority. Full source verification SHALL run on the worker.
The captured lease SHALL remain readable after its origin unloads or is replaced,
until the actual backend and every owned descendant/output reader have ended.
It SHALL NOT be a live timing/window/publication permit. Each current subscriber
SHALL obtain its own fresh publication permit only after completed work has been
revalidated against its captured source, content and monotone material intent.

#### Scenario: Resident source remains sealed after origin unload
- **GIVEN** an offline job captured from a fully resident loaded managed original
- **WHEN** the origin unloads while an independently current equal-material subscriber remains
- **THEN** the original remains immutable and readable through actual backend read end
- **AND** the survivor obtains fresh own publication authority without using the origin's old timing permit

#### Scenario: Equal PCM cannot authorize a foreign original
- **GIVEN** two loaded pads sharing equal PCM with distinct managed original references
- **WHEN** capture names the other pad's reference or a stale source generation
- **THEN** capture is rejected without opening or publishing the foreign original

#### Scenario: Timing changes while offline inference reads
- **GIVEN** a current source-leased job whose loaded content and material intent remain current
- **WHEN** timing or the playback window changes before inference finishes
- **THEN** offline source custody remains valid
- **AND** any later native preparation captures the subscriber's current timing/window authority separately

#### Scenario: Playing pad generates without disturbing its voice
- **GIVEN** a current finite NormalLoop voice using FullMix or selected stems and a nonneutral KEYLOCK/rate context
- **WHEN** an admitted immutable-source job runs offline
- **THEN** current source position, loop geometry, rate, masks and actual Native/FIFO/filter chronology remain continuous
- **AND** unrelated shared-material users remain usable without callback inference or disk access

#### Scenario: Source changes or origin unloads during inference
- **GIVEN** a shared source job with independently current subscribers
- **WHEN** one subscriber unloads, replaces content or cancels its interest
- **THEN** its completion cannot mutate newer intent or consume a newer job
- **AND** surviving subscribers retain the job and its actual read leases

### Requirement: E11-05 New Stem Sets Adopt With Independent Native Acknowledgement
The system SHALL adopt a newly generated complete set during playback only
through a separately proved bounded transaction with fresh own source/request,
timing, window, actual voice/history permits and native acknowledgement.

Disk completeness and P5a selected-same-set residency SHALL NOT authorize new
generation adoption. Prospective fullmix/component coverage, actual Native/FIFO/
filter continuation and immutable reader ownership SHALL be verified before
effective change. Own ACK SHALL precede effective UI/mode publication. Pending,
stale, queue-full, unsupported-domain and failed adoption SHALL preserve previous
effective audio with visible status; no stop/retrigger or position jump SHALL be
used to claim successful live adoption. Old voice A SHALL retain its own frozen
source/set/timing/history even after current bank B changes.

New-set preparation SHALL retain separate candidate and actual predecessor
authority. Registering or preparing candidate V2 SHALL NOT revoke V1's accepted
selection, source, resident-intent or Native/history permits. Capture SHALL pin
the actual callback selection and immutable readers; control-side disk selection
alone SHALL NOT identify the audible predecessor. Actual adoption SHALL recheck
both candidate and predecessor, own source/timing/window, launch/voice/history
and reserved old-plus-new PCM/queue/retirement capacity before changing audio.

V1 and V2 of the same original SHALL remain distinct complete-set owners even
when their source hash, geometry and enabled mask match. The existing transition
SHALL consume exactly 128 source frames through bounded current and outgoing
readers, using the same fractional taps and actual rate trajectory. Every copied
reader identity SHALL retain its corresponding immutable owner. Frozen voices
and Native/FIFO/history snapshots SHALL retain the readers their captured plan
actually uses; old history SHALL NOT be relabelled as the candidate. Realtime
completion or lifecycle changes SHALL transfer owners through reserved off-thread
retirement, retaining them for retry when retirement capacity is unavailable.

Own component/residency acceptance, claim/transition and actual wet effectiveness
SHALL remain separately observable. Paused adoption SHALL NOT report effective
output before resumed source consumption; stopped readiness SHALL NOT claim
audible output. Cancelling an unclaimed candidate SHALL leave V1 usable. A
claimed candidate SHALL remain observable through settlement when a later disk
candidate or intent arrives. A later candidate SHALL NOT replace an occupied
outgoing-reader slot or borrow an earlier candidate's ACK. Newer FullMix intent
SHALL withdraw future ALL authority so a late adoption cannot override it.
An ordered pair-release or Delete Stems command received during a claimed
handover SHALL retain bounded FullMix intent through the occupied ramp and
apply it after the ramp can safely change selection; callback rejection of the
immediate change SHALL NOT silently discard the command.

#### Scenario: New set becomes effective on a running voice
- **GIVEN** a complete current verified replacement and matching finite NormalLoop permits
- **WHEN** that pad's own guarded new-set transaction is accepted
- **THEN** the existing bounded source-selection transition preserves source fraction, rate/ramp, loop and masks with actual nontrivial wet output
- **AND** later real native preparations/adoptions preserve continuous history and old readers retire only after their final use

#### Scenario: Latest public mask waits for a claimed nonempty new-set handover

- **GIVEN** a first or replacement finite NormalLoop V2 transaction has claimed its own nonempty ALL mask with KEY LOCK OFF or ON
- **WHEN** public component-mask gestures include a subset and an empty intermediate intent and end with a different nonempty mask while the original source-frame handover remains occupied
- **THEN** the latest model intent SHALL be retained without withdrawing the claimed receipt, replacing either immutable reader, changing its captured mask or admitting another Capture before that transaction's own actual effect
- **AND** productive partial handover output SHALL retain its original two-reader PCM, fractional source trajectory, voice, Native/FIFO and uninterrupted filter history; an intermediate empty intent SHALL NOT retroactively silence the claimed nonempty handover
- **AND** observation of the original own dry or later prepared wet effect SHALL permit the latest authorized mask to obtain a fresh own Capture/worker/Adopt receipt through the same V2 registration
- **AND** that mask change SHALL consume its own 128-source-frame transition and prove its exact independent component output and own effectiveness; V2 identity or the earlier receipt's ACK alone SHALL NOT establish the latest mask

#### Scenario: Another subscriber acknowledges first
- **GIVEN** equal material assigned to two pads with independent permits
- **WHEN** only the first pad acknowledges a prepared replacement
- **THEN** only its effective state can report the new set
- **AND** the second keeps its old effective audio and pending/error status until its own ACK

#### Scenario: Stale or unsupported new-set adoption
- **WHEN** source, request, timing, window, voice/history or capacity no longer matches, or the requested domain lacks a genuine coverage proof
- **THEN** native adoption rejects safely and the current FullMix/stem/KEYLOCK trajectory remains usable
- **AND** neither a complete disk descriptor nor a same-set residency permission bypasses the guard

#### Scenario: Acknowledged Intro or Tail seek rejects a fresh new-set candidate

- **GIVEN** productive KEY LOCK ON FullMix or selected V1 and a real acknowledged finite resident Seek into Intro or Tail
- **WHEN** a distinct V2 obtains a fresh own source ticket, accepted current registration and actual candidate Capture before further source consumption
- **THEN** the unsupported explicit seek mode SHALL reject V2 before claim without Adopt, while its own registration remains accepted and current
- **AND** the actual KeyLockFiniteLoop union SHALL retain the old source and selected readers, rather than relying on a FullTrack rejection
- **AND** independent complete PCM, fractional one-shot source addressing, actual Native/FIFO and uninterrupted post-seek filter history SHALL remain usable through irregular callbacks and a later own complete Native preparation
- **AND** the authorized Seek SHALL establish its own new voice and filter chronology; candidate rejection SHALL NOT introduce another reset, retry or audible V2 acknowledgement

#### Scenario: Current bank B cannot authorize retained old-source voice A

- **GIVEN** actual Native admission has installed a distinct current bank source B while retained voice A continues using its own frozen source and FullMix or V1 readers with KEY LOCK OFF or ON
- **AND** B has its own acknowledged finite NormalLoop window, fresh PreparedSourceTicket and accepted current pending stem registration
- **WHEN** B's own residency request reaches actual callback Capture while only retained voice A is active for that pad
- **THEN** Capture SHALL reject that residency before claim and SHALL NOT enqueue Adopt or report B effective; B's registration SHALL remain accepted and current while its own residency reports rejection
- **AND** A's obsolete preparation ticket SHALL NOT authorize B, and B's current bank, window or registration SHALL NOT authorize adoption on A
- **AND** A SHALL continue through its own independently verified complete PCM, fractional source trajectory, Native/FIFO and uninterrupted filter history, while another active pad retains its independent owners and trajectory
- **AND** this rejection SHALL NOT enable new-set adoption on frozen old-source voices or extend supported playback domains

#### Scenario: Same-original replacement keeps two exact readers
- **GIVEN** active V1 and distinct verified V2 have the same source hash, geometry and mask
- **WHEN** V2 obtains its own current adoption ACK
- **THEN** rendering SHALL resolve V1 and V2 by their actual complete-set owners and mix both through 128 consumed source frames
- **AND** fractional rates, smoothing and irregular callbacks SHALL preserve the same interpolation, Native/FIFO/filter and source trajectory

#### Scenario: Accepted musical timing remains authoritative through new-set adoption
- **GIVEN** current callback-acknowledged constant timing whose musical loop period differs from the physical loop extent, and a verified new complete set
- **WHEN** that pad's own candidate adopts under BPM-controlled playback
- **THEN** the exact accepted timing, musical source phase and current KEY LOCK mode SHALL remain authoritative through both actual readers and the source-frame handover
- **AND** productive continuation SHALL preserve virtual-seam addressing, actual Native/FIFO/filter history and later source-bound preparation without a restart or position jump

#### Scenario: Public BPM target precedes productive replacement adoption
- **GIVEN** real finite NormalLoop PCM and callback-acknowledged Automatic timing whose musical loop period differs from its physical extent
- **WHEN** the public transport controller admits BPM Lock and a displayed BPM target before voice birth, the callback applies its actual control and coupled speed/master-period parameter messages, and productive V1 is later replaced through V2's own candidate transaction
- **THEN** display and saved intent SHALL remain distinct from callback acknowledgement, and the accepted source timing SHALL retain its exact authority through Capture, worker preparation and Adopt
- **AND** independent complete-PCM phase, virtual-tail interpolation, raw Native/FIFO and uninterrupted filter output SHALL verify the consumed-source-frame handover and V2's later own prepared wet effectiveness without restart

#### Scenario: Fresh new-set adoption crosses a public live BPM smoothing step

- **GIVEN** productive finite NormalLoop FullMix or independently confirmed selected V1, KEY LOCK OFF or ON, and callback-acknowledged Automatic timing
- **WHEN** the public BPM controller applies a new coupled speed/master-period intent before distinct V2 preparation and V2 obtains its own fresh registration, Capture, worker preparation and Adopt while the rate is still smoothing
- **THEN** the unchanged accepted timing, current source and own V2 authority SHALL remain valid without retry, restart or position jump
- **AND** the actual 128-consumed-source-frame handover SHALL cross an active-output-frame smoothing boundary using the continuous fractional trajectory and both complete readers
- **AND** independent complete PCM, rate epochs and uninterrupted filter history SHALL verify all irregular output partitions; another active pad SHALL retain its own physical loop trajectory under the global intent
- **AND** KEY LOCK OFF SHALL confirm V2's own dry output; KEY LOCK ON SHALL preserve actual raw Native/FIFO chronology and require V2's own later complete 4096-frame preparation and ready Wet output
- **AND** current registration, accepted residency and audible effectiveness SHALL remain separate, and earlier FullMix or V1 acknowledgements SHALL NOT confirm V2

#### Scenario: Public live BPM invalidates an already queued replacement Adopt

- **GIVEN** productive finite NormalLoop V1 with KEY LOCK ON, accepted Automatic timing and V2's own already captured and queued Adopt
- **WHEN** actual retirement pressure retains that Control message, the public transport controller's coupled BPM parameter command applies at the continuously advancing callback time, and a later callback dispatches the unchanged Adopt
- **THEN** the stale unclaimed V2 transaction SHALL reject while V1's readers, voice, fractional source phase, Native/FIFO and uninterrupted filter history remain usable under the new rate trajectory
- **AND** display admission SHALL remain distinct from actual timing authority; the musical period and source timing acknowledgement SHALL remain current, and the other active pad SHALL retain its own physical loop trajectory under the global rate change
- **AND** receipt polling SHALL report the failed own attempt without an implicit retry or a native-preparation-failed acknowledgement
- **AND** an explicit identical ALL gesture SHALL obtain a fresh own Capture/worker/Adopt receipt for the same accepted V2 registration and prove its source-frame handover and later own prepared wet effectiveness

#### Scenario: Public live BPM rejection and retry with KEY LOCK OFF

- **GIVEN** productive finite NormalLoop V1 with KEY LOCK OFF, accepted Automatic timing and V2's own already captured and queued Adopt
- **WHEN** actual retirement pressure retains that Control message, the public transport controller's coupled BPM parameter applies at continuously advancing callback time, and a later callback dispatches the unchanged Adopt
- **THEN** the stale unclaimed V2 attempt SHALL reject with its own error while V1's current readers, voice, fractional musical phase, Native/FIFO owners and uninterrupted dry filter output remain usable under the new rate trajectory
- **AND** independent complete PCM SHALL verify actual Varispeed interpolation across active-output-frame smoothing and irregular callbacks; the other active pad SHALL retain its independent physical trajectory under the global speed change
- **AND** repeated receipt polling SHALL retain the same accepted V2 registration and failed own attempt without implicitly retrying or reporting native-preparation-failed
- **AND** an explicit identical ALL gesture SHALL obtain a fresh own Capture/worker/Adopt receipt, consume the actual 128-source-frame handover and acknowledge its own dry effective output without claiming Native wet processing or preparation

#### Scenario: Public live BPM changes while productive V1 is already paused

- **GIVEN** productive finite NormalLoop V1 with accepted Automatic timing and KEY LOCK OFF or ON is publicly paused before V2's own Capture and queued Adopt
- **WHEN** retirement pressure retains that unchanged Adopt, the public coupled BPM parameter applies, and actual paused callbacks advance transport time before the old Adopt is dispatched
- **THEN** the stale unclaimed V2 attempt SHALL reject under its own receipt while the same V1 readers, source fraction, voice, Native/FIFO, filter history and handover remain frozen; paused callbacks SHALL consume no source or report new effective output
- **AND** timing acknowledgement SHALL remain authoritative independently of display intent, and the new applied source-rate glide SHALL begin only on resumed source consumption rather than silent output time
- **AND** public Resume SHALL retain the same V1 voice and independent other-pad physical trajectory; complete-PCM interpolation and uninterrupted dry or actual Native/FIFO/filter output SHALL verify all smoothing steps across irregular callbacks
- **AND** polling SHALL preserve the failed own attempt without an implicit retry or native-preparation-failed acknowledgement; an explicit identical ALL gesture after Resume SHALL obtain a fresh own Capture/worker/Adopt receipt and prove the consumed-source handover and its own dry or later prepared wet effectiveness before its observed effect clears the error

#### Scenario: Rate smoothing advances through an own new-set handover
- **GIVEN** current finite NormalLoop playback with a pending rate target and a verified first or replacement complete set
- **WHEN** its own Capture, worker preparation and Adopt complete while the same target's active-output-frame smoothing continues
- **THEN** the current voice and exact source trajectory SHALL retain the applied rate, pending target and smoothing interval through adoption
- **AND** the two-reader handover SHALL advance by actual consumed source distance across a smoothing step, without substituting a constant-rate output-frame count
- **AND** KEY LOCK OFF/ON rendering SHALL preserve actual varispeed or Native/FIFO/filter continuation and any later own preparation without a restart

#### Scenario: KEY LOCK OFF new-set continuation uses actual varispeed output
- **GIVEN** current finite NormalLoop playback with KEY LOCK OFF and a verified first or replacement complete set
- **WHEN** the own candidate adopts
- **THEN** the actual selected varispeed feed and uninterrupted filter output SHALL preserve the existing source trajectory through the source-frame handover
- **AND** component acceptance SHALL NOT be presented as proof of KEY LOCK wet processing or later Native preparation

#### Scenario: Candidate preparation fails or is cancelled
- **GIVEN** V1 remains effective while V2 is registered or preparing
- **WHEN** V2 fails admission, preparation or a currentness check, or is cancelled before claim
- **THEN** V1's own control and audio authority SHALL remain usable and V2 SHALL report its own failure or withdrawal
- **AND** retry SHALL require fresh explicit intent rather than an unbounded polling retry loop

#### Scenario: Public FullMix withdraws a prepared candidate before callback claim
- **GIVEN** productive V1 playback and a distinct V2 whose own Capture and worker preparation have queued Adopt without callback claim
- **WHEN** the pad receives a newer public FullMix intent
- **THEN** V2's future ALL authority SHALL be withdrawn and its actually queued late Adopt SHALL reject without replacing V1's readers, source trajectory, voice, Native, FIFO or filter state
- **AND** FullMix SHALL obtain its own residency and effective-output authority; V2's withdrawn operation SHALL NOT report V2 effective or override that intent
- **AND** polling SHALL NOT retry V2 implicitly; later explicit ALL SHALL obtain fresh V2 registration and its own Capture, worker preparation, Adopt and subsequent effectiveness

#### Scenario: Public mask supersedes a prepared candidate before callback claim
- **GIVEN** productive FullMix or selected V1 finite NormalLoop playback with KEY LOCK OFF or ON and a durably committed V2 whose own nonempty ALL Capture and worker preparation have queued Adopt without claim
- **WHEN** actual callback retirement pressure retains that Adopt and a newer public component-mask gesture requests a different nonempty ALL mask
- **THEN** the old unclaimed receipt and candidate registration SHALL lose future authority while disk V2 and the previous actual control/effect remain usable; intentional supersession SHALL NOT report Native preparation failure
- **AND** the unchanged old Adopt SHALL remain in normal FIFO order and reject when capacity returns without replacing actual readers, demand, source trajectory, voice, Native, FIFO, transition or filter history
- **AND** the latest authorized mask SHALL obtain fresh V2 registration and its own Capture, worker preparation and Adopt; polling SHALL NOT duplicate that admission or lend the old receipt's ACK
- **AND** only the new own Adopt SHALL start its 128-source-frame handover, with exact independent component output and own dry effectiveness for OFF or later complete prepared Native wet effectiveness for ON

#### Scenario: Public candidate admission encounters a full control queue
- **GIVEN** productive V1 playback and a distinct durably committed V2 with current accepted registration but no residency receipt
- **WHEN** an explicit public ALL request encounters a full Native control queue before Capture admission
- **THEN** the request SHALL report its own bounded admission failure without creating a V2 residency receipt or changing V1's own effective authority, readers, source trajectory, voice, Native, FIFO or filter history
- **AND** controller polling SHALL retain the desired V2 intent and visible failure without implicitly retrying admission, including after the queue drains
- **AND** a later explicit identical ALL request SHALL be able to reuse the current V2 registration and obtain its own new Capture, worker preparation, Adopt and subsequent effectiveness; V1's ACK SHALL NOT substitute for that operation

#### Scenario: Public candidate worker cannot post Adopt after successful Capture
- **GIVEN** productive V1 playback and a distinct durably committed V2 whose public ALL request obtained its own receipt and actual callback Capture
- **WHEN** the Native control queue remains full through the worker's bounded Adopt-post attempts
- **THEN** that V2 receipt SHALL report its own preparation/publication failure without callback claim, V2 reader installation or effective success, and its cold-job reservation SHALL release
- **AND** disk V2 SHALL remain committed while V1's own current receipt, control/effective identity, exact readers, source trajectory, voice, Native, FIFO and filter history remain usable
- **AND** polling SHALL preserve the failed V2 attempt and visible error without retrying after the queue drains; the failed uncurrent receipt SHALL NOT report current effective output
- **AND** a later explicit identical ALL request SHALL obtain a fresh own receipt through the still-current V2 registration and require its own Capture, worker preparation, Adopt and later effectiveness; the failed receipt SHALL NOT lend its ACK to the retry

#### Scenario: Candidate Capture remains queued beyond its worker deadline
- **GIVEN** productive V1 playback and a durably committed registered V2 whose public ALL request admitted an own waiting receipt but whose actual Capture remains queued behind insufficient callback retirement capacity
- **WHEN** the worker's real bounded Capture deadline rejects that operation before later Native reconciliation
- **THEN** V1 SHALL continue with its own effective authority, readers and processing while the failed V2 receipt reports its own deadline error and releases its job reservation
- **AND** later runtime reconciliation and actual dispatch of that unchanged late Capture SHALL NOT replace V1's readers, trajectory, Native, FIFO or filter history, revive the failed receipt or enqueue V2 Adopt
- **AND** controller polling SHALL retain the failed attempt and error without an implicit retry; a later explicit identical ALL request SHALL use fresh own residency/effectiveness through V2's still-current registration

#### Scenario: Paused and stopped acceptance is not audible effectiveness
- **WHEN** a current NormalLoop candidate is installed on a paused voice or stopped pad
- **THEN** own residency acceptance SHALL remain distinct from effective output
- **AND** paused transition SHALL advance only on resumed source consumption and stopped readiness SHALL report no audible success

#### Scenario: Effective selected V1 is replaced while publicly paused

- **GIVEN** a proper finite NormalLoop voice has confirmed its own selected V1 effect with KEY LOCK OFF or ON at a nonneutral rate
- **WHEN** public Pause freezes that voice before distinct V2 preparation, durable commit, current registration and own Capture/worker/Adopt
- **THEN** V2 SHALL remain current, claimed and accepted for residency but transitioning without audible effectiveness or consumed source-frame handover
- **AND** timestamped paused callbacks and controller polling SHALL consume no source, Native FIFO, filter history or handover progress while retaining current V2 and outgoing V1 readers, voice generation and fractional phase
- **AND** V1's earlier effectiveness SHALL NOT confirm V2, and paused observation SHALL NOT admit a duplicate operation
- **AND** public Resume SHALL retain the same voice, phase and filter chronology and complete the handover through 128 actually consumed source frames
- **AND** KEY LOCK OFF SHALL confirm V2 through its own actual varispeed and continuous-filter output; KEY LOCK ON SHALL require its own subsequent complete 4096-frame Native preparation and ready current-selection Wet output

#### Scenario: Ordinary Play supersedes a stopped candidate's accepted observation
- **GIVEN** a new complete set was installed with KEY LOCK OFF or ON and its own stopped residency acceptance but no audible effectiveness
- **WHEN** an ordinary authorized Play changes launch authority while retaining those installed readers, before or after controller observation of that acceptance
- **THEN** the controller SHALL withdraw the obsolete accepted observation without reporting a candidate adoption failure or borrowing its effectiveness for the new voice
- **AND** polling SHALL NOT submit an implicit replacement operation; a later explicit ALL gesture SHALL request fresh own residency and effective-output authority even for the same mode and mask
- **AND** genuine Native preparation failure or unaccepted candidate rejection SHALL remain observable and require explicit retry
- **AND** with KEY LOCK OFF, actual nonneutral varispeed output and continuous filter processing SHALL confirm the fresh operation's own dry effectiveness; stopped readiness SHALL NOT imply Native wet processing or preparation

#### Scenario: Ordinary Play supersedes a queued unclaimed new-set adoption
- **GIVEN** productive finite NormalLoop FullMix or selected V1 playback with KEY LOCK OFF or ON and durably committed V2 whose own Capture and worker preparation have queued Adopt without claim
- **WHEN** public ordinary Play requests the same finite window before that Adopt can pass the actual callback retirement gate
- **THEN** the new resident intent SHALL invalidate the old candidate's authority; its unchanged FIFO Adopt SHALL reject before claim, separately from the new window's own ACK and actual prepared Play
- **AND** admission, late rejection and the same-window ACK SHALL retain the prior productive source trajectory, voice, immutable readers, Native/FIFO and filter continuation until the actual authorized Trigger
- **AND** that Trigger SHALL start its own new voice/source and filter chronology without borrowing either old voice's or V2's effectiveness, while other pads continue independently
- **AND** the rejected unaccepted receipt SHALL remain observable without Native preparation failure or implicit polling retry; a later explicit ALL SHALL require fresh V2 registration, own Capture/worker/Adopt and its own 128-source-frame handover and subsequent dry or prepared wet effect on the new voice

#### Scenario: Public finite loop expansion supersedes queued new-set adoption

- **GIVEN** productive finite NormalLoop FullMix or selected V1 with KEY LOCK OFF or ON and a committed V2 whose actual Adopt remains queued and unclaimed
- **WHEN** public loop-end expansion changes the resident window while the current fractional taps remain inside the old loop
- **THEN** the old candidate's authority SHALL become obsolete before claim; normal FIFO rejection SHALL remain distinct from the changed window's own preparation and callback ACK
- **AND** admission and rejection SHALL preserve the old productive readers, voice, source trajectory, Native/FIFO and filter history; the acknowledged expansion SHALL retain current phase and continue through independently proved new loop geometry without a retrigger
- **AND** polling SHALL preserve the failed unaccepted receipt and durable V2 selection without automatically retrying or treating a window ACK as a V2 effect
- **AND** later explicit ALL SHALL prepare that same verified V2 through the existing worker lane using a fresh current-window source ticket and component views, then require fresh own registration, Capture/worker/Adopt,128-source-frame handover and actual dry or prepared wet effect before clearing its own error
- **AND** this explicit refresh SHALL require Native-confirmed same source/request and changed sealed preparation context; unrelated admission failures or another source SHALL NOT grant that refresh, and preparation-context equality SHALL NOT constitute an audio ACK

#### Scenario: Public source assignment replaces audio after its own acknowledgement

- **GIVEN** productive finite NormalLoop FullMix or selected V1 with KEY LOCK OFF or ON and an old-source V2 whose actual Adopt remains queued and unclaimed
- **WHEN** public source loading admits a different original as an assignment replacement
- **THEN** the new source request SHALL invalidate the old candidate before claim while pending assignment and preparation preserve the old productive PCM, voice, Native/FIFO and filter chronology
- **AND** normal FIFO rejection of the old V2 SHALL remain separate from the new source's own callback acknowledgement; the old V2 SHALL never claim or borrow that acknowledgement
- **AND** only the acknowledged assignment replacement SHALL deliberately stop that pad's old voice and install the new original without an automatic voice start, while other pads retain their own source, voice and trajectory
- **AND** the matching actual loader completion SHALL publish the new source identity, assignment and defaults, invalidate the old stem observation and persist the new assignment without treating this lifecycle stop as a successful live stem adoption
- **AND** repeated polling SHALL NOT retry the old-source candidate or infer a new source's audible effect; later playback and new-source stem adoption SHALL require their own explicit authority and acknowledgement

#### Scenario: Public Play starts the acknowledged replacement original with its own voice

- **GIVEN** a different source assignment has obtained its own load acknowledgement, the old-source V2 is rejected and unclaimed, and the new pad inherits KEY LOCK OFF with neutral mixing defaults
- **WHEN** explicit public Play requests a finite NormalLoop of the new original while another pad is active
- **THEN** Play SHALL prepare the new source's own current resident window and wait for its actual acknowledgement before admitting a source-bound launch
- **AND** neither preparation admission nor window acknowledgement SHALL start a voice; only the matching explicit launch SHALL start the new voice from the requested loop origin
- **AND** actual output SHALL follow the new original's complete PCM, declared playback rate and fresh neutral filter chronology without old component readers, old-source stem effectiveness or predecessor wet history
- **AND** the old rejected V2 SHALL remain rejected and unclaimed through repeated polling, and the other pad SHALL retain its independent source, voice and playback trajectory

#### Scenario: Public KEY LOCK ON belongs to the playing replacement original

- **GIVEN** a different original has obtained its own load, finite NormalLoop window and public Play acknowledgements and is producing FullMix with inherited KEY LOCK OFF
- **WHEN** public per-pad KEY LOCK ON requests that current playing original while another pad remains active
- **THEN** the existing resident preparation path SHALL acknowledge its own current-source window and KEY LOCK request without restarting the voice, source phase or filter history
- **AND** window acknowledgement SHALL NOT alone confirm effective ON; confirmation SHALL require current-source feedback with that operation's own window revision and request ID and actual ready, effective Wet output
- **AND** actual output SHALL follow independently verified complete new-original PCM through the dry-to-Wet transition and its own Native preparation while the rejected predecessor candidate remains unclaimed and the other pad retains its independent owners and trajectory
- **AND** persistence SHALL retain the explicit per-pad ON override, and any subsequent stem preparation SHALL capture fresh current preparation context rather than borrow the earlier OFF-window ticket or KEY LOCK acknowledgement

#### Scenario: New original requires its own live stem preparation and dry effect

- **GIVEN** a different original has obtained its own load, finite NormalLoop window and explicit Play acknowledgements, is producing FullMix with KEY LOCK OFF, and the predecessor's queued stem candidate remains rejected and unclaimed
- **WHEN** that new original's verified complete stem files enter the existing pair worker and durable selection path while playback continues
- **THEN** preparation SHALL capture the new original's current source, request and window context, retain its own complete-source and component artifact owners, and commit its own complete pair independently of the predecessor's selection
- **AND** durable selection and active pending registration SHALL preserve current FullMix output, voice, phase, Native/FIFO and filter chronology without admitting Capture or reporting stem effectiveness
- **AND** a later explicit public ALL gesture SHALL require that new selection's own Capture, component worker and Adopt acknowledgement, followed by128 consumed source frames of handover and actual dry output before its own receipt becomes effective
- **AND** output SHALL follow independently verified new-original component PCM and the explicitly selected mask while the rejected predecessor remains unclaimed, other pads retain their own trajectory and owners, and persistence reopens the new assignment and complete pair

#### Scenario: New original requires its own live stems after public KEY LOCK ON

- **GIVEN** a different original has obtained its own load, finite window and public Play acknowledgements, and its own public KEY LOCK ON has separately confirmed ready, effective FullMix Wet output
- **WHEN** that playing original's verified complete stem files enter the existing pair worker and durable selection path
- **THEN** preparation SHALL capture fresh current ON-window source context, retain its own artifact owners and commit its own complete pair while preserving productive FullMix, voice, phase, Native/FIFO and filter chronology without admitting Capture or reporting stem effectiveness
- **AND** a later explicit public ALL gesture SHALL require that selection's own Capture, component worker and Adopt acknowledgement; the earlier FullMix KEY LOCK acknowledgement SHALL NOT certify the stem operation
- **AND** its receipt SHALL remain transitioning after128 consumed source frames of handover until its own later Native preparation and ready Wet output confirm the current stem selection
- **AND** output SHALL follow independently verified complete new-original component PCM, selected mask, continuous input ramp, Native FIFO and filter chronology; the complete4096-frame preparation interval SHALL begin after its own stem adoption without borrowing predecessor or FullMix history
- **AND** the rejected predecessor SHALL remain unclaimed, other pads SHALL retain their independent source owners and trajectory, and persistence SHALL reopen the new assignment, complete pair, ALL intent and explicit KEY LOCK ON override

#### Scenario: Public Unload cancels a queued new-set adoption before its own stop

- **GIVEN** a playing FullMix or selected V1 voice has an unclaimed V2 Adopt queued behind its genuine Capture and component worker, and another pad is active
- **WHEN** public Loader Unload admits its own native source revocation and ordered unload command and invokes the existing stem cancellation and pad-default paths
- **THEN** the earlier queued V2 SHALL remain unclaimed and rejected even when its late callback command precedes Unload in the FIFO; cancellation SHALL NOT grant residency or effectiveness
- **AND** while retirement capacity holds the unload command, the current voice SHALL retain its source phase, Native/FIFO and filter history, with independently verified current PCM and the actually admitted default-parameter changes applied in their normal order
- **AND** only that unload command's own callback acknowledgement SHALL stop the pad and remove its bank/readers; subsequent ordered defaults, polling or late stem work SHALL NOT restore the removed assignment or ALL intent
- **AND** another active pad SHALL retain its independent source owners, generation and absolute trajectory, while persistence SHALL reopen the unloaded assignment, cleared stem selection and inherited KEY LOCK intent without borrowing a stem receipt as unload acknowledgement

#### Scenario: First candidate is prepared after a productive FullMix voice was paused
- **GIVEN** a current proper NormalLoop FullMix voice has produced genuine wet output and is paused before new-set registration and own Capture
- **WHEN** its new complete-set candidate obtains own residency acceptance while the same voice remains paused
- **THEN** timestamped paused callbacks SHALL consume no source, handover, Native FIFO or filter history and SHALL NOT report effective output
- **AND** Resume SHALL retain that voice generation and exact source phase, Native, FIFO and filter state, complete the handover through actual resumed source consumption and confirm new-set wet output only through its own later preparation and effective receipt

#### Scenario: First KEY LOCK OFF candidate is prepared after public Pause
- **GIVEN** a productive proper NormalLoop FullMix voice with KEY LOCK OFF and nonneutral varispeed is publicly paused before new-set preparation, durable commit, registration and own Capture
- **WHEN** the new complete-set candidate obtains its own residency acceptance while that same voice remains paused
- **THEN** advancing callback timestamps SHALL consume no source distance, handover or filter history, preserve the exact voice and source phase and report no effective new-set output
- **AND** public Resume SHALL retain that voice and phase, then complete the handover through actual resumed source consumption and confirm the candidate only through its own effective varispeed/filter output
- **AND** KEY LOCK OFF acceptance SHALL NOT borrow Native wet preparation or the KEY LOCK ON receipt as evidence

#### Scenario: Later candidate arrives during an owned handover
- **GIVEN** V2 has been claimed and V1 remains the outgoing reader
- **WHEN** disk candidate V3 or a new mode or mask intent arrives
- **THEN** the bounded controller SHALL retain truthful observation of V2 and defer or safely reject interruption of the occupied handover
- **AND** V2's delayed ACK SHALL NOT label V3 effective or overwrite V1's reader before final use

#### Scenario: Public mask intent and a later set survive an empty owned handover
- **GIVEN** productive finite NormalLoop V1 playback and a distinct V2 whose own accepted and claimed handover targets ALL with no enabled components while V1 remains the outgoing reader
- **WHEN** public mask changes coalesce to a latest ALL intent and a distinct verified V3 becomes the durable current disk selection
- **THEN** the controller SHALL retain V2's own receipt and the latest desired mask and disk target separately, without interrupting the claimed handover or reporting V3 effective
- **AND** rendering SHALL retain V1 through its final use and complete the handover through 128 consumed source frames; the resulting zero source feed SHALL preserve existing Native/FIFO/filter chronology rather than force instantaneous output silence
- **AND** V3 registration and admission SHALL remain deferred while V2's own effective transition is unsettled, including its required source-specific Native preparation with KEY LOCK ON
- **AND** after settlement V3 SHALL obtain fresh own Capture, worker preparation and Adopt authority for the latest requested mask, even when the ALL mode is unchanged
- **AND** only V3's own subsequent effective output SHALL confirm V3; neither its disk commit nor V2's delayed ACK SHALL supply that confirmation

#### Scenario: Retirement pressure retains the outgoing reader
- **WHEN** a ramp, stop, unload or source replacement releases its use of V1 while retirement capacity is full
- **THEN** the actual V1 owner SHALL remain held for bounded off-thread retry without callback destruction of its final backing
- **AND** frozen source/history plans SHALL retain both readers for as long as their real captured plan needs them

#### Scenario: Ordered deletion arrives during claimed handover
- **GIVEN** V2 is claimed and V1 still supplies an unfinished outgoing ramp
- **WHEN** the pad receives an ordered pair release or Delete Stems
- **THEN** future ALL authority SHALL be withdrawn and bounded FullMix intent SHALL survive the occupied handover
- **AND** resumed source consumption SHALL finish the owned ramp before the safe FullMix transition, with exact readers retained through their actual use and off-thread retirement

### Requirement: E11-19 Selected Stem Model Has One Current Verified Set
The system SHALL maintain one current verified stem set per shared material and
atomically replace its complete five-WAV/five-PCM/common-descriptor selection
for a changed effective model/configuration without physically overwriting any
leased generation.

The system SHALL use one versioned `effective-stem-v3` identity for verified
full original SHA-256/byte length, full output rate/channel-layout/frame extent,
transform/alignment revision, separator/model identity, only that separator's
output-affecting quality options and the actual full sealed producer lineage.
Producer lineage SHALL bind the complete verified checkpoint/configuration and
implementation/runtime inputs that determine the produced output; requested
model labels or paths alone SHALL NOT attest the actual producer. Paths SHALL
remain locators and SHALL NOT enter this effective identity. Diagnostic execution
metadata SHALL remain separate unless it changes the defined output.

Provenance SHALL be an optional versioned field in the common descriptor, PCM
binding and current selection. Every new attested generation SHALL persist that
record in all three and SHALL verify their agreement before eligibility. Equal
WAV hashes from different known producer lineages SHALL NOT establish effective-set
compatibility. Absent provenance in
legacy records SHALL preserve their existing artifact identity and model-free
verified usability as UNKNOWN; restoration SHALL NOT load a model, synthesize
selected-model attestation or silently promote UNKNOWN to a known producer.

Verification and both artifact areas SHALL complete before selection commit.
The project config's successful atomic replacement SHALL be the disk-selection
linearization point. The same existing fenced writer SHALL verify current
subscriber/source/model intent immediately before replacement, with independent
predecessor and complete new assignment owners and bounded settlement capacity
already held. A failure before replacement SHALL leave old disk references and
live selection intact. A failure settling owners after replacement SHALL keep
the committed complete selection and its exact owners reachable for bounded retry;
it SHALL NOT report or perform a precommit rollback. Artifact descriptor completion,
dirty marking or a later native adoption ACK SHALL NOT substitute for this commit.

Failure or Cancel before commit SHALL retain the old usable selection;
a postcommit Cancel SHALL report the committed result truthfully and detach only
pending interests. Current-model selection SHALL NOT expose a parallel model
library. Retained immutable predecessors SHALL protect independently assigned copies as well as old runtime/rollback readers until actual final retirement. Publishing one current material-model selection SHALL NOT force a non-requesting copy C off its separately selected V1 when requesting A adopts V2; C keeps V1 until explicit replacement/deletion or final release, without exposing a selectable parallel-model library.

#### Scenario: Changed model replaces the current set
- **GIVEN** one current verified set from model A
- **WHEN** the selected model B completes joint verification and atomic selection commit
- **THEN** material selection names one complete B set and each requesting current pad requires its own later effective adoption ACK
- **AND** A remains immutable and readable for actual old voice/history/job/rollback readers until they retire

#### Scenario: Replacement fails or is cancelled before commit
- **WHEN** a new generation fails WAV, PCM, descriptor or config publication, or Cancel wins before commit
- **THEN** the previous complete selected set remains usable and no mixed or partial generation becomes current
- **AND** only the failed job's contained private artifacts can retire after their actual readers end

#### Scenario: Cancellation or owner settlement failure follows config replacement
- **GIVEN** both complete artifact areas, their common descriptor and all required owners are held
- **WHEN** the config replacement succeeds before cancellation or a later owner-transfer failure
- **THEN** reopen names the complete committed new set and cancellation cannot restore the old selection as though commit had failed
- **AND** failed settlement retains exact predecessor/new owners and reserved retry capacity without granting effective audio adoption

#### Scenario: Model selection changes while work is running
- **WHEN** Settings changes the selected model during an admitted job
- **THEN** the job keeps its captured effective fingerprint
- **AND** its late completion cannot overwrite a newer current material/model intent

#### Scenario: New content or unload follows config replacement
- **GIVEN** the complete new pair has committed through actual config replacement
- **WHEN** newer content or unload supersedes that pad before owner settlement
- **THEN** the newer memory state remains current and the same writer persists it before the stale disk-selected pair retires
- **AND** a failed superseding write retains both exact new/predecessor owners and reserved retry capacity until that newer snapshot is durable

#### Scenario: Producer lineage distinguishes otherwise equal artifacts
- **GIVEN** two complete sets have equal WAV hashes but different verified producer lineage
- **WHEN** current selection or reuse checks their effective identity
- **THEN** the sets are not compatible as the same selected-model generation
- **AND** changing only a verified locator leaves the effective identity unchanged

#### Scenario: Quality identity uses only effective separator options
- **GIVEN** the actual selected separator consumes only its defined quality options
- **WHEN** an option used only by another separator changes
- **THEN** the effective fingerprint remains equal
- **AND** changing the selected separator's output-affecting quality changes the fingerprint

#### Scenario: Legacy provenance is absent on model-free reopen
- **GIVEN** a complete verified legacy WAV/PCM pair without a provenance record
- **WHEN** the project reopens without model installation or loading
- **THEN** the old artifact identity and usable stems remain intact with UNKNOWN producer provenance
- **AND** no selected-model fingerprint is inferred from filenames, WAV hashes or current Settings

### Requirement: E11-20 Selected Model Fingerprint Prevents Duplicate Generation
The system SHALL disable Generate when a current valid source-bound set matches
the selected effective model/configuration fingerprint and SHALL enforce the
same duplicate decision in UI, controller, direct and batch request paths.

Duplicate complete requests SHALL return the diagnostic `Stems already present`
and perform no inference or artifact writes. Concurrent equal in-flight requests
SHALL share one physical job with independent interests. Selecting a different
effective fingerprint SHALL enable generation, and successful complete
replacement SHALL disable it again. Changed fingerprints SHALL serialize per
verified material. UI state SHALL derive from the same control-owned authoritative
decision used by direct requests and future batch admission, without render-time
source/model hashing or model loading. Legacy unknown fingerprints SHALL NOT be
silently promoted. Model-free validated restoration SHALL remain usable without
loading/downloading a selected model.

Validated stem-generation Settings changes SHALL advance a monotone
generation-intent revision.
Each material's changed request intent SHALL advance its own monotone revision.
Each subscriber SHALL bind its captured Settings/material revisions as well as
its own source ticket, source version, content instance and request identity.
The authoritative control path SHALL revalidate these fences at worker events
and immediately before current-selection mutation. An A-to-B-to-A change SHALL
NOT restore an earlier job's authority merely because the final fingerprint is
equal. Stale results SHALL preserve the previous usable selection and SHALL NOT
consume a newer job or mutate newer subscriber intent.

#### Scenario: Complete selected model already exists
- **WHEN** UI, a direct controller call or an all-loaded batch requests generation of the current valid selected fingerprint
- **THEN** no new inference or artifact writes start and the action reports Stems already present
- **AND** the Generate button is disabled for that valid selected set

#### Scenario: Model change and successful return
- **GIVEN** valid model A stems
- **WHEN** model B is selected, then B generation successfully commits
- **THEN** Generate is enabled before B completion and disabled after current B verification
- **AND** selecting A again permits a new replacement without retaining a selectable A/B library

#### Scenario: Settings returns to A before the old A result arrives
- **GIVEN** an A job captures current Settings and material intent revisions
- **WHEN** validated Settings changes A to B to A before an old worker event or selection mutation
- **THEN** the old A result remains stale despite matching the final A fingerprint
- **AND** only current independently fenced interests can authorize selection without losing the old usable set

#### Scenario: Material intent returns to an earlier fingerprint
- **GIVEN** Settings remains unchanged while a material requests fingerprint A
- **WHEN** that material's replacement intent changes to B and then A
- **THEN** its monotone material revision rejects the earlier A completion at event handling and immediately before selection
- **AND** equal source bytes or final fingerprint do not revive the earlier content instance, ticket or request

#### Scenario: Valid metadata is checked during UI rendering
- **GIVEN** verified current provenance and a validated selected-model state snapshot
- **WHEN** UI renders Generate readiness repeatedly
- **THEN** it uses the shared authoritative decision state without hashing source/model files or loading a separator
- **AND** a direct request revalidates the same decision before inference regardless of rendered button state

### Requirement: E11-19 Stem Retirement Waits For Actual Last Readers
The system SHALL retire replaced or deleted stem files off-thread only after
their actual final all-bank assignment, job/subscriber, action/version, queued
publication, native unload ACK, bank/voice, Native/FIFO/history and retained
complete reader releases its own ownership.

Admission SHALL reserve bounded rollback/retirement capacity before changing
intent, and new owners SHALL be acquired before old owners release. Cleanup
SHALL serialize with reader admission, check resolved containment, retry sharing
failures and preserve unknown, external, newer-generation and still-owned files.
Original FullMix/source PCM SHALL remain usable after stem deletion.

#### Scenario: Replacement keeps an old native history reader
- **WHEN** all visible pads select a new set but an old voice or native history still reads the predecessor
- **THEN** predecessor WAV/PCM/descriptor files remain immutable and protected
- **AND** off-thread cleanup occurs only after that actual final reader retires

#### Scenario: Cleanup races a new generation or fails sharing
- **WHEN** cleanup encounters a new generation, unknown file, external path, reparse point or sharing denial
- **THEN** it preserves non-owned content and defers/report failures through bounded control status
- **AND** no callback deletion or immediate recursive material removal occurs

## MODIFIED Requirements

### Requirement: Stem Generation And Replacement Require An Inactive Pad
The system SHALL separate source-leased offline generation from guarded native
new-set adoption and SHALL permit offline generation regardless of pad playback
after the E11-05 safety gates pass.

This inherited heading identifies the superseded main requirement. Activity
alone SHALL NOT block immutable offline work. A generated set SHALL change
effective audio only through fresh own source/request/timing/window/voice/history
permits and a separately proved bounded native transaction with its own ACK.
Unproved active new-set domains SHALL remain guarded. Same-set residency or
relocation SHALL NOT authorize new complete content. Failure, stale completion
or Cancel SHALL preserve current FullMix/stem playback and reader ownership;
no model inference or preparation SHALL execute in the callback.

#### Scenario: Playing pad starts offline generation
- **GIVEN** a playing pad with a current immutable source and no conflicting job intent
- **WHEN** generation is requested after offline safety proof
- **THEN** generation runs through the source-leased bounded background path
- **AND** current playback continues while later native adoption remains independently guarded

#### Scenario: Pad starts before generated result returns
- **WHEN** a pad begins playing before complete verification finishes
- **THEN** the verified result can commit on disk for still-current material/model intent
- **AND** effective new stems require the pad's own proved active transaction and ACK, or remain pending with old audio usable

## ADDED Requirements

### Requirement: E11-19 Independent copied assignments retain selected predecessors
The system SHALL preserve a non-requesting copied assignment's separately selected immutable predecessor after another equal material user commits a new model, without advertising multiple selectable model libraries or deleting the predecessor before actual final use.

#### Scenario: A adopts V2 while C remains on V1
- **GIVEN** A and independent copy C share verified V1
- **WHEN** A requests model B and commits V2 while C has no replacement request
- **THEN** only A's own accepted replacement SHALL change its effective selection
- **AND** C SHALL retain usable V1 files, intent and readers through origin deletion/reopen until its explicit replacement/deletion or actual final release
