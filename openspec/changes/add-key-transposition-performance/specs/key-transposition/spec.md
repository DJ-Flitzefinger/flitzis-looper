## ADDED Requirements

### Requirement: Source metadata and two numeric shifts remain separate
The system SHALL represent detected or corrected source key, numeric audible base_shift and numeric extra_shift as separate per-content state and SHALL derive base and result labels without conflating them.

#### Scenario: Base and additional zero use the prepared tuning
- **GIVEN** E minor source and base C minor with base_shift=-4
- **WHEN** extra_shift is0,+2 or+12
- **THEN** nominal result is C minor,D minor or C minor one octave higher respectively
- **AND** the base menu stays C minor; extra0 means neutral to base, not original file

### Requirement: Correction changes labels without changing sound
The system SHALL persist an independent source metadata correction and SHALL preserve numeric base/extra, playback position and accepted events when correction is set or removed.

Correction MAY fix major/minor metadata. It SHALL NOT retune/retrigger or recompute
already chosen numerical shifts. Remove correction, reset base0 and reset extra0
SHALL be three independent actions.

#### Scenario: Correct a wrong source label while playing
- **WHEN** a source correction is set or consciously removed on playing A
- **THEN** source/base/result labels update consistently without sound/cursor/retrigger change
- **AND** copies' corrections and both numeric shifts remain independent

### Requirement: Analysis epochs preserve newer correction intent
The system SHALL remove only a preexisting correction at successful deliberate new analysis admission and SHALL fence late analysis metadata by source/lifetime/analysis and correction epochs.

#### Scenario: New correction after analysis admission wins
- **GIVEN** old correction C1 and successful new analysis epochE
- **WHEN** C1 is removed at admission, C2 is set afterward and E's result arrives late
- **THEN** C2 remains authoritative and saved numeric shifts stay unchanged

#### Scenario: Failed analysis admission or project reopen
- **WHEN** analysis cannot be admitted or the same verified project reopens
- **THEN** existing correction and shifts remain; reopen is not a new analysis
- **AND** late results for removed/reassigned lifetimes cannot affect a replacement

#### Scenario: Real admission and metadata-only version publication
- **GIVEN** the current content instance and native waveform source generation, digest and shape
- **WHEN** deliberate analysis returns a real integer request ID after successful native admission
- **THEN** analysis epoch advances and only its preexisting correction is removed
- **AND** the result may update a source key version only while that captured content, native source, request and analysis epoch still match
- **AND** missing or boolean request IDs and duplicated, older or equal-byte replacement results grant no new key metadata authority
- **AND** a later correction is retained, independently of whether a newer timing edit has made the analysis timing projection stale

### Requirement: Durable key policy remains neutral before audible activation
The system SHALL keep persisted key correction, base, extra and retrigger policy separate from native audio until the required pitch application gates are implemented and accepted.

#### Scenario: Neutral policy edit during ordinary playback
- **WHEN** correction is changed or removed, absolute base intent is chosen, either numeric shift is reset or extra/retrigger intent changes in K-META
- **THEN** no native pitch, trigger, stop, cursor, Key Lock or timing command is issued
- **AND** immutable source versions and independent per-content settings persist without restoring live tokens

### Requirement: Absolute base key selection has deterministic octave and mode
The system SHALL set base_shift from the corrected known source root to a same-mode target using d=(target-source) mod12 and d>6 then d-=12, with a fixed +6 tie and no accumulation.

#### Scenario: Repeated absolute key selection is stable
- **WHEN** any of12 source roots selects any of12 same-mode targets repeatedly
- **THEN** base remains the same integer -5..+6 and extra remains unchanged
- **AND** a tritone always selects+6 without octave switching

#### Scenario: Unknown source or unreachable major minor conversion
- **WHEN** GUI or MIDI requests an absolute target without known/corrected source or with different mode
- **THEN** admission explicitly rejects that target without guessing or audible mode conversion
- **AND** relative semitone choices remain usable with unknown source metadata

### Requirement: Additional transposition sets every supported signed value
The system SHALL offer all37 desired fixed integer extra_shift values -18..+18 including0 without scale filtering or accumulation and SHALL validate the whole resulting base+extra against one shared supported domain.

#### Scenario: Repeated semitone and independent resets
- **WHEN** +2 is selected repeatedly or extra is reset0
- **THEN** extra remains+2 or becomes0 respectively without changing base or source correction
- **AND** base reset0 independently preserves extra and correction

### Requirement: One pitch route preserves tempo and explicit lock behavior
The system SHALL use one integrated pitch route with explicit lock behavior and proven support.

#### Scenario: One route retains the complete pitch and readiness contract
- **WHEN** transposition is prepared, admitted or applied
- **THEN** the following complete route, domain and failure contracts SHALL apply:

The system SHALL apply k=base_shift+extra_shift through one integrated pitch route while preserving source time, tempo, M/B/S/phi, loop/launch and all other pads.

The desired total SHALL cover -23..+24 plus actual r(n). Equal-rate varispeed route
SHALL use h=2^(k/12), lockON p=h/r and lockOFF p=h with existing additional speed
coupling r*h. Quality/RT/readiness/latency/finite-history/unity proof SHALL precede
support claims; current clamp/dry/finite API checks SHALL NOT certify support.
Concrete demonstrated limitations SHALL be explained and consistently rejected
by UI/MIDI/storage/audio instead of hidden clamping or tempo change. Transposition
SHALL NOT create complete source PCM or two serial pitch shifters.

#### Scenario: Extreme total shift under variable rate
- **WHEN** total k=-23 or+24 and actual accepted r(n) extrema are evaluated
- **THEN** B5 proves the combined range or records a concrete unsupported limit
- **AND** no partial retune, time change or accepted-looking clamp conceals failure

#### Scenario: Native pitch unity differs from source rate unity
- **WHEN** r=1 with k nonzero or h/r=1 with r nonunit
- **THEN** prepared processing/transition preserves intended pitch and matched output history/latency
- **AND** rate-only/dry bypass cannot silently omit transposition or reset voices

### Requirement: A live Key Lock change owns a prospective numeric tuple
For a live nonzero applied transposition, the system SHALL retain the previously
effective Key Lock mode and its productive Native/FIFO history until the original
mode request can atomically adopt source-specific prepared owners for every
participating voice at one exact output frame E. Preparation SHALL use each
voice's actual applied numeric pitch intent with the requested Key Lock mode,
and SHALL retain the original request, content/source generation, actual window,
accepted timing/ACK, voice generation and preparation epoch fences. An old
effective-mode candidate SHALL NOT certify the prospective tuple.

The system SHALL use its existing bounded source-preparation/recycle lanes and
render boundaries. Callback work SHALL NOT construct Native state, allocate
owners, wait, perform I/O, or partially change a pad when any participant fails
preflight. Preparation failure or unavailable history SHALL reject only that
request visibly, preserve the old effective mode, and leave worker retirement
and later requests usable. Existing strong Native history permits SHALL remain
required at actual adoption. A constant endpoint SHALL NOT stand in for missing
variable source history. The bounded history contract below does not certify
acoustic, device or full B5 acceptance.

Prospective mode preparation SHALL reuse the owning voice's genuine bounded
source-read chronology before F and frozen F-to-original-E projection under the
same declared O/A replay contract as a moving SET. Each replay span SHALL use the
actual applied numeric pitch with the requested mode and its actual source rate.
A settled endpoint SHALL NOT erase earlier rate, Intro/Tail or selection history.
The same held-selection chronology contract below SHALL apply to prospective
mode and nonstarting SET preparation.

Each source lane SHALL acquire already allocated reusable snapshot storage before
capture; exhausted storage SHALL cause local refusal without callback allocation
or owner destruction. The worker SHALL clear and return snapshot owners through
bounded recycling on successful, stale and unavailable-history paths. A small
guard captured from the genuine snapshot SHALL retain its SOURCE-trajectory and
voice-generation fences with the candidate through adoption even after the large
snapshot has been recycled. That guard SHALL only narrow a fresh strong Native
permit, never replace source/window/timing authority. Its later invalidation
SHALL NOT by itself revoke already productive Native history.

For a participant paused at F, prospective mode preparation SHALL retain the
same original global E while projecting zero additional active source frames
between F and E. The requested tuple SHALL replay only the genuine retained
active chronology to that frozen endpoint. The callback SHALL retain its own
prepared candidate until the all-participant claim at E without consuming its
Native input/output FIFOs, source smoothing clock or filter history during pause.
Pause or Resume before E SHALL fence the original pending request; Resume after
successful adoption SHALL retain the adopted Native/FIFO owner and continue from
the frozen source position. Armed readiness while paused SHALL remain distinct
from that voice's first actual successful Native output after Resume.

#### Scenario: Complete source changes Lock in either direction with nonzero pitch
- **GIVEN** a playing complete source with positive or negative numeric transposition and actual r=1 or a nonunit rate
- **WHEN** its current own mode request changes Key Lock in either direction
- **THEN** real source preparation targets the prospective tuple while uninterrupted old-mode PCM remains effective before E
- **AND** all participating prepared Native/FIFO owners and the same own mode request adopt together at E without a source restart, numeric pitch change or tempo change
- **AND** the first resulting PCM uses the prospective tuple and its matched prepared history

#### Scenario: Finite window storage precedes the same own mode acknowledgement
- **GIVEN** a finite source whose nonzero live transposition needs a new Key Lock tuple
- **WHEN** the original mode-only resident transaction claims actual storage
- **THEN** its original ticket SHALL remain adopting and its resident marker SHALL remain owned while the old mode is audible
- **AND** preparation SHALL capture a fresh strong permit from the actual installed window rather than fabricate authority for a future window
- **AND** ordinary or GLOBAL starts depending on that ticket SHALL remain pending until that same request's complete prospective adoption at E
- **AND** only complete adoption may accept the original ticket and publish its own mode acknowledgement

#### Scenario: Stale or unavailable prospective work preserves old effective audio
- **WHEN** Stop, source replacement, window/timing/ACK change, an earlier pitch effect, a newer mode request or a changed participant invalidates prospective work
- **THEN** no participant SHALL consume that candidate or change the shared effective mode
- **AND** the original resident marker and owner SHALL settle exactly once through bounded off-thread retirement
- **AND** an unavailable real source-history trajectory SHALL remain a request-local visible refusal without terminating the preparation worker or certifying unsupported history

#### Scenario: Prospective mode replays actual variable history at its original E
- **GIVEN** a live nonzero applied pitch with genuine off-block rate edits and an Intro or Tail passage inside its required pre-F history
- **WHEN** an ON or OFF request needs a Native preparation bound greater than E minus F
- **THEN** the existing worker SHALL replay the genuine retained past and original frozen future for the requested tuple
- **AND** every adopted PCM sample, source phase and FIFO count SHALL match an independent scalar/direct-Native oracle with the same declared A reset
- **AND** a backwards-constant reference SHALL disagree while old productive mode and owners remain effective until the unchanged original E

#### Scenario: Recycled snapshot leaves strict adoption fences intact
- **GIVEN** prepared prospective work whose large snapshot has already returned to its source lane
- **WHEN** a source control changes and returns to its former value, or a captured voice lifetime ends before E
- **THEN** the retained small guard SHALL refuse the whole tuple at adoption without changing old productive mode or partially swapping another participant
- **AND** a later original request on the same worker SHALL capture retained genuine history and remain usable
- **AND** a valid history guard SHALL NOT authorize expired or foreign Native source/window/timing ownership

#### Scenario: History storage saturation preserves every original owner
- **GIVEN** a source lane whose reusable history owner is in flight or awaiting bounded recycling
- **WHEN** another prospective mode request arrives or unavailable history returns
- **THEN** no callback SHALL allocate replacement snapshot storage, overwrite an owning request or destroy a filled snapshot
- **AND** bounded worker retirement SHALL return that same reusable owner and leave a later request functional

#### Scenario: A paused participant changes nonzero-pitch mode without invented reads
- **GIVEN** a publicly started voice with positive or negative applied pitch is paused with its own source position, active clock and filter history retained
- **WHEN** its addressed ON or OFF request is prepared while global output advances from F to the original E
- **THEN** the actual frozen endpoint SHALL be claimed at E through the same own mode/window transaction, with no START, STOP, retrigger or source-frame advance
- **AND** its prepared Native/FIFO candidate SHALL survive intervening paused callbacks while other running voices continue independently
- **AND** every sample after Resume, source phase and FIFO count SHALL match an independent direct-Native reference with the same declared A reset and zero paused active-frame projection
- **AND** successful paused ON SHALL report armed readiness while first live wet success or failure remains owned by that voice after Resume

#### Scenario: Paused request cancellation and mixed global targets keep independent owners
- **GIVEN** a global request addresses inherited running and paused pads while explicit ON and OFF peers retain their overrides
- **WHEN** one original paused request is superseded, stopped or resumed before its original E, or bounded retirement is temporarily unavailable
- **THEN** that target SHALL preserve its previous effective tuple and original request owner until its own safe completion or visible refusal
- **AND** another inherited target SHALL remain independently eligible for its own E and acknowledgement
- **AND** a later paused request on the same worker SHALL remain usable without borrowing another target's receipt or refreshing the refused E

#### Scenario: A prepared ordinary birth owns its first live wet acknowledgement
- **GIVEN** a tracked zero-pitch KEY LOCK ON voice previously emitted confirmed wet output and was stopped
- **WHEN** a new addressed or global ordinary start adopts its own prepared Native/FIFO at its original E
- **THEN** the new voice SHALL rearm the existing first-live-wet guard independently of the predecessor's completed readiness
- **AND** preparation alone SHALL NOT report successful live wet output for that new voice
- **AND** failure before its first actual output SHALL retain the declared dry source trajectory and report its own effective OFF/error without rewriting inherited or explicit intent
- **AND** successful first live output SHALL retain the exact adopted Native/FIFO owner, original source phase and acknowledgement
- **AND** this birth guard SHALL NOT adopt a later requested mode or borrow that later mode's completion

#### Scenario: Saturated retirement retains the original finite mode owner
- **GIVEN** an adopting resident mode transaction and zero available retirement queue or backlog slots
- **WHEN** its exact E, cancellation or a newer mode request is observed
- **THEN** the system SHALL retain the original transaction, PCM/stem pins and pending state without consuming, overwriting, forgetting or destroying that owner on the callback
- **AND** only available retirement capacity may settle that owner exactly once
- **AND** if E has already passed, later capacity SHALL permit refusal and retirement, never late mode adoption

#### Scenario: Refused mode reconciles its own already adopted physical window
- **GIVEN** a mode-only transaction whose real storage claim installed a different finite window or context before E
- **WHEN** prospective mode adoption is refused by Stop, timing, unavailable work or missed E
- **THEN** its original mode ticket SHALL remain rejected and dependent starts SHALL remain unable to launch
- **AND** control reconciliation SHALL preserve the actual claimed storage using only that original transaction's separate callback storage observation and current native content/source-generation/unique-window fences
- **AND** a fresh capture and later own mode request SHALL use that real window and remain functional without fabricating an accepted predecessor or replacement receipt

### Requirement: A moving SET retains the genuine bounded source-feed chronology
A nonstarting SET that needs source history before its capture frame F SHALL use
the owning voice's actual successful source reads, including dry reads, followed
by the frozen deterministic projection from F to its original execution frame E.
Each retained span SHALL preserve the pre-read source clock, physical read plan,
rate epoch and source/selection context. A settled endpoint rate SHALL NOT replace
earlier rate changes, smoothing, Intro/Tail reads or loop-domain changes. Pause
SHALL consume no active-feed frames; refused reads SHALL create an explicit gap.
Start, restart, seek and Stop SHALL fence the retained chronology.

For a nonstarting SET captured while its voice is paused at F, the original
global execution frame E SHALL remain unchanged. Its active-feed endpoint SHALL
remain at F and its frozen F-to-E future SHALL contain zero active source reads.
The existing action-owned snapshot and strong action permit SHALL retain the
genuine required past, original receipt and voice owner through adoption at E.
Preparation and adoption while paused SHALL NOT consume source smoothing, filter
history or Native FIFOs. Resume after adoption SHALL continue with that exact
prepared Native/FIFO owner from the frozen active endpoint.

The callback SHALL record and snapshot only bounded scalar spans in storage
allocated before realtime use. An accepted action SHALL retain its own immutable
snapshot, voice generation, continuity and source-trajectory fences; its worker
SHALL NOT read mutable callback history. Historical domain/context markers SHALL
describe data only, never replace fresh strong Native adoption authority. Every
historical read SHALL remain backed by actually retained PCM/stem pins and its
physical coverage; missing contexts, gaps, retention overflow and stale owners
SHALL produce request-local refusal while leaving productive audio and the same
worker usable.

Source/rate/domain/selection changes and Pause/Resume after F SHALL invalidate the
captured future even if controls return to the same endpoint before E. This fence
SHALL preserve genuine retained past for a later action. Source-neutral DSP
preparation for a later prospective Key Lock request SHALL NOT invalidate an
earlier SET's independent source trajectory or original execution frame.

Historical stem descriptors SHALL retain copied complete-PCM identity values and
their original publication epoch owner/value in addition to allocation markers;
reused addresses SHALL NOT substitute another PCM or publication generation.

For a settled original E, replay SHALL support a preceding selection ramp whose
required stem readers belong to one actually held committed StemSet, including
a ramp still active at capture F that deterministically completes by E. The
request SHALL retain that physical history owner even when E uses FullMix and
requires no endpoint stem reader. One pin MAY fulfill both reader roles only
when each required role independently matches that pin's complete PCM identity,
source, physical backings and original publication generation.

The worker SHALL resolve the current and outgoing roles required by each sliced
read plan against real retained owners before validating coverage or reading.
An empty selected stem mask SHALL retain its owner requirement. Only roles no
longer required by that actual sliced plan MAY be omitted; an unsuccessful
lookup SHALL NOT erase a required role. Frozen F-to-E projection SHALL preserve
the actual reader roles at F and their deterministic retirement at ramp completion.
Endpoint/adoption comparisons SHALL use E's required readers independently of
the retained history pin, while retaining all physical identity, timing, window
and publication checks for readers that remain required. Historical pins SHALL
retire through the existing worker-owned bounded retirement path.

An already productive Native owner SHALL survive a storage-only WindowACK with
the same actual source, accepted timing and committed publication generation
when its current physical coverage remains valid. Its stem publication's old
capture-window acknowledgement SHALL NOT replace the existing productive source
and timing authority check. Fresh preparation and adoption SHALL retain their
strict current-window gates; a changed actual held publication identity or a
source/timing/voice revocation through the owning Native authority SHALL remain
invalidation reasons.
A fresh Native request on the refreshed physical view MAY reuse the already
admitted stem publication stamp; its own fresh Native permit SHALL prove the
current source, timing, window and acknowledgement. Initial stem publication
and admission SHALL retain their existing strict publication-window checks.
For already admitted Mixer-owned data, copied publication owner/expected value
SHALL identify its original data provenance. Mutable operation/receipt status and
intent counters SHALL NOT replace that data identity or the owning Native
authority. Withdrawing or superseding an old selection receipt SHALL NOT revoke
unchanged admitted PCM retained by a valid ordinary start or productive voice.
An independent selection operation reusing an already admitted physical set
SHALL preserve that set's original data-publication stamp. Its new receipt SHALL
retain its own separate operation/admission authority rather than restamping the
unchanged data and invalidating productive Native history.
Reader reconstruction from a shared cache without an admitted Mixer StemSet
SHALL take the new operation's admitted data-publication stamp; an unbound cache
placeholder SHALL NOT be promoted to admitted provenance by retaining it.

This bounded contract SHALL NOT admit a still-active selection transition at E,
unheld historical readers or a second distinct StemSet without its own physical
owner proof. Those cases SHALL remain request-local refusal with the old
productive tuple retained and a later valid request on the same worker usable.
Existing constant endpoint priming MAY retain deliberate pre-origin start
semantics only after its whole required actual interval proves the same
rate/read/selection contract without a gap. A settled source clock alone SHALL
NOT certify earlier stem selections, masks or outgoing readers.

Fresh Native replay SHALL declare its active-feed origin O and reset origin A.
For the bounded variable suffix contract, A SHALL lie on O's Native input-block lattice
and precede the action's active-feed endpoint by at least the configured
preparation bound H. A paused endpoint remains at active F despite global E.
Alignment may add
up to one block minus one frames; the complete interval SHALL fit the explicit
retention budget, without clipping H. Native configuration and bound queries
SHALL run off realtime, with reset-dependent bounds queried after configuring the
actual encountered pitch. Replay SHALL preserve pitch changes at the spans that
complete Native input blocks, reset only before replay, and restore the exact E
source clock/read plan and adapter phase. Expanded-pitch cold initialization
SHALL use the first real completed block's pitch profile.

An independent A-reset PCM oracle SHALL certify only that declared finite replay
contract. A bounded suffix or matching FIFO counts SHALL NOT certify equality to
Native processing continuously from O or acoustic transition quality. Separate
continuous-reference diagnostics SHALL retain their actual results; the original
acoustic, performer-deadline and full B5/B6 acceptance gates SHALL remain required.

#### Scenario: Off-grid rate edits precede an expanded moving SET
- **GIVEN** a voice with actual rate edits away from Native block boundaries and an Intro or Tail passage into Normal inside its retained pre-F history
- **WHEN** a nonstarting expanded-pitch SET needs H greater than E minus F
- **THEN** its worker SHALL replay the genuine required past and original F-to-E projection with the prospective pitch/Lock tuple
- **AND** an independent scalar chronology and Raw Native with the declared reset origin SHALL agree on every adopted PCM sample, source phase and FIFO count
- **AND** a backwards-constant reference SHALL disagree, while a separately evaluated continuous-from-O reference SHALL retain its actual positive or negative result

#### Scenario: History refusal does not damage the next original action
- **WHEN** a required interval crosses a read gap, evicted history, unheld old stem context or a changed voice/source-trajectory fence
- **THEN** only that original action SHALL be visibly refused without changing its original T or adopting a replacement capture
- **AND** old productive audio SHALL remain owned and a later valid action on the same worker SHALL remain usable

#### Scenario: Paused SET replays genuine variable past and retains its original E
- **GIVEN** a publicly playing source whose retained active past contains off-grid rate smoothing and an Intro or Tail passage into Normal is paused at F
- **WHEN** a positive or negative nonstarting SET is prepared under either Key Lock mode for its original global E
- **THEN** its worker SHALL replay the actual required active past ending at F with zero active future, without inventing reads during the global wait
- **AND** adoption at E SHALL preserve the original action sequence, timestamp, T/E, content and voice generation without START, seek or source advance
- **AND** every resumed PCM sample, source phase and FIFO count SHALL match an independent direct-Native A-reset reference while retaining the same prepared owner and filter history

#### Scenario: A paused SET cannot borrow silent waiting time or a restored control state
- **GIVEN** an original paused SET with its own immutable history capture
- **WHEN** its genuine required past is insufficient, or Resume/Pause or source-rate changes after F restore the same endpoint before E
- **THEN** that original SET SHALL refuse locally without changing T/E or consuming the old productive owner
- **AND** silent global frames SHALL NOT supply missing active history, while a later original request with valid retained history SHALL remain usable on the same worker

#### Scenario: Later mode preparation preserves an earlier independent SET
- **GIVEN** a moving SET prepared for its original E and an unchanged source trajectory
- **WHEN** a later prospective Key Lock request prepares a tuple using the still-effective old numeric pitch
- **THEN** the earlier SET SHALL apply its own captured tuple at its original E
- **AND** its effect SHALL preserve the later pending mode request's original receipt and status ownership
- **AND** the later mode tuple SHALL refuse through its original captured-pitch mismatch without partially adopting another mode or invalidating that SET

#### Scenario: A restored endpoint cannot hide a changed future
- **GIVEN** a moving SET with a captured F-to-E source projection
- **WHEN** the source rate or read domain changes after F and is restored before E
- **THEN** that SET SHALL refuse through its source-trajectory fence even when the endpoint matches
- **AND** retained genuine reads SHALL remain available to a later original action

#### Scenario: A settled rate does not erase an earlier selection or failed read
- **GIVEN** a constant-rate source whose required SET history overlaps a previous stem selection or a refused source read
- **WHEN** the current source clock supports backwards constant reconstruction
- **THEN** the system SHALL still prove the actual selection/context chronology before reusing constant endpoint preparation
- **AND** it SHALL replay the genuine held past or refuse that request locally rather than repeat the current selection across the missing history

#### Scenario: A held same-set ramp precedes the settled original endpoint
- **GIVEN** a source whose required history contains a real FullMix-to-stems, stems-to-FullMix or stem-mask ramp backed by one still-held committed StemSet
- **WHEN** a nonstarting SET or prospective mode request captures that history, with any active ramp at F completing by the original E
- **THEN** replay SHALL preserve the actual selection weights and current/outgoing reader roles through the required past and frozen future
- **AND** every adopted PCM sample, source phase and FIFO count SHALL match an independent direct-Native A-reset reference without changing original E or replacing the adopted owner
- **AND** a FullMix E SHALL retain its required historical pin without creating an unused endpoint-reader mismatch

#### Scenario: Slicing across ramp completion preserves only actual reader dependencies
- **GIVEN** a retained or projected span starts during a selection ramp and extends beyond its completion
- **WHEN** replay slices before, exactly at or after that completion
- **THEN** every still-required role SHALL match its genuine held pin and original publication generation, including an empty selected mask
- **AND** a role no longer read after completion SHALL NOT create a fictitious dependency on an obsolete outgoing owner
- **AND** a missing, different or stale required owner SHALL refuse that original request without borrowing a marker or another participant's pin

#### Scenario: A storage-only WindowACK retains productive stem Native ownership
- **GIVEN** an adopted wet stem Native owner with a real source-bound stem publication
- **WHEN** actual resident adoption changes only its storage window and preserves the source, accepted timing, publication generation and required coverage
- **THEN** productive matching SHALL retain its exact Native/FIFO owner without automatic replacement or readiness failure
- **AND** a new preparation or adoption SHALL still require its own fresh current-window authority, while a changed held publication identity or an owning Native source/timing revocation SHALL still refuse

#### Scenario: Ordinary Play withdraws a stopped selection receipt while retaining admitted stems
- **GIVEN** an actual stopped committed ALL selection has admitted its physical StemSet through its original receipt
- **WHEN** an ordinary Play obtains its own resident ACK and withdraws that earlier selection observation while retaining the same admitted PCM
- **THEN** its own fresh Native request SHALL prepare and adopt those exact stem readers at its original T/E without reviving or borrowing the earlier receipt
- **AND** unchanged copied publication provenance SHALL remain data evidence even when the earlier operation counter advances, while changed physical/publication identities and stale owning Native authority SHALL still refuse
- **AND** a subsequent explicit ALL confirmation reusing the same admitted set SHALL keep its original data-publication stamp and existing Native history while reaching its own effective receipt
- **AND** shared-cache reconstruction without such an admitted set SHALL instead obtain the new operation's genuine admitted publication stamp
