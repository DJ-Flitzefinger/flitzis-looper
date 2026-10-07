# G3b2a-h native adoption, current authority and precise consumers

## Native authority

Preparation captures the engine's actual loaded source pointer, content digest,
source generation, loaded rate and current request under its publication owner.
The complete channel-mean PCM is verified and the existing lossless QM pipeline
executes off the audio callback. Caller-provided stale matching metadata cannot
replace this capture. Heavy work is followed by another current-owner check.

The ticket exposes diagnostic source/PCM/timebase and raw positions for an
explicit independent count assertion. It supplies neither a musical quarter
interpretation nor automatic acceptance. Publication constructs fresh
AcceptedConstantTiming with full evidence and uses TimingAdoptionGuard under
native ownership. New requests, load/unload, cancellation and timing edits revoke
pending authority. Explicit Manual, Tap and Legacy intent rejects automatic
acceptance; returning to Automatic cannot revive prior tickets.

Queue capacity and checked identities are validated before publication mutates
authority. Failed inputs, stale ownership, exhausted counters and full queues
preserve the previously effective state. Existing request/preparation freshness
and acknowledgement patterns are reused. Callback acceptance verifies the actual
loaded source and current permit with bounded work; evidence and sample owners
must retire outside the callback. Feedback, rather than enqueue success, declares
availability.

## Exact metadata and consumption

The accepted revision is the complete G3a canonical identity, not the raw revision
or a request/preparation counter. The realtime payload carries fixed revision
bytes, the binary64 seconds-per-quarter and independently chosen signed origin.
SourceGrid consumes the period directly through its existing from_period path.
No binary32 BPM roundtrip, source-zero rewrite, physical marker rewrite or
playhead reset participates in this publication.

Ordered and parameter-ring updates must not let an older pending legacy update
override a later accepted publication. Tests exercise this real callback path.
Successful later legacy timing edits clear accepted grid authority. New analysis
requests invalidate pending adoption without making already accepted source timing
depend on control polling.

## Current acknowledgement and native consumers

Current-pad resolution uses native loaded-source ownership, explicit Automatic
intent and fixed callback acknowledgement epochs to select retained current
metadata with complete accepted revision identity. Ticket metadata remains
historical. New preparation/requests
do not erase the previous effective record; pending replacements do not become
current until callback adoption. Successful intent/source/legacy edits revoke
current authority; returning to Automatic cannot revive a revoked record.
The metadata registry owns neither strong nor weak PCM array references. Its
non-dereferenced address/extent plus checked loaded generation/digest/rate reject
stale/recycled source addresses. Full evidence and source owners remain in the
existing off-realtime ticket/retirement paths; registry pruning also occurs off RT.

Mixer source periods prefer the accepted projection over legacy parameters.
Transport stores authoritative binary64 output seconds per quarter; bootstrap,
anchor, frame-per-beat arithmetic and quantization consume it directly. Shared
OutputClock stores the exact period bits; public BPM is derived presentation.
Legacy public BPM/speed messages preserve binary64. Master admission converts BPM
once, while BPMLOCK target is source period divided by master output period.
Start/render use the same rate resolver, including existing supported clipping;
clipping does not redefine the requested master. SourcePlayback ratio/target/ramp
are binary64 and rate changes retain fractional source epochs. LiveShifter pitch
uses its native c_double ABI through a named inverse-rate conversion. Its existing
near-unity/update threshold policy remains; no sub-threshold pitch accuracy or
audible SYNC is claimed. Physical wrap policy is unchanged. Productive history
ownership is described below.

## Python current projection and controls

CurrentPadTiming is a frozen control snapshot of the exact source period, signed
origin, actual loaded rate, complete accepted revision and full current native
source/adoption identity and provenance. Manual project intent wins; native
current_constant_timing supplies accepted authority; ordinary Legacy analysis
retains its existing period/origin projection. Historical ticket metadata is
never consulted or cached. Native pad_timing_intent is a read-only declaration,
not acknowledgement. Automatic without current acknowledgement is unavailable,
so passive restore/refresh cannot revoke it by replaying saved analysis.

Scalar projection, waveform lines/view margin/labels and loop snap/clamp/duration/
auto endpoints use period directly. One operation carries a single resolved
snapshot, including explicit unavailable None. Only physical marker boundaries
round to loaded frames. Native accepted frame extent bounds ALL, maximum auto
length and waveform navigation/query clipping. Waveform cache/view keys include
actual accepted source generation/digests/extent/rate, excluding timing-only
revision changes; this is not the future complete PCM cache/residency design.
Intentional legacy grid-offset edits revoke accepted
authority and resume existing saved analysis/grid policy, refreshing pad/master
controls; accepted evidence is not persisted as a manual override. Manual and
Tap numeric admission precede saved BPM mutation, and final intent declaration
follows legacy origin publication. Accepted origin is not automatically copied
into the legacy persisted base when manual authority replaces acceptance.

Accepted BPMLOCK resolves the anchor anew for each explicit control operation,
publishing period/speed through set_master_period. BPM is presentation. Native
direct-period and compatibility-BPM writes share a coalesced master slot with
last-admitted ordering. Speed/BPM-target/nudge reuse one anchor snapshot; explicit
speed admission rejects an unavailable Automatic anchor before changing saved
speed. Passive unavailable refresh keeps existing native loop/master state.
Accepted locked speed uses one combined parameter admission/callback record for
speed and exact master period; full queues preserve both parameters and saved
speed. Existing coalescing still respects later individual parameter writes.

Explicit validated load/unload/reset and ordinary analysis completions restore
Legacy authority before derived callbacks. Pending work keeps previous current
acceptance; retired timing_stale completions cannot perform this authority change.

Explicit accepted publication remains a control API, not the normal estimator
or an automatic acceptance orchestrator. A caller invokes the derived controller
refresh after acknowledgement to publish current physical auto-loop endpoints
and master controls. Read-only UI polling does not drive audio correction. An
acknowledged replacement alone does not automatically republish Python-owned
physical endpoints/master intent; this remains an integration boundary for the
later productive acceptance workflow, not sustained-SYNC evidence.

## MIDI current source and scheduled authority

An opaque InputRuntimePadBinding captures actual native source identity,
generation/digest/rate/extent/channels and declared authority outside realtime.
Accepted metadata is resolved from the same current record as
current_constant_timing. Python checks one frozen CurrentPadTiming against the
binding and uses that snapshot for loop endpoints and signature comparison.
Equal numerical values cannot hide full revision or authority changes;
Automatic without a matching acknowledgement is unavailable.

Runtime replacement validates current native ownership before mutation. Direct
MIDI loop/launch uses one guarded fixed effect, retained through quantized
scheduling and checked before any loop mutation or exclusive playback. Native
source/authority revisions retire old bindings immediately; effective runtime
loop refresh retires its old queued intent. Pending analysis/acceptance requests
do not revoke the previous effective accepted record. Fixed atomics and scalar
source/projection comparisons avoid callback locks, PCM scans and evidence or
source owners. Failed direct pad-trigger fallback refreshes current runtime and
uses the same guarded admission with the captured timestamp; it never emits an
unguarded partial loop/play sequence. Admission feedback does not claim later
scheduled acceptance; subsequently retired effects are discarded at execution.
G3b2d covers runtime pad triggers and their guarded fallback. G3b2h extends the
shared source/authority binding to the controller batch path below. MIDI global
actions dispatch through that controller instead of an unguarded direct stop-all.

## G3b2h controller global batch transaction

The controller captures one frozen CurrentPadTiming and opaque native
InputRuntimePadBinding per affected pad. The same authoritative binding comparison
used by MIDI runtime checks complete source/adoption identity, exact period and
signed origin bits; equal numbers do not hide another revision or signed zero.
START calculates matching effective loop endpoints once per captured snapshot.
STOP captures every active pad, including paused voices, while the restore intent
remembers only the playing subset. Generic stop-all does not create restore intent.
All required pads must have actual native source ownership; unavailable Automatic
or stale/foreign binding rejects admission without a legacy timing fallback.
Manual, Tap and Legacy remain nonaccepted. Pending/rejected accepted replacement
retains the previous acknowledged record through the existing current resolver.

Native START accepts tuples of binding/start/end; STOP accepts bindings. Each
batch is limited to the existing voice capacity and carries one captured input
timestamp for START. An opaque GlobalPlaybackBatchTicket distinguishes pending,
accepted and rejected execution. Enqueue alone cannot consume remembered intent
or establish successful playback. The callback schedules one transaction through
the existing output-clock/quantization path, with all starts on one output frame;
immutable batch storage is allocated on the control path and scheduler storage
is allocated at callback setup.

Admission and execution validate the entire batch against actual current source,
authority, acknowledgement and full effective accepted projection. Before any
loop, stop, start or bootstrap mutation, execution validates voice availability
and reserves enough off-realtime retirement and playback-feedback capacity for
the complete effect. Every required SampleStarted/SampleStopped must fit the
native-to-control ring before the first mutation; command-ring admission alone
cannot guarantee the controller projection. The callback is the single feedback
producer, so this bounded slots check reserves the whole transaction while the
consumer can only release capacity.
It prepares every start with existing single-pad voice preparation and rate/
trajectory behavior, then commits the loops and voice ownership together. STOP
also checks each actually pinned voice source/effective projection; a voice still
holding an old source after bank replacement rejects the entire current-bank STOP
batch without relabeling or partially stopping other voices. An admitted restart
can replace that old pin using the established current-bank adoption and retirement.

Full command/feedback rings or schedulers, stale ownership, unavailable Automatic, voice exhaustion,
retirement saturation and execution failure leave previous effective loops,
transport, PCM/native/FIFO/filter ownership and restore intent intact. Native
SampleStarted/SampleStopped messages remain the active/paused authority; ticket
polling updates only accepted GLOBAL restore intent and does not optimistically
clear or populate active/paused sets. An accepted STOP remembers its captured
playing subset; an accepted remembered START consumes the restore intent.
Pending or rejected work preserves the previous intent. Actual SampleStarted
telemetry clears the started pad's paused projection. The loader's unload callback
prunes only that pad from observed and pending restore ids and normalizes restore
engagement; a late accepted STOP cannot reinsert an unloaded/replaced pad while
unaffected captured restore ids remain. Productive MIDI global
actions drain the existing native playback event handler after observing batch
acceptance and before capturing their next targets; queued START feedback cannot
make an immediately following STOP overlook actual active voices. Native unload
reserves one playback-feedback slot through the existing bounded command drain,
deferring the entire unload when full, then emits ordered SampleStopped feedback.
This preserves FIFO state after an older queued START without another playback
or timing owner. Productive MIDI global
actions use the same controller and retain their original Rust input timestamp.

The callback adds fixed-capacity scalar/atomic checks and feedback only. It does
not allocate batch storage, scan/hash PCM, own evidence, acquire locks, call
Python/UI, perform I/O/logging, construct/reset native DSP or destroy large owners.
Existing source trajectory, voice/history invalidation and off-realtime retirement
remain authoritative. This batch work does not supply general explicit accepted
publication or derived loop/master refresh orchestration.

## Prepared source admission and retained stem projection

PreparedSourcePermit reuses the current native source/authority resolver and fixed
accepted projection alongside its existing actual source/content/request/rate/
preparation-epoch owner. The shared resolver verifies actual generation/digest
at capture. The fixed InputPadBinding retains source address/shape/loaded rate,
monotonic authority revision and, for Automatic, complete current acknowledged
accepted revision, binary64 period and independent signed origin. Its shared
owners contain fixed atomics; separate generation/digest fields do not enter the
callback binding. The pinned actual sample, matching source_version digest and
checked request/epoch/authority owners preserve the captured source ownership.
Automatic without a consistent current record fails admission. Manual/Tap/Legacy are admitted under
their own authority without promoting numerical values. Historical constant-timing
ticket metadata, raw revision, endpoint equality, source hash or preparation epoch
alone cannot establish current accepted evidence.

The productive stem path validates this binding before heavy preparation, after
decoding/alignment, under the request owner through enqueue and at callback adoption.
Request/epoch guards remain independent freshness checks. Pending or rejected timing
replacement leaves the previous acknowledged accepted projection current; a newer
request can still retire a preparation job. A successful source/authority/revision
change between stages rejects that ticket. Full queues leave it unconsumed and
failed or late-rejected admission preserves the previous audio. Fixed scalar/atomic
checks reach the callback, while sample/evidence owners retire off realtime.

Control revokes accepted acknowledgement before an admitted Legacy/Manual/Tap
parameter clear necessarily reaches the mixer. A fresh nonaccepted ticket can
capture valid current control authority during that interval, yet callback
adoption rejects it against the still-effective accepted mixer projection.
Previous PCM/audio remains valid; fresh capture after the effective clear can
succeed. This is a safe rejected transition, not historical-ticket authority or
a guarantee that control availability implies immediate adoption.

After actual stem adoption, immutable same-source PCM does not need preparation
again for timing-only changes. Successful native accepted adoption or clearing
updates only the retained PreparedStemSet's fixed effective projection, without
replacing PCM owners, resetting fractional carry or touching physical endpoints.
Pending/failed/rejected updates do not refresh it. source_reader checks the actual
reference source and exact effective accepted projection against effective mixer
timing. Full mix, every stem and both sides of a source-selection transition use
the same SourcePlayback rate/ramp, position, interpolation taps and loop/seek rules.
There is no new per-stem cursor or callback allocation/lock.

## G3b2f1 continuous productive voice source and native/FIFO history ownership

StretchProcessor fills its fixed feed storage directly from the actual borrowed
SampleBuffer through SourceReadPlan and the canonical SourcePlayback. Its actual
Rubber Band history and both pending adapter FIFOs carry fixed source address,
shape and loaded rate plus the complete effective accepted projection, with
bit-exact period and signed origin. A warmed source-neutral handle has no source
history. key_lock_source_preparation remains a separate test-only proof.

Before consuming each productive feed, the processor compares its expected next
fractional source position, including seek mode, with the actual canonical
position. A source mismatch or discontinuity clears bounded adapter storage and
marks used native state dirty before foreign feed can enter it. Native reset,
construction and warming use the existing preparation worker; exchange still
reserves bounded return capacity before ownership moves. No large sample owner
or native handle is destroyed on the callback.

Continuous same-source accepted adoption/clear refreshes the complete effective
projection on productive feed while retaining chronological native/FIFO history.
An equal-valued replacement still changes full revision identity. Rate changes
retain history and the canonical fractional epoch; pause freezes both progression
and history. Stem mode/mask crossfades continue through the same reader. Start,
retrigger, stop, seek, leaving wet processing and a loop clamp remain explicit
discontinuities. Pending, failed or rejected accepted replacement supplies no
new effective projection and cannot relabel history.

VoiceSlot pins the source it is actually playing and that source's effective
timing. A replacement bank sample and its accepted projection cannot relabel
the older active voice. The old voice may finish under its own prior effective
source/timing until explicit retrigger or stop; it is not CURRENT pad-bank
accepted authority. Retrigger adopts the actual current bank owner and retires
the old pin through existing off-realtime retirement. Same-source successful
adoption/clear updates only matching voices. Failed admission or late rejection
preserves the previously effective source, timing and history.

Explicit active seeks use the pinned voice's full source extent, even when a
shorter bank replacement is already current. Every successful explicit seek,
including a same-position seek, clears bounded adapter/FIFO history and fixed
per-pad filter ownership. Native reset/warming remains worker-owned.

New voice/retrigger adoption checks declared Automatic availability and effective
projection acknowledgement before replacing audio. It cannot treat unavailable
Automatic as Legacy fallback. After control revocation but before the effective
callback clear, fresh adoption can reject while old effective playback continues.
Manual/Tap/Legacy can admit after actual clear under their own authority. Required
old-pin retirement capacity is reserved before loop/exclusive/replacement side
effects. G3b2h reuses this per-voice preparation inside the complete guarded
controller transaction described above.

AudioEngine uses a tracked current native source fence with fixed generation,
address, shape and loaded-rate atomics. A zero generation is unavailable. Load
requests revoke the fence before releasing control cache ownership. Successful
loaded publication installs the command/cache/generation/digest under the request
owner, then publishes the fence; full queues change none of those values. Unload
and run revoke current fences. New adoption requires the callback bank to match
one bounded source check/recheck before and after timing availability, with no
spin or address dereference. Loading unavailable and replacement control PCM
ahead of the callback bank reject even Legacy new starts. Existing effective
voice/history ignores this new-adoption fence and retains its own pinned source.
The marker contains no PCM/digest/evidence owner and does not establish C1
immutable original-to-decoder lineage.

G3b2f1 binds productive continuous history. Source-specific worker preparation
and timed native/FIFO adoption are the G3b2f2 contract below, before G3b2g
persistence. Crop/delay correction and click-safe wet/bypass policy remain later
B5 work. Source-neutral reserve unavailability retains the existing
bounded wet-silence policy with canonical source progression; it does not promise
an audible seamless handover.

The productive PerPadDspChain also checks the actual rendered voice's source,
complete effective projection and expected fractional next position before
filtering a chunk. Continuous same-source timing/rate changes retain actual EQ/
isolator filter storage; a foreign source, loaded-rate mismatch or discontinuity
clears only fixed Rust filter state before replacement output enters it. Its
ledger counts actually filtered output, including existing wet fallback silence.
That records filter ownership and trajectory, not an assertion of audible source
content. It adds no native DSP allocation/reset/loading to the callback.

## G3b2f2 productive source-specific prepared native continuation

This contract is implemented and validated alongside this change; checked tasks
and actual production tests determine completion. Existing worker ownership is
extended with bounded request, prepared-result and recycling lanes. One third
preallocated prepared_native_history::NativeAdapterState per voice provides off-thread preparation capacity
without taking the voice's current effective state or its neutral warmed reserve.
The prepared state retains the actual Rubber Band handle and complete pending
input/output FIFOs, rather than only a source tag or newly warmed neutral handle.
Current/prepared NativeAdapterState owners and pending-request retention are boxed
at setup; ready/recycle/pending-return lanes move those preallocated boxes. Request
rings have fixed setup-allocated storage. This keeps large retained payloads out of
the 32-VoiceSlot Windows stack aggregate without a stack-size override. The callback
only moves/swaps existing owners; temporary catch-up feed/output scratch is worker-only.

A productive request pins the actual SampleBuffer and any admitted PreparedStemSet.
It copies SourcePlayback and SourceReadPlan at the actual render boundary, including
the physical loop, fractional source phase, explicit seek mode, binary64 rate target
and active-frame smoothing state, and the stem-selection state. NativeHistoryPermit
binds actual current generation/load request/loaded rate and shared preparation
request/epoch to declared authority and the same current acknowledged full accepted
projection as other native consumers. Exact period and signed origin bits remain
part of that projection. Manual/Tap/Legacy carry no accepted projection; unavailable
Automatic fails admission. A local invalidation epoch and request counter identify
this voice trajectory independently of musical acceptance and source-load ownership.
The local epoch is shared atomically with the worker, checked before/after catch-up
and at adoption; the exact outstanding request ID is checked before native exchange.

The worker applies the requested starting inverse-rate pitch before native reset,
then primes through the same fixed adapter and SourceReadPlan used by productive
rendering. Copied SourcePlayback chunk/advance performs exactly 4096 active output
frames, including existing rate smoothing. This fixed catch-up horizon is bounded
preparation work, not a native delay constant or musical/acoustic alignment policy.
Native and FIFO state remain owned together with the request pins until successful
adoption or off-thread recycling. Source-selection transitions do not have a copied
earlier transition history: requests defer until that transition has completed while
the current effective owner continues rendering.

The absolute adoption deadline is the captured request output frame plus 4096.
StretchProcessor::chunk_until_prepared_adoption splits a callback at that boundary;
catch-up is never performed on the callback. At the exact frame the callback checks
the complete current permit/effective accepted projection, actual source and stem
pointer identities, copied plan, canonical playback checkpoint, local invalidation
epoch and request identity. A loop, seek, stem/rate trajectory, source, request or
authority change cannot enter as a continuation; equal timing values cannot hide a
different complete accepted revision. Recycling capacity is reserved before the
native/FIFO state moves. Adoption exchanges the complete prepared state and keeps
the canonical logical source trajectory intact.

Pending, worker-failed, unready, late, stale or recycle-saturated adoption preserves
the old effective audio and native/FIFO owner. A rejected prepared state and its
sample/stem pins return through bounded lanes for destruction/recycling on the worker.
No callback native/large-owner destruction, reset, cold pitch preparation, PCM scan, lock, I/O,
Python/UI or logging is introduced. Prepared catch-up does not rewind transport,
rewrite markers, silently accept a proposal or establish guarded global launch.

Stop/reset/wet deactivation publishes local cancellation before owner retirement.
A separate bounded worker lane can retire current source/stem request pins while
the callback retains fenced dirty native/FIFO state. The worker discards stale
in-flight requests before publication, destroys their pins off realtime and records
completed request IDs. The callback uses that fixed acknowledgement to settle
cancelled pending work rather than becoming permanently stuck. Inactive and paused
voices poll the bounded ready/recycle retirement path on each callback without
source reads or native preparation; this closes the publish-after-cancel race even
when that voice never renders again. Teardown releases remaining tail owners only
off realtime after rendering has stopped.

Evidence must exercise the productive worker/exchange/render path, show actual
native handle/FIFO ownership and nonzero shifted output, compare a coherent
independent continuation, and cover current-source/timing and runtime failures.
The separate test-only key_lock_source_preparation fixture remains diagnostic;
it cannot substitute for this production path.

## Limits and remaining G3b2 consumers

This slice activates a precise native grid only through explicit control API
acceptance. Normal loading, manual/TAP controllers and project restore retain
their existing behavior. Source content is an observed loader digest; the
complete mono digest and Arc/timebase checks verify the actually loaded PCM, not
an immutable historical original-to-decoder relationship.

The neutral Key Lock reserves and test-only key_lock_source_preparation fixture
remain distinct from the productive source-specific native/FIFO ownership and
timed adoption described above. G3b2f2 must pass actual integration, failure and
ownership tests for that native gate. G3b2g supplies supported complete source-
verified SampleAnalysis/ProjectState persistence and fresh loader adoption.
G3b2h supplies source/accepted-bound controller GLOBAL START/STOP batches including
MIDI. General explicit acceptance/derived loop/master refresh orchestration remains
open; loader-specific refresh alone cannot close it. Original-hash association does not prove immutable
copy-first/ABA lineage (C1). G3c retains separate musical/rounded physical loops
over 75/1000 cycles, fractional periods/rates, callback partitions/wrap and
rendered/onset/device/listening gates. Later B5 audible crop/delay/transition
compensation is separate from required f2 integration. Current 96-handle setup/RAM
and persistence integrity I/O/CPU costs remain unmeasured; historical 64-handle
measurements do not cover them. This slice claims no audible SYNC.
