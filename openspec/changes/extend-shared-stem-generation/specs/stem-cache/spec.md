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

The system SHALL permit first and replacement new-complete-set adoption on a
physically complete resident NormalLoop `[0,N)` with KEY LOCK OFF or ON through
the same own Capture, off-thread preparation and guarded Adopt transaction.
Complete residency alone SHALL NOT authorize explicit Seek, Intro, Tail, frozen
old-source voices, multiple same-pad voices or another unproved playback domain.

The system SHALL additionally permit first and replacement new-complete-set
adoption on a single current running voice during its acknowledged Tail with
KEY LOCK ON, no accepted musical timing, and a KeyLockFiniteLoop view covering the
entire Tail-and-loop union. This candidate-only path SHALL use the same fresh
Capture, off-thread preparation, two-reader coverage and guarded Adopt authority.
The system SHALL additionally permit this own candidate transaction on a single
current running KEY LOCK OFF Tail without accepted musical timing when the
acknowledged Seek holds a physically complete FullTrack view `[0,N)`. Complete
source layout and both exact predecessor/candidate reader generations MUST
cover all current and future Tail/loop taps. Actual consumed-source handover and
dry output SHALL establish OFF effectiveness without claiming Native Wet
preparation or resetting Native/FIFO/filter history at candidate adoption.
The system SHALL also permit first and replacement candidate adoption while
that own acknowledged nonmusical Tail voice is paused, with the same ON union
or complete OFF FullTrack coverage. Paused Capture, preparation and Adopt MUST
retain the exact source position, actual voice, Native/FIFO/filter history and
both immutable reader owners without consuming source or reporting effective
output. Only that voice's own Resume and subsequent consumed-source handover
SHALL establish dry OFF effectiveness or later exact-history Native Wet ON
effectiveness; neither callback time nor disk/preparation acceptance suffices.
The system SHALL permit first and replacement candidate adoption on a single
current running BeforeLoop voice after its own acknowledged nonmusical Intro
Seek. KEY LOCK ON MUST hold the actual KeyLockFiniteLoop prefix-and-loop union
`[0,E)` or the separately proved physically complete KeyLockFullTrack exception;
OFF MUST hold a physically complete FullTrack `[0,N)`. Fresh own
Capture, off-thread preparation and guarded Adopt SHALL verify both exact
immutable reader generations through Intro and all future loop taps. Own
consumed-source handover and output SHALL establish dry OFF or later exact-history
Native Wet ON effectiveness without another source/voice/filter restart.
The same candidate transaction SHALL additionally permit paused Intro and
running or paused Intro/Tail with current callback-accepted musical timing.
Intro MUST retain its actual ON prefix-and-loop union or complete ON/OFF track;
Tail MUST retain its actual ON suffix-and-loop union or complete ON/OFF track.
Both exact immutable reader generations MUST cover every physical one-shot tap
and both accepted musical seam taps after natural return. Capture and Adopt
MUST retain and revalidate the exact own accepted timing projection and ACK epoch,
source/window, launch/voice/history and reserved capacity. A stale projection
SHALL reject without claiming another timing or another pad's permission.
Paused adoption SHALL freeze source consumption, handover, Native/FIFO and
filter history; only own resumed dry OFF or later exact-history Native Wet ON
output SHALL acknowledge effectiveness. Natural return MUST preserve the accepted
musical period, fractional source phase and actual voice without extra Seek,
launch or filter reset. Ordinary same-set mode requests, frozen old-source and
multiple same-pad voices SHALL retain their existing unsupported-domain guards.

The same own candidate transaction SHALL permit acknowledged changed-position
and equal-phase in-loop Seek followed by new-set adoption. The explicit Seek
SHALL retain its existing intentional source/voice/Native/FIFO/filter cut and own
ACK; candidate adoption after that ACK MUST introduce no additional cut. A covered
Seek's preserved-window ACK SHALL be sufficient without an invented relocation.
First and replacement candidates SHALL also be supported on a physically complete
FullTrack or KeyLockFullTrack source `[0,N)` with a proper partial NormalLoop
`[S,E)` where `0 <= S < E <= N` and `[S,E)` differs from `[0,N)`, including stopped,
running and paused states with physical or callback-accepted musical timing.
The actual resident extent SHALL remain distinct from the physical loop H and
accepted musical period P; rewriting the loop as the complete extent SHALL NOT
establish this partial-loop contract.
Candidate Intro/Tail with KEY LOCK ON SHALL additionally be permitted when the
actual acknowledged KeyLockFullTrack source physically covers all of `[0,N)`.
Both exact predecessor/candidate readers MUST cover the physical one-shot and
all later physical or accepted musical loop taps. Context tags without the actual
complete layout and both physical bounds SHALL reject before claim. All own
source/window/timing/launch/history/capacity fences, pause-frozen ownership and
consumed-source dry OFF or later exact-history Native Wet ON effectiveness MUST
remain authoritative. Complete residency SHALL NOT authorize ordinary same-set
Intro/Tail requests, frozen old-source voices or multiple same-pad voices.

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

#### Scenario: Complete NormalLoop adopts first and replacement sets

- **GIVEN** a callback-acknowledged FullTrack or KeyLockFullTrack resident source physically covering `[0,N)`, a NormalLoop over that complete extent, and fresh own source/window/timing authority
- **WHEN** a distinct verified complete set obtains that pad's own Capture, off-thread preparation and guarded Adopt from FullMix or selected V1
- **THEN** both exact reader generations SHALL physically cover the complete source and both `N-1` to `0` seam taps throughout the 128-consumed-source-frame handover
- **AND** fractional nonunity playback, source position, actual voice, Native/FIFO and uninterrupted filter history SHALL remain continuous; later own complete Native preparation SHALL establish the new set's actual wet effectiveness with KEY LOCK ON
- **AND** paused adoption SHALL consume no source or report audible effectiveness before Resume, and stopped readiness SHALL require its own later playback/effect observation
- **AND** the existing source/launch/timing/window/history/capacity fences SHALL remain authoritative; this complete NormalLoop proof alone SHALL NOT authorize explicit Seek, Intro, Tail, frozen old-source or multiple same-pad voices

#### Scenario: Paused Intro preserves post-seek audio after candidate withdrawal

- **GIVEN** productive KEY LOCK ON FullMix or selected V1, a real acknowledged finite resident Seek into Intro, and its own Pause before candidate Capture
- **WHEN** a distinct V2 obtains a fresh own source ticket and accepted current registration, then its own interest is withdrawn before actual candidate Capture
- **THEN** the withdrawn request SHALL reject before claim without Adopt, while its own registration remains accepted and current
- **AND** the actual KeyLockFiniteLoop union SHALL retain the old source and selected readers, rather than relying on a FullTrack rejection
- **AND** after its own Resume, independent complete PCM, fractional one-shot source addressing, actual Native/FIFO and uninterrupted post-seek filter history SHALL remain usable through irregular callbacks and a later own complete Native preparation
- **AND** the authorized Seek SHALL establish its own new voice and filter chronology; candidate rejection SHALL NOT introduce another reset, retry or audible V2 acknowledgement

#### Scenario: Public running Intro adopts first and replacement sets

- **GIVEN** a single current running nonmusical BeforeLoop voice after its own public Intro Seek acknowledgement, with FullMix or selected V1, KEY LOCK ON prefix-and-loop union or OFF complete FullTrack coverage
- **AND** the actual Intro is long enough for own Capture, Adopt, consumed-source handover and complete ON 4096-output-frame preparation with productive Wet both before and after V2 while still BeforeLoop
- **WHEN** a distinct complete V2 obtains fresh post-Seek source authority, durable disk selection, registration, Capture, off-thread preparation and guarded Adopt through the public StemController
- **THEN** both exact reader generations SHALL cover every current and future prefix/loop tap through the 128-consumed-source-frame ramp; complete independent physical PCM SHALL verify distinct own dry OFF or later exact-history Native Wet ON output with uninterrupted nonneutral filter history
- **AND** adoption SHALL preserve the actual voice, source fraction and Native/FIFO checkpoint; natural Intro-to-NormalLoop progression and further output SHALL introduce no extra Seek, reset or launch
- **AND** obsolete V1 or another pad's receipt SHALL NOT confirm V2; another active pad SHALL retain its absolute independent source trajectory and owners, and model-free reopen SHALL verify saved V2 and loop/KEY LOCK intent separately
- **AND** ordinary same-set requests, frozen old-source and multiple same-pad voices SHALL remain guarded; this individual scenario SHALL NOT establish consolidated task3.3 closure, full L4 or Human acceptance

#### Scenario: Unsupported same-set Intro and Tail preserve their own voice and owners

- **GIVEN** an own acknowledged Intro or Tail Seek with KEY LOCK ON or OFF, running or paused, with physical or accepted musical timing
- **WHEN** an ordinary same-set mode request obtains fresh own registration and reaches actual Capture
- **THEN** Capture SHALL reject before claim without Adopt or effectiveness, while the own registration remains current and accepted
- **AND** source position/fraction, voice generation, used Native/FIFO, pause state and actual source/component owners SHALL remain usable and unchanged

#### Scenario: Public paused Intro adopts only through own resumed output

- **GIVEN** first FullMix or selected V1 has obtained its own public nonmusical Intro Seek acknowledgement with ON prefix-and-loop union or OFF complete FullTrack coverage, then its own public Pause
- **WHEN** a distinct complete V2 is durably committed and obtains fresh own registration, Capture, off-thread preparation and guarded Adopt while that voice remains paused
- **THEN** exact source position, voice generation, both immutable reader generations, Native/FIFO and filter history SHALL remain frozen through irregular callbacks and controller polls, with an unconsumed handover and no V2 effectiveness
- **AND** only own public Resume, consumed-source handover and dry OFF or later complete exact-history Native Wet ON output SHALL establish V2 effectiveness while still BeforeLoop
- **AND** natural return and further NormalLoop output SHALL preserve voice, source chronology and another independently paused/resumed pad's absolute trajectory; model-free reopen SHALL verify saved V2 and loop/KEY LOCK intent separately

#### Scenario: Public musical Intro and Tail preserve accepted return geometry

- **GIVEN** own callback-accepted Automatic timing whose fractional musical loop period differs from physical extent, and first FullMix or selected V1 obtains its own public Intro or Tail Seek with ON physical union or OFF complete FullTrack coverage
- **AND** the current voice is running or publicly paused before V2 Capture
- **WHEN** a distinct complete V2 obtains durable disk selection, own registration, Capture, off-thread preparation and guarded Adopt through the public StemController
- **THEN** exact own timing projection and ACK epoch SHALL remain authoritative for both real generations through physical one-shot playback, the consumed-source handover and natural return to both virtual seam taps of the accepted musical loop
- **AND** independent complete PCM, raw Native/FIFO or dry buffers and uninterrupted nonneutral filtering SHALL prove distinct own V2 output; ON effectiveness SHALL require its own later complete exact-history Native preparation
- **AND** paused callbacks SHALL consume no source or handover and report no new effectiveness; own Resume SHALL preserve source fraction, actual voice, Native/FIFO and reader owners
- **AND** natural return and multiple later musical seams SHALL cause no extra Seek, reset or launch; another active pad SHALL retain its independent absolute trajectory and owners, and saved reopen SHALL retain V2 and loop/KEY LOCK intent
- **AND** this individual scenario SHALL NOT establish consolidated task3.3 closure; domain-specific final-reader retirement, full L4 and Human acceptance SHALL remain separate gates

#### Scenario: Own in-loop and equal-phase Seek precede fresh new-set adoption

- **GIVEN** one current FullMix or selected V1 voice on a proper partial loop with KEY LOCK OFF or ON and physical or callback-accepted musical timing
- **WHEN** an own public changed-position or exactly equal-phase in-loop Seek receives its real callback ACK, followed by fresh V2 registration, Capture, off-thread preparation and guarded Adopt while running or paused
- **THEN** the intentional Seek SHALL cut voice/Native/FIFO/filter chronology even for an equal-phase target, retaining its actual source/window, rate and accepted timing authority
- **AND** a covered direct preserved-window ACK SHALL retain the actual window and both current reader generations without requiring a Relocate command
- **AND** V2 adoption SHALL preserve the post-Seek voice, source fraction, Native/FIFO/filter chronology and exact own accepted timing through the consumed-source handover and subsequent physical or virtual seams
- **AND** paused callbacks SHALL consume no source or handover and report no V2 effectiveness before own Resume; another pad SHALL retain its independent trajectory, and reopen SHALL retain saved V2 and loop/KEY LOCK/timing intent

#### Scenario: Complete-resident partial loop retains its actual geometry

- **GIVEN** a genuine acknowledged FullTrack OFF or KeyLockFullTrack ON source physically covering `[0,N)` with a proper partial loop `[S,E)` and physical or callback-accepted musical timing
- **WHEN** first FullMix or selected V1 obtains its own fresh V2 transaction while stopped, running or paused
- **THEN** Capture and Adopt SHALL retain the actual complete source layout and partial-loop geometry rather than replacing the loop by `[0,N)`
- **AND** independent complete PCM and actual dry or raw Native/FIFO plus uninterrupted filter output SHALL verify running or resumed exact two-reader handover and later physical or accepted musical seams
- **AND** stopped readiness SHALL require own later Play and fresh own effect authority plus independently verified V2 output, without claiming a stopped audible V1-to-V2 handover or borrowing an obsolete readiness permit; paused adoption SHALL require own Resume and source consumption before V2 effectiveness
- **AND** actual source/window/timing/voice/history authority, independent other-pad trajectory and model-free saved reopen SHALL remain separate from preparation acceptance

#### Scenario: Complete KEY LOCK Intro and Tail preserve the acknowledged exception

- **GIVEN** one current FullMix or selected V1 voice with genuine physically complete KeyLockFullTrack residency and a proper partial loop, with physical or callback-accepted musical timing
- **WHEN** its own acknowledged public Intro or Tail Seek is followed by fresh V2 Capture, off-thread preparation and guarded Adopt while running or paused
- **THEN** both actual complete reader generations SHALL cover the entire physical one-shot and all later loop taps through the own consumed-source handover
- **AND** own later exact-history Native Wet output SHALL establish V2 effectiveness; paused callbacks SHALL freeze the source, ramp, Native/FIFO/filter and owners until own Resume
- **AND** natural return and later physical or accepted musical seams SHALL preserve the post-Seek voice without extra Seek, launch or filter reset
- **AND** incomplete physical bounds, stale pre-Seek or timing authority, ordinary same-set Intro/Tail requests, frozen old-source and multiple same-pad voices SHALL remain guarded with current audio usable

#### Scenario: Musical Intro and Tail reject an obsolete own timing ACK

- **GIVEN** own accepted musical Intro or Tail, KEY LOCK ON or OFF, running or paused, with a selected usable V1 and a genuine queued V2 Capture or prepared Adopt
- **WHEN** a newer real timing publication obtains that pad's own callback ACK before the held old operation is dispatched
- **THEN** the old V2 operation SHALL reject before claim and effectiveness without borrowing the newer accepted projection
- **AND** dispatch of that stale operation SHALL preserve the actual current source fraction, voice generation, pause state, used Native/FIFO and current immutable reader owners; the newer own timing ACK SHALL remain authoritative

#### Scenario: Current KEY LOCK Tail adopts first and replacement sets

- **GIVEN** a single current running voice after a genuine acknowledged Seek into Tail, KEY LOCK ON without accepted musical timing, and actual FullMix or selected V1 readers covering the entire Tail-and-loop union
- **AND** the suffix is long enough to prove actual own 4096-output-frame Native preparation and nontrivial Wet output both before and after the new-set handover while the voice is still AfterLoop
- **WHEN** a distinct verified V2 obtains fresh own registration, Capture, off-thread preparation and guarded Adopt while ordinary productive callbacks advance the Tail
- **THEN** both exact immutable generations SHALL cover all current and future Tail/loop taps throughout the 128-consumed-source-frame ramp; independent complete PCM SHALL establish the selected V2 feed and its distinct Wet effect
- **AND** V2 adoption SHALL preserve the existing voice, source fraction, actual Native/FIFO and uninterrupted post-seek filter history; later Native adoption SHALL use its own advanced exact target checkpoint
- **AND** the actual source-end-to-loop-start return SHALL preserve fractional playback, exact PCM, Native/FIFO and filter chronology without another seek, reset or retrigger
- **AND** this running ON candidate proof alone SHALL NOT authorize Intro, paused Tail, KEY LOCK OFF or accepted-musical Tail, ordinary same-set mode requests, frozen old-source voices, multiple same-pad voices or whole task3.3 acceptance

#### Scenario: Public StemController adopts first and replacement sets after own Tail Seek

- **GIVEN** the public playback seek-only API has obtained its own acknowledged Tail window for a single current running KEY LOCK ON voice without accepted musical timing
- **AND** FullMix or selected V1 has established its own complete Native preparation and productive Wet output while still AfterLoop
- **WHEN** a distinct complete V2 is durably committed and the public StemController requests its own candidate registration, Capture, worker preparation and guarded Adopt
- **THEN** disk selection and accepted preparation SHALL remain distinct from actual effectiveness until that transaction's own 128-source-frame handover and later complete Native preparation establish V2 Wet while still AfterLoop
- **AND** independent complete PCM, raw Native/FIFO and uninterrupted nonneutral filter history SHALL verify post-Seek output, natural return to NormalLoop and continued playback; candidate adoption SHALL introduce no additional seek, reset or launch
- **AND** another active pad SHALL retain its independent absolute source trajectory and authority; neither its ACK nor an earlier V1 receipt SHALL confirm V2
- **AND** model-free reopen SHALL verify the saved committed selection without treating persistence as audible acknowledgement

#### Scenario: Public KEY LOCK OFF Tail adopts with complete resident coverage

- **GIVEN** a single current running KEY LOCK OFF voice without accepted musical timing and its own public seek-only acknowledgement into Tail
- **AND** the acknowledged source and its actual FullMix or V1 readers physically cover the complete FullTrack `[0,N)` including the entire Tail-and-loop union
- **WHEN** a distinct complete V2 obtains fresh own source authority, durable disk selection, registration, Capture, off-thread preparation and guarded Adopt through the public StemController
- **THEN** independent complete PCM and fractional one-shot source addressing SHALL verify both exact generations through the 128-consumed-source-frame ramp, subsequent distinct dry V2 output and natural return to NormalLoop
- **AND** adoption and continued output SHALL retain the actual voice, Native address/FIFO and uninterrupted nonneutral filter chronology; productive Native history and Wet readiness SHALL remain absent
- **AND** disk/preparation status and obsolete V1 receipts SHALL NOT confirm V2 effectiveness before its own actual source consumption; saved reopen SHALL verify persistence separately
- **AND** another active pad SHALL retain its absolute physical source trajectory and independent owners; this running OFF proof alone SHALL NOT authorize paused Tail, accepted-musical Tail, Intro, ordinary same-set requests, frozen old-source or multiple same-pad voices

#### Scenario: Public paused Tail adopts only through its own resumed consumption

- **GIVEN** first FullMix or selected V1 has obtained its own public nonmusical Tail Seek acknowledgement with KEY LOCK ON union coverage or KEY LOCK OFF complete FullTrack coverage, then its own public Pause
- **WHEN** a distinct complete V2 is durably committed and obtains fresh own registration, Capture, off-thread preparation and guarded Adopt while that voice remains paused
- **THEN** irregular silent callbacks SHALL retain the exact source fraction, generation, actual Native/FIFO, productive and filter history, occupied source-frame ramp and both required immutable reader owners; the claimed receipt SHALL remain transitioning without own audible effectiveness
- **AND** the voice's own public Resume SHALL preserve that exact checkpoint; independent complete PCM SHALL verify the subsequent 128-consumed-source-frame handover and uninterrupted nonneutral filter output
- **AND** OFF SHALL establish its own distinct dry output without Native Wet preparation; ON SHALL establish its own advanced exact-history 4096-output-frame preparation and distinct Wet output while still AfterLoop
- **AND** the natural Tail-to-NormalLoop return and further playback SHALL preserve the voice and source/Native/FIFO/filter chronology without another Seek, reset or launch
- **AND** another independently paused and resumed pad SHALL retain its own absolute consumed-source trajectory and owners; saved reopen SHALL verify V2 and loop/KEY LOCK intent separately from audible acknowledgement
- **AND** ordinary same-set requests, frozen old-source and multiple same-pad voices SHALL remain guarded; this individual paused proof SHALL NOT establish consolidated task3.3 closure, full L4 or Human acceptance

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

#### Scenario: Publicly born A and ordinary B replacement retain separate audio lifecycles

- **GIVEN** source A was started through actual public Transport, has its own effective nonneutral KEY LOCK ON output with FullMix or selected ALL, and an unrelated pad retains its independent source trajectory
- **WHEN** ordinary public Loader admits different source B and subsequently receives its matching assignment acknowledgement
- **THEN** A SHALL remain productive through preparation and pending acknowledgement; only B's own assignment acknowledgement SHALL stop A and install B without automatically starting B
- **AND** actual Loader completion SHALL project B's own identity and incoming defaults before later public Play derives B controls; B SHALL obtain its own window acknowledgement, explicit launch and actual START feedback
- **AND** addressed B ON, when requested, SHALL confirm its own actual wet output separately from the source/window acknowledgement and from A's historical Native adoption
- **AND** B's first and replacement live stem selections SHALL obtain distinct durable source-bound pairs and own Capture/worker/Adopt receipts, preserve B voice/source/filter chronology through each handover and confirm only their own dry or subsequently prepared wet effect
- **AND** output SHALL match independent complete B original/component PCM, declared per-voice source trajectories, Native/FIFO and uninterrupted EQ without borrowing A or another pad's rate, loop or receipt
- **AND** actual outgoing, Native/history and asset owners SHALL retain B's predecessor set until final use; production off-thread retirement and actual final readers SHALL precede cleanup of its WAV, PCM and descriptor areas
- **AND** B's current pair SHALL remain usable until its own public Unload acknowledgement and actual final-reader retirement, while the unrelated pad and persistence retain their own state
- **AND** separately proved queued predecessor fences and native non-replacing frozen-A restore SHALL retain their scopes; this ordinary assignment replacement SHALL NOT create post-acknowledgement frozen-A playback or close full domain/music/resource/Human gates

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

#### Scenario: Durable replacement waits for the superseding Unload snapshot
- **GIVEN** a complete V2 pair replaced the config successfully with independent new and predecessor settlement owners, and a subsequent public Unload changed current intent
- **WHEN** the superseding config flush fails after that actual replacement
- **THEN** the exact settlement SHALL retain both generations and its cleanup reservation for bounded retry, even after the own Unload acknowledgement and actual predecessor audio retirement
- **AND** the last durable config SHALL still restore its complete V2 selection, while current memory SHALL preserve the newer Unload intent
- **AND** leased files SHALL remain byte-identical and protected from overwrite during independent off-thread asset-worker progress
- **WHEN** the ordinary writer durably saves that newer intent and bounded settlement polling completes
- **THEN** the stale action SHALL release its exact owners and complete pair readers, allowing eligible predecessor and V2 cleanup through the existing asset worker without late registration or adoption
- **AND** unrelated assignments and playback SHALL remain usable

#### Scenario: Sidebar separates a saved replacement from its own audio confirmation
- **GIVEN** a pad retains its own old-set audio confirmation for the current launch and a different complete current set has committed to disk
- **WHEN** the sidebar renders before the new set's own effective confirmation, including no request, waiting, transition or failed admission/adoption
- **THEN** it SHALL separately describe the saved new set, its pending or failed switch and the previously confirmed audio
- **AND** it SHALL NOT present the old set's confirmation as confirmation of the saved replacement or imply that a last-confirmed description measures current transition audio
- **WHEN** that pad receives the saved new set's own effective confirmation
- **THEN** the sidebar SHALL show the new confirmed audio without the previous-set description
- **AND** reopening verified saved stems without an audio confirmation SHALL remain unconfirmed and SHALL NOT require loading a separator model to render that status
- **AND** an own effective FullMix confirmation SHALL remain visible independently of a withdrawn pending new-set registration

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

#### Scenario: Productive replacement retires through both native owner lanes
- **GIVEN** V1 has its own effective KEYLOCK ON output and V2 has a fresh complete-set Capture/Adopt transaction
- **WHEN** the 128-source-frame handover finishes while callback retirement has no capacity
- **THEN** the outgoing V1 reader SHALL remain owned with continuous source, Native/FIFO and filter output
- **WHEN** capacity returns and the existing audio retirement worker consumes that outgoing reader
- **THEN** the still-used V1 Native/history reader SHALL continue protecting its immutable WAV, PCM and descriptor areas
- **AND** only V2's own later complete Native preparation and retirement of all remaining V1 readers SHALL allow the existing off-thread asset worker to clean up V1
- **AND** V2, the current original and unrelated playing pads SHALL remain usable

#### Scenario: Unload retires a queued replacement through its actual readers
- **GIVEN** selected V1 has its own effective KEYLOCK ON output and a distinct V2 Adopt remains queued and unclaimed
- **WHEN** the pad's own public Unload revokes publication while callback retirement capacity delays command consumption
- **THEN** the queued V2 and still-playing V1 SHALL retain their immutable WAV, PCM and descriptor files through their actual reader ownership
- **WHEN** the FIFO rejects the obsolete V2 Adopt before consuming that Unload
- **THEN** V1 SHALL remain usable until Unload's own callback `SampleStopped` acknowledgement, separately from the candidate rejection
- **AND** only the actual audio retirement worker and final Native/history/complete-reader release SHALL permit the off-thread asset worker to remove each retired pair's three areas
- **AND** the current original, unrelated saved assignments and unrelated playing pads SHALL remain usable

#### Scenario: Seek and one-shot replacement releases its actual final readers
- **GIVEN** productive selected V1 KEY LOCK ON playback in a supported acknowledged changed/equal in-loop Seek, finite Intro/Tail, complete-source partial-loop Normal/Intro/Tail or complete-source Normal context, and V2's own claimed replacement
- **WHEN** the pad is publicly paused during its consumed-source handover
- **THEN** source phase, Native/FIFO/history, the occupied transition and both immutable generations SHALL remain frozen and their WAV, PCM and descriptor areas SHALL remain protected
- **WHEN** own Resume finishes that handover and actual callback retirement releases the outgoing lane
- **THEN** the still-used predecessor Native/history and complete PCM readers SHALL continue protecting V1's three areas
- **AND** cleanup SHALL wait until V2's own later complete Native preparation and actual retirement release every remaining V1 identity and component PCM owner
- **WHEN** the later own public V2 Unload is consumed and reports its own `SampleStopped` acknowledgement
- **THEN** V2 cleanup SHALL likewise wait for every actual remaining Native/history/complete-reader owner; the original and unrelated playing pads SHALL remain usable throughout

#### Scenario: Seek-domain queued Unload preserves both generations until final release
- **GIVEN** effective V1 KEY LOCK ON playback in a supported finite musical Tail or actual complete-source musical Intro and a distinct unclaimed V2 Adopt in the existing FIFO
- **WHEN** own public Unload revokes that candidate before callback command consumption
- **THEN** actual queued V2 owners and still-productive V1 Native/history/PCM owners SHALL protect their exact three-area files
- **AND** candidate rejection SHALL NOT substitute for the Unload's own callback `SampleStopped` acknowledgement
- **WHEN** that Unload is consumed and actual off-thread retirement releases each generation's final identity and component PCM owners
- **THEN** the existing asset worker SHALL clean only the eligible exact retired areas, while preserving the original and unrelated playing pads

#### Scenario: Frozen old-source current and outgoing sets survive bank replacement until own Unload
- **GIVEN** source A's current ON voice has claimed V2 while V1 remains its outgoing reader and used Native/history source during an unfinished handover
- **WHEN** an actual native restore admits distinct current bank source B without replacing A's saved assignment
- **THEN** the retained A voice SHALL freeze its exact V2 current and V1 outgoing sets, source trajectory, transition and required Native/history owners independently of B's bank and preparation authority
- **AND** actual bank-owner retirement SHALL NOT release those frozen readers or permit either generation's three-area cleanup; strict productive output SHALL continue from A's complete PCM and uninterrupted filter history
- **WHEN** own public Unload releases the saved A selection and its own callback reports `SampleStopped`
- **THEN** both frozen set generations SHALL retire through the existing audio worker and all remaining Native/history/complete readers before physical asset-worker cleanup
- **AND** actual expiration of both complete identities and every component PCM owner SHALL precede removal of their exact WAV, PCM and descriptor areas; A's and B's originals and unrelated playing pads SHALL remain usable
- **AND** this retained-reader proof SHALL NOT authorize a current-bank candidate to borrow A's frozen voice or substitute native restore for public Loader assignment replacement

#### Scenario: Frozen outgoing completion keeps current stems live through later retained-voice preparation
- **GIVEN** retained source A owns claimed V2, outgoing V1 and its used V1 Native/history under distinct actual bank B, while its candidate receipt is obsolete
- **WHEN** the retained voice is publicly paused and resumed without a new source assignment
- **THEN** paused output SHALL preserve both exact frozen generations, source phase, occupied transition and Native/FIFO/history, and own Resume SHALL continue their consumed-source chronology
- **WHEN** that frozen handover finishes while callback retirement capacity is exhausted
- **THEN** outgoing V1 SHALL remain owned and immutable until capacity returns and the actual audio worker consumes its outgoing retirement; used V1 Native/history SHALL continue protecting its files afterward
- **WHEN** the retained A voice's own later source-bound Native preparation covers the required complete history after Resume and handover, and is actually adopted
- **THEN** the retained-voice authority SHALL bind that preparation to A's source, voice and current frozen V2 independently of B's bank; it SHALL NOT rehabilitate the obsolete candidate receipt
- **AND** only expiration of every V1 complete identity and component PCM owner SHALL permit its exact three-area cleanup while A/V2 remains productive with continuous source, Native/FIFO and filter output
- **WHEN** the later own public V2 Unload reports its own callback `SampleStopped` acknowledgement
- **THEN** actual remaining frozen, Native/history and complete-reader release SHALL precede V2 identity/component expiration and its exact three-area cleanup, preserving both originals and unrelated playing pads

#### Scenario: Cleanup races a new generation or fails sharing
- **WHEN** cleanup encounters a new generation, unknown file, external path, reparse point or sharing denial
- **THEN** it preserves non-owned content and defers/report failures through bounded control status
- **AND** no callback deletion or immediate recursive material removal occurs

#### Scenario: A shared running job outlives its final public subscriber
- **GIVEN** two independently loaded pads requested the same verified managed original through one actual shared Python generation job, whose backend read is still running
- **WHEN** the origin is publicly unloaded
- **THEN** only that subscriber's interest SHALL end and the surviving subscriber's job SHALL remain current
- **WHEN** the final subscriber is publicly unloaded and both own Unload acknowledgements and actual Native retirement complete before the backend returns
- **THEN** the running job SHALL retain its actual original/source and private-generation custody and reserved cleanup capacity through independent asset-worker progress
- **AND** the original SHALL remain byte-identical and protected from overwrite, while known private backend outputs SHALL remain present
- **WHEN** that backend actually returns and control settles its stale event
- **THEN** the job SHALL release its real custody and reservations, allowing the shared off-thread asset worker to finish eligible original and private-generation cleanup without a late publication
- **AND** unrelated current assignments and playing pads SHALL remain usable

#### Scenario: Engine shutdown precedes final lease and sharing release
- **GIVEN** exact unassigned owned-original retirement receipts retain their inventory custody, transient source/job leases remain live and a real Windows handle denies deletion of one target
- **WHEN** the actual last Engine owner shuts down and those leases release afterward
- **THEN** the shared off-thread asset worker SHALL continue processing eligible unrelated targets while preserving the blocked target bytes and pending receipt
- **WHEN** the blocking handle releases
- **THEN** the same queued retirement SHALL complete without re-admission or a direct collector call
- **AND** inventory custody SHALL remain through physical cleanup and unknown neighboring files SHALL remain unchanged

#### Scenario: A queued target changes after retirement admission
- **GIVEN** an exact verified original or complete WAV-generation retirement is pending behind its actual transient source/job lease
- **WHEN** an unknown child enters the generation, the original path is replaced or the original's bytes change after admission and before that lease releases
- **THEN** the shared off-thread asset worker SHALL recheck the admitted identity, complete known leaves and byte evidence without removing any leaf of the changed target
- **AND** it SHALL preserve the unknown file, replacement object or edited bytes and expose a bounded terminal error on that target's own receipt
- **AND** it SHALL release that retirement's inventory custody after reporting preservation and SHALL NOT resume the failed retirement merely because the unknown child is later removed
- **AND** independent eligible retirement targets SHALL continue to complete

#### Scenario: A queued material path becomes a junction to external content
- **GIVEN** a valid original retirement retains its real inventory custody and transient source/job pin after exact admission and seal release
- **THEN** the inventory's ordinary Samples-root guard SHALL prevent root replacement while that custody is held
- **WHEN** an otherwise unguarded material subtree is replaced by a junction to external content before the final transient pin releases
- **THEN** the actual shared off-thread asset worker SHALL reject the changed route without deleting external content or the moved original
- **AND** the own receipt SHALL expose a bounded terminal containment error and release its inventory custody
- **WHEN** the caller removes only its junction and restores that material route without a new retirement admission
- **THEN** the rejected retirement SHALL NOT revive, and an independent eligible retirement SHALL still complete

### Requirement: Post-admission paired PCM cleanup preserves unknown content
The system SHALL recheck complete known PCM leaves at physical off-thread cleanup
after valid paired retirement admission. An unknown child added while actual
Native/history readers retain the predecessor SHALL preserve the affected PCM
target and its known leaves, and SHALL expose a bounded error identifying that
target after final reader release. Unchanged independently retired WAV and
descriptor targets MAY complete; cleanup SHALL NOT delete the unknown content
or resume the terminally rejected PCM retirement merely after that child is removed.

#### Scenario: Unknown PCM child arrives while predecessor Native history remains
- **GIVEN** V1's three-area retirement was admitted before an unknown PCM child existed and V1's actual Native/history readers still retain its files
- **WHEN** the child is added, V2 completes its own later Native preparation and V1's actual final readers end
- **THEN** the shared asset worker SHALL preserve every known leaf and the unknown child in the affected PCM generation, with a bounded error for that exact PCM path
- **AND** independent eligible cleanup SHALL continue, while current V2, the original and unrelated playing pads remain usable
- **WHEN** the caller later removes its own unknown child without a new retirement admission
- **THEN** the rejected PCM target SHALL remain preserved

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

#### Scenario: A's final audio readers end while C remains saved
- **GIVEN** independent C has its own genuine saved V1 assignment and has requested no replacement or audio publication
- **WHEN** A's own V2 adoption completes and every actual predecessor Native/history/PCM reader ends
- **THEN** C's assignment SHALL continue retaining byte-identical V1 WAV, PCM and descriptor files while independent eligible off-thread cleanup continues
- **WHEN** A receives its own public Unload acknowledgement and the saved project reopens without a model
- **THEN** A SHALL remain removed and C SHALL retain its exact independent V1 selection and saved intent
- **WHEN** C explicitly deletes its stems and releases the final V1 assignment
- **THEN** the existing off-thread asset worker SHALL remove V1's three areas without deleting the current original or unrelated assignments
