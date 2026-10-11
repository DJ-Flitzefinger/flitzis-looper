## ADDED Requirements

### Requirement: Shared material complete PCM and retained range readers
The system SHALL own complete versioned FullMix PCM and all aligned complete
stem PCM beneath each immutable material version's `.pcm-cache` while
retaining original/content/rate/transform/schema/generation and assignment identities.

#### Scenario: Verified descriptors supply a new window
- **WHEN** a current pad requests a different proved finite range
- **THEN** complete source identity and retained descriptor lineage stay unchanged
- **AND** only requested ranges/required context are loaded under byte reservation
- **AND** adoption still requires guarded native ACK and final-reader retirement
- **AND** fresh leases SHALL verify complete integrity while retained verified descriptors supply ranges without full WAV decode/alignment or hidden complete f32 allocation
- **AND** compatible backing MAY share in-process with equal independent content owners without duplicated files
- **AND** reads/validation/allocation/retirement SHALL remain bounded and off-thread

#### Scenario: A held verified pair supplies four final ranges
- **WHEN** an acknowledged paired set prepares a different finite source interval
- **THEN** its retained sealed reader SHALL supply exactly the four requested PCM intervals without reopening the complete pair verifier or decoding or realigning WAVs
- **AND** complete integrity work SHALL be measured separately from interval read bytes and final PCM allocations
- **AND** checked admission SHALL include distinct old backing, new final ranges and bounded conversion scratch before allocation
- **AND** an unchanged eligible interval SHALL share its actual backing while each pad keeps independent source, timing and window authority
- **AND** cancellation or rejection SHALL preserve prior effective audio and keep each old voice, job and history reader until its own final use

#### Scenario: Cross-area complete set ownership remains coupled
- **WHEN** WAVs reside in the material version's `stems` and all PCM resides in that version's `.pcm-cache`
- **THEN** common descriptors SHALL bind both immutable generations/source/transforms/digests
- **AND** joint publication/final-reader retirement SHALL prevent half-complete eligible pairs

### Requirement: Complete 216-slot residency and local refresh
The system SHALL support actual usable acknowledged FullMix ranges for all216
occupied slots and four-component ranges for all216 eligible stem-demand slots
within a declared sufficient supported resource configuration.

#### Scenario: All216 unique pads reach actual readiness
- **WHEN** all216 occupied slots have valid bounded geometry and a sufficient declared budget
- **THEN** bounded scheduling eventually obtains actual ACKed usable windows for all216
- **AND** off/preload-on and explicit all-stems cases are independently exercised
- **AND** readiness requires no216-voice claim

#### Scenario: One edit preserves215 valid pad windows
- **WHEN** one pad's loop is changed after all216 are ready
- **THEN** only its affected windows prepare and adopt
- **AND** the other215 views retain handles/revisions and remain usable
- **AND** source/loop setup SHALL reconcile every occupied slot while an edit renews only affected read sets

#### Scenario: Resource failure is incomplete readiness
- **WHEN** a required window cannot fit configured bytes or owner capacity
- **THEN** it reports finite recoverable preparation/admission failure
- **AND** partial ready count cannot satisfy the all216 acceptance obligation
- **AND** requested/queued/preparing/ACKed/error and exact-byte totals SHALL remain distinct

### Requirement: Finite DSP coverage is a feature completion gate
The system SHALL prove and implement finite source-domain supply for the supported
loop/DSP context before claiming completion of shared-material loop residency.

#### Scenario: Dry parity is insufficient for KEYLOCK completion
- **WHEN** dry interpolation passes but finite native continuation remains unproved
- **THEN** the supported labelled fallback or prior effective audio remains available
- **AND** finite-DSP and complete-feature acceptance stay open
- **AND** admitted full-track KEYLOCK fallback MAY remain temporarily but SHALL NOT close finite supply

#### Scenario: Actual finite continuation is adopted
- **WHEN** a bounded native/history read plan proves all required source inputs and state
- **THEN** its complete-buffer regression oracle and causality checks precede native ACK
- **AND** no hidden callback disk read, long scan or missing-sample silence is introduced
- **AND** proof SHALL cover interpolation/P-H seam/rate smoothing/Rubber Band feed/lookahead/native/FIFO/history/filter/current and pinned voices/both transition sides
- **AND** guessed halos or unavailable future input SHALL NOT authorize readiness
- **AND** accepted P/integer H/one rate owner and acoustic/latency/1598-frame33.292-ms fixtures SHALL remain intact

#### Scenario: Normal-loop native preparation and continued rendering share finite input
- **WHEN** a stopped current source obtains a finite Normal-loop window with production k=0 KEYLOCK and a non-unity ratio
- **THEN** retained verified readers SHALL prepare only the physical loop range and the existing window transaction SHALL obtain its own callback ACK
- **AND** a copied SourceReadPlan and binary64 SourcePlayback trajectory SHALL prove both executed interpolation taps within held FullMix and selected component ranges, including virtual P/H seams and rate smoothing
- **AND** the existing NativeHistoryRequest/Permit SHALL bind complete source identity, actual window/revision, timing projection, selection and stem-set ownership before real native/FIFO catch-up and exact adoption
- **AND** callback rendering SHALL continue through that same actual native adapter with independently held complete PCM and raw-native output as regression oracles
- **AND** a missing tap, stale source/window/rate/selection, cancellation or saturated admission/recycle lane SHALL preserve prior effective audio without treating missing input as successful silence

#### Scenario: A finite storage refresh preserves already adopted native chronology
- **WHEN** an acknowledged same-source Normal-loop finite window changes only resident storage while native history is already adopted
- **THEN** pending native preparation SHALL remain bound to its exact captured window
- **AND** adopted history MAY retain the same effective-source permit, actual native handle, FIFO and filter chronology while its original reader pins remain held until final use
- **AND** unproved seek/intro/tail, physical-domain cuts and every unproved finite context SHALL retain their guards and keep the complete P4b gate open

#### Scenario: An already resident selected set supplies both finite transition sides
- **WHEN** current-source NormalLoop KEYLOCK playback at production k=0 and a non-unity ratio receives an existing native scalar mode or mask command for the identical already ACKed resident selected committed StemSet
- **THEN** prospective admission and executed feed SHALL cover both the actual target selection and active outgoing selection against held FullMix and four component ranges, including FullMix-to-stems, stems-to-FullMix and nonempty or empty masks
- **AND** complete source/component-owner equality, source/window/revision, accepted timing, loop period/seek mode and canonical smoothed-rate trajectory SHALL remain bound by the existing independent permits and ACKs
- **AND** SourceReadPlan/fractional_taps and SourcePlayback SHALL remain the actual source-address/domain/rate authority through the existing 128-source-frame ramp and interruption policy
- **AND** an inactive verified pair publication followed by playback and those scalar commands SHALL provide the real admitted Some/Some seam, without private fixture flags or target-default substitutions for an outgoing selection
- **AND** a missing tap, changed owner/selection or stale ACK SHALL preserve prior effective audio rather than authorizing missing-input silence

#### Scenario: Finite ramp deferral preserves wet chronology before later actual adoption
- **WHEN** both sides of an admitted resident-selection ramp have proved finite NormalLoop coverage while source-specific preparation is deferred until the ramp settles
- **THEN** the actual wet native handle, input/output FIFOs, logical cursor/loop/timing and chronological filter SHALL continue throughout the ramp without reset or dry fallback
- **AND** subsequent settled-selection preparation SHALL process real source input and adopt through the existing request/ready/recycle lanes only with matching Source/Window/Stem/History permits and the exact checkpoint
- **AND** independent complete PCM and algebraic raw-native/chronological-filter oracles SHALL verify continued nontrivial output, both executed taps/feed and irregular callback partitions crossing adoption with rate smoothing
- **AND** cancellation, late/stale results or saturated admission/recycle capacity SHALL preserve the prior owner and audio until actual safe retirement

#### Scenario: Outgoing components and history retire at their distinct final uses
- **WHEN** the existing pair-FullMix command completes an admitted finite stems-to-FullMix transition
- **THEN** resident components SHALL remain held through the outgoing selection's actual final ramp use and SHALL retire through the existing producer/callback path
- **AND** native-history, job, old and new reader pins SHALL remain held until each actual final use and SHALL release through existing lifecycle/registry/recycle paths with checked peak admission
- **AND** instrumental SHALL NOT become a fifth live component or a hidden complete backing
- **AND** this resident Some/Some transition proof SHALL NOT close initial active None-to-Some, retention/effective-mode ACK or ordinary StemController warm return, which remain P5a obligations
- **AND** this resident-transition proof alone SHALL NOT close finite pause/resume/retrigger/timing refresh, old-source voices, seek/intro/tail or the complete P4b musical/resource/causality matrix

#### Scenario: Finite pause and resume preserve productive chronology but fence pending work
- **WHEN** a proved finite NormalLoop voice with production k=0 KEYLOCK and a non-unity settled or smoothing ratio pauses or resumes before or after source-specific preparation capture or an admitted storage-only ACK
- **THEN** its actual source position, adopted native handle, input/output FIFOs and chronological filter SHALL retain continuity through the existing pause/resume commands
- **AND** pending preparation SHALL retain its exact captured source/window/selection/timing/playback binding and SHALL be rejected when its capture or checkpoint is stale without mutating current audio
- **AND** only already adopted history MAY use the proved storage-only effective-source permit while its real original reader pins remain held
- **AND** resumed wet output SHALL continue beyond the first resumed callback through subsequent real request, preparation and adoption decisions, with no dry fallback or reset inferred from a retained pin
- **AND** independent complete PCM and algebraic raw-native/chronological-filter oracles SHALL observe nontrivial continued output, executed taps/feed/lookahead, native identity, FIFO occupancy and cursor across irregular partitions crossing pause/resume, rate ramps and ACK/adoption

#### Scenario: Same physical region permits only the chronological accepted-period alias
- **WHEN** a current finite NormalLoop source receives an accepted-period refresh or clear with unchanged physical loop geometry and an independently acknowledged current source/timing binding
- **THEN** the existing narrow chronological accepted-period projection alias SHALL preserve native/FIFO/filter state and the equivalent wrapped fractional phase across virtual P plus or minus H seams
- **AND** pending preparation SHALL retain its exact captured projection and checkpoint guards while new requests SHALL capture the new effective projection
- **AND** source changes, marker changes, seek or a discontinuous source position SHALL NOT borrow that alias
- **AND** refresh or clear for active or paused voices belonging to an older replacement source SHALL reject before changing their audio, timing, physical region or frozen selection
- **AND** actual continued finite wet output and later request/adoption decisions SHALL match independent complete PCM and raw-native/chronological-filter references

#### Scenario: Replacement bank authority cannot relabel an old finite voice
- **WHEN** bank B replaces bank A while an active or paused finite NormalLoop voice still owns source A
- **THEN** that voice SHALL retain its actual source A, physical region, timing, FrozenStemView, transition and productive history under their original ownership
- **AND** bank B's source generation, timing, selection or ACK SHALL NOT authorize new source A preparation or relabel the retained voice
- **AND** a current-bank candidate captured before replacement SHALL remain invalid after replacement even if an old-voice PCM pin is later retained
- **AND** any new retained-voice preparation SHALL use fresh actual Voice/Source/Timing/Selection/History capture and matching independent permits from existing admitted mechanisms
- **AND** source A SHALL continue nontrivial wet output after resume and through further real request/preparation/adoption decisions against independent complete-source A PCM and raw-native/filter references
- **AND** SourceReadPlan/fractional_taps and SourcePlayback SHALL remain the actual source-domain, address and rate authorities

#### Scenario: Future-bank range ACK is separate from native preparation and retrigger
- **WHEN** actual existing WindowWork prepares and admits finite ranges for future bank B while source A still plays or remains paused
- **THEN** B SHALL obtain its own real callback window ACK without changing source A's voice, parameters or owners
- **AND** that window ACK alone SHALL grant no native-history preparation permission; fresh public capture and independent Source/Timing/Stem/History permits SHALL govern real preparation and exact checkpoint adoption
- **AND** an explicit retrigger SHALL perform the existing intentional DSP/history cut into the independently acknowledged source B
- **AND** failed retirement or admission reservation SHALL preserve old playback, parameters and all owners before any effects
- **AND** complete-source B PCM and raw-native/filter references SHALL verify subsequent nontrivial finite wet output through real preparation/adoption rather than only a readiness flag

#### Scenario: Lifecycle readers retire at their distinct actual final uses
- **WHEN** finite pause/resume, refresh/clear, bank replacement or retrigger leaves old and new voice, source, stem, job, native-history, reader-pin or outgoing-selection owners in flight
- **THEN** admission SHALL count actual distinct old/new held backing and native-history overlap, including replacement and late, cancelled or saturated work, before allocation or mutation
- **AND** each owner SHALL remain held through its own actual final use and SHALL release through the existing retirement and recycle paths off the realtime callback
- **AND** a retained PCM pin alone SHALL NOT become a native permit, a hidden complete backing or a fifth live instrumental component
- **AND** no new loader, reader, worker pool, queue or speculative lifecycle framework SHALL be introduced for this bounded context
- **AND** this lifecycle proof SHALL NOT close finite seek/intro/tail, physical-domain cuts, P5a active None-to-Some/retention/effective-mode ACK/ordinary StemController warm return or remaining complete P4b resource/musical/history/causality gates
- **AND** binary64 P, integer H, 119.999 through 123.45 BPM, the 75/1000-cycle tolerance of at most one loaded-source frame and the 48-kHz/rate-2/H16384/C3678/U2080 unavailable-at-T boundary of 1598 frames or 33.292 ms SHALL remain unchanged

#### Scenario: Finite explicit source contexts obtain their own productive ACK
- **WHEN** a held finite source at production k=0 KEYLOCK receives an explicit seek inside the loop, before the loop or after the loop
- **THEN** the existing capture and retained range worker SHALL bind the actual current or retained voice's complete source, physical loop, timing and accepted StemSet
- **AND** admitted finite coverage SHALL include the physical loop and the actual prefix for Intro or remaining source extent for Tail, including both fractional taps and source-end return
- **AND** the existing window/control ACK SHALL precede effects, and independent complete PCM and raw-native/filter oracles SHALL verify actual Native preparation, exact adoption and sustained wet continuation
- **AND** every explicit seek, including equal-phase seek, SHALL cut generation, pending Native/FIFO history and filter state while preserving canonical rate target and smoothing intent
- **AND** failure of coverage, ownership, cancellation or clean-handle/recycle admission SHALL preserve old usable audio before effects
- **AND** explicitly admitted complete-track Seek/ALL exceptions SHALL remain supported without imposing a total track-duration or disk-PCM limit

#### Scenario: Physical loop edits and return from complete exceptions retain only real continuity
- **WHEN** an admitted physical loop edit or finite replacement follows Intro, Tail or an explicit complete-track exception
- **THEN** actual copied SourcePlayback and SourceReadPlan SHALL prove the next context and both selected transition sides before adoption
- **AND** history MAY continue only when the exact next physical read remains continuous; a real clamp or jump SHALL discard Native/FIFO/filter history using admitted clean state
- **AND** pending preparation SHALL remain exact-source/window/timing/selection/history/checkpoint bound while already adopted continuous history retains its original readers until final use
- **AND** an old source voice SHALL retain its own source and fresh retained-voice permits independently of a replacement bank
- **AND** peak admission and true final-reader retirement SHALL use the existing workers, registry and recycle lanes without hidden complete backing, callback IO or a fifth live component
- **AND** this bounded result SHALL NOT close whole P4b, P5a/P5b, nonzero KEY/B5, resource/music or Human/device/hearing gates

### Requirement: First residency of the selected committed stem set preserves live audio
The system SHALL admit typed residency-only `None`-to-`Some` adoption of an already selected,
unchanged committed disk StemSet for production k=0 NormalLoop without stopping or retriggering
current playback, while retaining independent residency and effective-mode acknowledgements.

#### Scenario: Selected disk authority admits only the unchanged current set
- **GIVEN** the current source has its own selected verified committed disk StemSet but no resident components
- **WHEN** its pad requests first live ALL STEMS residency
- **THEN** a typed residency-only transaction SHALL bind that pad/source's complete content/cache/set identity, current source ticket and selected committed disk authority
- **AND** `None`-to-`Some` SHALL attach only that same selected set, not select another set or publish a new generation
- **AND** saved project intent, another pad's eligible set, old callback ACK or disk metadata alone SHALL NOT grant current live authority
- **AND** general prepared-set replacement and new-generation adoption SHALL retain their separate L3 guards

#### Scenario: Fresh current context precedes callback attachment
- **WHEN** first-residency work captures, prepares and reaches the existing callback adoption boundary
- **THEN** fresh guards SHALL match the actual current voice/source/request and STOP epoch, window/revision/geometry, accepted timing, selected set/mask and Native history/checkpoint
- **AND** copied SourcePlayback and SourceReadPlan SHALL prove both executed fractional taps, actual smoothed-rate feed and both FullMix/outgoing and target stem sides against held ranges
- **AND** the existing Source/Timing/Window/Stem/History permits, retained readers and reserved retirement/feedback capacity SHALL precede mutation
- **AND** stale source, voice, request, timing, window, selection or history SHALL reject without changing prior effective audio or treating unavailable input as successful silence
- **AND** native history, FIFO/filter chronology and independent complete-PCM/raw-Native continuation SHALL remain real proof obligations, not inactive-ready or private-fixture substitutes

#### Scenario: FullMix continues until own residency ACK and continuous mode adoption
- **WHEN** a playing current NormalLoop voice requests first residency while FullMix is effective
- **THEN** preparation and queued adoption SHALL leave its FullMix output, source position, loop, timing, Native/FIFO/filter history and effective selection unchanged
- **AND** only that matching request's own callback residency ACK SHALL authorize the existing 128-source-frame FullMix-to-stems ramp
- **AND** the ramp SHALL use the same canonical source addresses and active source-frame progress under rate smoothing and irregular callback partitions without stop, retrigger, cursor jump or Native reset
- **AND** a separate own source/request-bound effective-mode receipt SHALL distinguish pending, transitioning and actually adopted selection; enqueue or resident readiness SHALL NOT report effective ALL STEMS
- **AND** empty/nonempty masks and interrupted ramps SHALL use their actual captured selection and existing interruption policy

#### Scenario: Pause and retained old voices keep their own authority
- **WHEN** first residency crosses pause/resume or a bank/source replacement retains an old active or paused voice
- **THEN** paused source position, Native history/FIFOs/filter and ramp progress SHALL freeze and resume under the actual voice's source clock
- **AND** a current-source residency or mode receipt SHALL NOT relabel or authorize the retained old voice's source, frozen selection, readers or history
- **AND** any admitted operation affecting a retained voice SHALL obtain fresh authority for that actual voice/source through existing permits rather than borrowing the replacement bank's ACK
- **AND** stopped/paused readiness SHALL remain distinct from actual successful continuing playback and the own effective-mode result

#### Scenario: Stopped readiness and later superseding context remain truthful
- **WHEN** component residency is accepted for a stopped pad without actual voice output
- **THEN** the effective-mode receipt SHALL remain waiting rather than infer audible adoption from configured bank state
- **AND** an accepted no-ramp FULL MIX release SHALL retire its actual component control owners independently of continuing-output proof
- **WHEN** a later authorized ready Play, window change or STOP makes an already accepted operation receipt stale while the selected source/set remains current
- **THEN** the controller SHALL withdraw that historical receipt to unconfirmed without claiming a new live acknowledgement or reporting the successful past admission as a new failure
- **AND** it SHALL clear the withdrawn receipt's queued desired intent so periodic polling cannot automatically submit a replacement; an explicit mode gesture SHALL obtain fresh current authority and its own receipt
- **AND** pending or rejected transactions SHALL retain their strict source/window/voice/history/request/STOP guards and visible errors
- **AND** a known required source-specific Native preparation failure SHALL remain visible as rejected with its actual reason even after later context invalidation until a fresh own effective receipt succeeds
- **AND** an ordinary ready Play SHALL retain its existing ready launch and usable output without requiring new cold admission merely to refresh mode feedback

#### Scenario: Cancellation and bounded failure preserve owners and audio
- **WHEN** FullMix/STOP supersedes pending demand, work is cancelled, history changes, results arrive late or a worker/queue/coverage/byte/owner/recycle/feedback capacity check fails
- **THEN** stale first-residency work SHALL not attach components or begin a mode ramp
- **AND** prior usable audio and requested intent SHALL retain truthful pending/error state, subject only to an explicit superseding FullMix or STOP action
- **AND** actual distinct old/new backing and history overlap SHALL be reserved before allocation or mutation
- **AND** old voice, source, stem, job, history, outgoing-ramp and reader-pin owners SHALL remain held until each actual final use and retire through existing off-thread registry/recycle paths
- **AND** the callback SHALL perform no file/JSON I/O, Python/GIL/UI calls, blocking, logging, inference, heavy allocation/deallocation or unbounded preparation

#### Scenario: Ordinary ALL to FullMix to ALL returns after real retirement
- **WHEN** ordinary StemController ALL STEMS reaches effective selection, FullMix completes its outgoing ramp and resident components actually retire, then ALL STEMS is requested again
- **THEN** verified selection of the unchanged committed disk set SHALL remain available independently of retired resident handles
- **AND** the existing worker/controller path SHALL reacquire its required ranges with a new own residency ACK and separate effective-mode receipt without stop/retrigger or general inactive-set publication
- **AND** saved ALL intent or prior readiness SHALL NOT cause an early success return while components are absent
- **AND** outgoing components, Native history and job readers SHALL retire only at their distinct actual final uses; pinned old-source readers SHALL remain protected
- **AND** productive public-route/native continuation and failure tests SHALL prove both first activation and actual retired warm return
- **AND** this bounded P5a proof SHALL NOT close active new-generation L3, whole P4b/P5b, nonzero KEY/B5, aggregate resource/music or Human/device/hearing gates

### Requirement: Separate aggregate residency and preparation accounting
The system SHALL reserve unique resident backing and simultaneous pending/old
live resident allocations against the explicit aggregate ProjectState budget
before mutation while preserving separate existing bounded preparation limits.

#### Scenario: Shared backing and pinned overlap are accounted
- **WHEN** compatible pads share backing while an old window remains pinned
- **THEN** backing bytes are charged once with each owner retained
- **AND** new/old overlap is reserved without evicting live readers
- **AND** PCM byte counters are not reported as whole-process RSS
- **AND** exact PCM costs/conservative estimates/job overlap/measured process resources SHALL be separate

#### Scenario: Budget reduction meets active ownership
- **WHEN** a smaller budget is selected while pinned data exceeds it
- **THEN** old effective audio remains valid and above-budget state is visible
- **AND** idle retirement and new admission honor the bound without deleting active owners

### Requirement: Existing bounded lanes and exact ownership capacity
The system SHALL keep two workers,32 queued/reserved jobs,eight startup admissions
per poll,1GiB maximum simultaneous transient scratch PCM per job and ordinary512MiB timing/analysis preparation.

#### Scenario: Remaining contract constraints hold for every operation
- **WHEN** the system exercises this requirement
- **THEN** Complete on-disk source/stem extent SHALL NOT be charged as simultaneous scratch or rejected by its total length; E11-03 requires the separate streaming path.
Assignment/descriptor capacity SHALL cover216 plus bounded pending/old owners
with checked reservation, overflow rejection and off-thread retirement.

#### Scenario: Owner capacity is sufficient without multiplying voice lanes
- **WHEN** all216 unique FullMix and StemSet owners plus bounded overlap are exercised
- **THEN** actual owner accounting SHALL determine any justified capacity increase
- **AND** the96 native handles for32 voice lanes SHALL NOT be treated as a216-pad pool

### Requirement: Measured warm reuse and full lifecycle acceptance
The system SHALL support performance/RAM claims only with actual source/runtime-
bound paired measurements and causal path checks for the implemented feature.

#### Scenario: Warm trigger has no cold work
- **WHEN** valid resident PCM receives repeated unchanged starts with a saturated cold lane
- **THEN** genuine native ACK and guarded launch use the retained handles
- **AND** instrumentation shows no new disk/decode/alignment/preparation work
- **AND** measured startup or latency claims use their actual separate observations
- **AND** unchanged starts SHALL preserve delivered ACK/guarded reuse without a cold dependency

#### Scenario: Complete measurement scope is retained
- **WHEN** feature performance/RAM acceptance is evaluated
- **THEN** actual repeated runs SHALL include216 unique/shared content, preload off/on, cold/new-process/in-process warm, first live switch, ready triggers, single edits, disk/integrity and final-owner lifecycle
- **AND** actual logs/negative runs/process scope and human/device gates SHALL remain explicit


### Requirement: True last use governs shared material retirement
The system SHALL acquire all new assignment/action/version references before retiring old references and SHALL reclaim managed material only after every all-bank assignment, voice/old voice, reader, job/subscriber, queued action, history/FIFO and native unload ACK owner has ended.

#### Scenario: Retirement preserves authority and exact ownership
- **WHEN** a slot is removed or native ownership is irreversibly claimed without confirmation
- **THEN** slot removal revokes playback authority without giving queued old actions authority over replacements
- **AND** unconfirmed claims retain old/new pins and fence conflicting input
- **AND** cleanup resolves verified contained owned file identities off-thread and preserves external, private and unknown files

#### Scenario: Swap and overwrite do not create a zero-owner gap
- **WHEN** contents sharing a material move/swap or a stopped copy overwrites a target
- **THEN** new refs/reservations are secured before old refs retire
- **AND** shared material never becomes temporarily eligible for deletion

#### Scenario: Assignment count zero still has a real reader
- **WHEN** the final assignment disappears while an old voice/job/action/native ACK is pending
- **THEN** bytes remain pinned until actual terminal/read end
- **AND** cleanup runs only after that last use, without changing external user files

#### Scenario: Copies share input without sharing DSP
- **WHEN** compatible copies request the same resident interval
- **THEN** verified immutable input backing is shared once in accounting
- **AND** each copy retains independent DSP/voice/settings and varied ranges remain valid separate views
- **AND** Copy/transposition creates no original/PCM/stem file duplicates or complete pitch PCM

#### Scenario: Paired WAV and PCM owners end together
- **WHEN** the final assignment ends while any paired descriptor, component or instrumental reader, voice, history, queued action or job still retains either area
- **THEN** both immutable WAV and PCM areas SHALL remain protected
- **AND** all new pair owners SHALL be acquired before old selection or readers retire
- **AND** only the actual final reader end SHALL permit existing off-thread exact-leaf retirement and bounded empty-container cleanup
- **AND** replacement/content-change outcomes SHALL preserve the affected files and settle visibly while retaining inventory protection until terminal

#### Scenario: Paired restart obtains fresh authority
- **WHEN** a new process reconciles a selected pair and current journal/config
- **THEN** typed roots, file identities, full five-WAV/five-PCM integrity and the common descriptor SHALL be reverified before pair eligibility
- **AND** current content UUIDs, key intent and newer performer edits SHALL survive
- **AND** native source/timing/window captures and callback ACKs SHALL be freshly acquired, never restored from durable pair metadata

### Requirement: E11-01 Live KEYLOCK mode changes preserve finite playback
The system SHALL apply resolved global-default KEYLOCK changes only to inheriting loaded pads and explicit per-pad OFF-to-ON and ON-to-OFF requests only to their addressed loaded pad during stopped, playing and paused states through current preparation, each voice's own callback mode/window ACK and actual Native processing, preserving source position, loop geometry and continuing audio without stop or retrigger.

#### Scenario: Remaining contract constraints hold for every operation
- **WHEN** the system exercises this requirement
- **THEN** An already warmed unique Native handle MAY initially render wet from the actual current feed before a later source-specific candidate is adopted. Window/mode ACK, actual wet processing and later exact source-candidate adoption SHALL remain distinct; every later candidate SHALL still satisfy the exact output-frame/SourcePlayback/read-plan match and own permits. Actual nonneutral-rate Native wet output SHALL establish successful ON; queued intent, button color, a dry fallback or another pad's ACK SHALL NOT establish success. Confirmed Native/effective, pending and failed status SHALL follow current engine feedback and reject stale completion separately from requested-origin colors; rejection or observed audio SHALL NOT rewrite explicit ON/OFF/inherit intent.

#### Scenario: OFF-started finite NormalLoop enables live processing
- **GIVEN** a production k=0 finite NormalLoop started with KEYLOCK OFF at a nonunity SPEED or BPM-lock rate
- **AND** the pad inherits the disabled global default or receives an addressed explicit ON request
- **WHEN** a resolved global or pad ON request is published for that pad
- **THEN** matching current-window preparation and native ACK SHALL precede effective ON
- **AND** actual subsequent Native/FIFO/filter output SHALL match independent complete-PCM and raw-Native chronology beyond the first callback without a source-position jump

#### Scenario: Both directions and old voices retain authority
- **WHEN** repeated ON/OFF commands cross pause, rate smoothing, FullMix/selected stems, queued work or bank replacement
- **THEN** the actual current or old voice SHALL keep its own source/timing/window/selection/history permits and readers until safe adoption
- **AND** stale, failed or superseded work SHALL leave effective audio intact with truthful pending/error state
- **AND** supported fixed NormalLoop correction SHALL NOT open unproved seek/intro/tail, nonzero-pitch, P5a or whole-P4b guards

#### Scenario: First wet readiness and failures retain truthful audio ownership
- **WHEN** an ON request has its own callback ACK but has not emitted successful wet output
- **THEN** playing nonneutral-rate audio SHALL retain its previous dry feed and continuous source/EQ chronology while the actual Native handle warms
- **AND** the first wet handover SHALL follow the same bounded Native block boundary across callback partitions
- **AND** worker, reserve or processing failure before that handover SHALL preserve dry output and report its own effective OFF and error
- **AND** stopped or paused ON MAY report armed readiness after reserving an actual usable handle, while retaining the first-live-wet failure guard for subsequent playback
- **AND** an explicit owned-ON launch or retrigger SHALL establish first wet readiness for its new voice; a previous voice's wet receipt SHALL NOT certify that voice
- **AND** actual waiting feedback SHALL remain visible on global and pad controls after armed readiness even when no controller transaction remains
- **AND** a failure after actual wet confirmation SHALL report the actual effective ON with error even if the UI missed the ready snapshot
- **AND** terminal own-request feedback SHALL reconcile the actual audio baseline without treating the error as success or undoing already adopted geometry through project fields alone

#### Scenario: A nonneutral rate target precedes ON in the same callback batch
- **GIVEN** a dry voice has an applied unity ratio and a nonneutral SPEED or BPM target is drained before ON
- **WHEN** its own ON transaction is admitted before the next render updates SourcePlayback
- **THEN** both the authoritative rate target and applied ratio SHALL determine first-live-wet readiness without mutating source position during admission
- **AND** the actual smoothed Native/FIFO/EQ trajectory SHALL retain the first-wet dry guard and own prewet failure baseline

### Requirement: E11-02 Explicit global broadcast and pad override
The system SHALL retain a global boolean KEYLOCK default and nullable track-bound pad overrides.
Global actions SHALL publish only to inherited loaded pads, retaining explicit ON/OFF overrides.
Local actions SHALL affect only the addressed pad. Clearing its override SHALL resolve the current
default through the existing per-pad transaction. `null` SHALL mean inheritance without another
mutable flag. Native feedback SHALL NOT rewrite requested origin.

#### Scenario: New source inherits only after matching successful completion
- **WHEN** a matching new-source load completes successfully
- **THEN** its prior track override SHALL clear and the latest global default SHALL resolve
- **AND** admission failure or stale completion SHALL retain prior origin and confirmed Native state

#### Scenario: Remaining contract constraints hold for every operation
- **WHEN** the system exercises this requirement
- **THEN** Each targeted inherited or addressed pad SHALL settle through the same existing per-pad pending/retry/unconfirmed transaction. A target failure SHALL NOT abort remaining targets or roll back successful targets; partial/mixed/unconfirmed/error Native status SHALL remain visible separately from requested intent. Stale predecessor feedback SHALL NOT rewind superseding intent or new content. Explicit overrides and requested-origin colors SHALL NOT be replaced by observed audio or failure baselines.

#### Scenario: Global ON then one pad OFF then global ON
- **GIVEN** loaded pads inherit the initial disabled global default
- **WHEN** global ON is followed by explicit pad4 OFF and another global ON action
- **THEN** only pad4 SHALL change on its local action and it SHALL remain explicitly OFF after the final global action
- **AND** inherited pads SHALL continue to follow the global default without acquiring explicit overrides
- **AND** stopped, playing and paused pads SHALL expose their independently ACKed effective/pending/error status

#### Scenario: Global OFF then one pad ON with out-of-order results
- **WHEN** global OFF, explicit pad ON and later global changes have queued or stale results
- **THEN** that pad SHALL retain explicit ON while inherited pads follow the latest default
- **AND** current command ownership SHALL determine each confirmed Native result and unrelated pads SHALL retain their actual state
- **AND** neither a green global intent nor a brief local blink SHALL substitute for actual successful Native output

#### Scenario: Clearing a pad override restores inheritance without changing the default
- **GIVEN** selected loaded pad4 has explicit OFF and the global default is ON
- **WHEN** the performer activates its adjacent GLOBAL inherit action
- **THEN** pad4 SHALL retain `null` override and request ON through its own existing transaction
- **AND** the global default and every other pad's override SHALL remain unchanged
- **AND** failure SHALL retain inherited origin and the previous confirmed Native state with visible error

### Requirement: E11-03 Complete source extent is independent of scratch admission
The system SHALL prepare and verify complete FullMix, all five aligned stem WAV/PCM derivatives and mono integrity through bounded streaming blocks and disk-backed immutable generations without an artificial duration or total-PCM 1-GiB limit.

#### Scenario: Remaining contract constraints hold for every operation
- **WHEN** the system exercises this requirement
- **THEN** A track longer than ten minutes or a multi-GB complete PCM extent SHALL remain admissible when actual storage and selected live residency budgets suffice. Existing per-job scratch, worker/queue, native cold/analysis and user-selected loop/ALL residency bounds SHALL remain explicit, separately accounted and fail visibly on real resource exhaustion. Demucs inference/activations/model memory SHALL remain separately measured and SHALL NOT be claimed bounded by streaming postprocessing.

#### Scenario: Long complete conversion and new-process warm validation
- **WHEN** a source longer than ten minutes with complete PCM above 1 GiB is prepared and reopened
- **THEN** complete frame counts, full EOF digests, finite PCM values, exact PCM16 normalization, common signed loaded-frame alignment offset and zero padding SHALL match the independent existing conversion oracle
- **AND** the first-onset four-second analysis with at most plus/minus 0.25-second shift SHALL retain existing semantics across arbitrary chunk boundaries
- **AND** fresh warm verification SHALL stream the full source/five-WAV/five-PCM/mono integrity from retained immutable readers without complete arrays
- **AND** measured scratch and actual process peak SHALL be reported separately at multiple source lengths, while live RAM SHALL follow requested loop/ALL demand and the user budget

#### Scenario: Cancel and real storage failure preserve prior data
- **WHEN** long preparation is cancelled, truncated, changed at final EOF or encounters disk/RAM failure
- **THEN** only exact attempt-owned staging SHALL retire after actual readers return and incomplete pairs SHALL remain unavailable
- **AND** the prior source/set/config SHALL remain usable with a visible error
- **AND** raising a total-size cap or allocating full multi-GB buffers SHALL NOT satisfy this requirement

#### Scenario: Automatic analysis has independent admission and outcome
- **WHEN** ordinary import requests automatic analysis with a finite selected loop
- **THEN** complete disk preparation and the selected live view SHALL NOT demand whole-source residency for that analysis
- **AND** automatic analysis SHALL still execute when its separately accounted analysis budget admits the complete input
- **AND** a budget or analyzer failure SHALL be delivered visibly with the matching successful source ACK without inventing analysis metadata or failing that source's selected live admission
- **AND** cancellation, superseded content or a newer timing intent SHALL NOT publish a stale analysis result or error

#### Scenario: Large WAV formats remain productive across consumers
- **WHEN** a complete PCM16 WAV exceeds RIFF's unsigned 32-bit chunk extent
- **THEN** the productive Python model-output reader, artifact writer and target admission, Native stem parser and source decoder SHALL support an exact 64-bit RF64 extent and actual EOF
- **AND** decoder provenance SHALL retain the actual RF64 format and decoder policy rather than claim a RIFF header
- **AND** existing small RIFF PCM16 conversion and writer results SHALL retain their independent byte/sample oracle
