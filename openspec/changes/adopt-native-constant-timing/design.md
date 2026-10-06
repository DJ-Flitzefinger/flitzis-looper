# G3b2a-e and G3b2f1 native adoption, current authority and precise consumers

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
Controller-owned global START/STOP retains ordinary Python batch launch semantics,
including its MIDI mapping. It does not consume RuntimePadState; accepted/source-
bound global batch launch and caller-owned refresh/adoption orchestration remain
follow-up consumers. G3b2d covers runtime pad triggers and their guarded fallback.

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
effects. This remains per-voice adoption, not guarded controller global batching.

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

G3b2f1 binds productive continuous history. It supplies no source-specific worker
priming, retained prepared native/FIFO owner or timed prepared adoption/catch-up.
Those are required NEXT G3b2f2 before G3b2g persistence, not deferred B5 acoustic
compensation. Crop/delay correction and click-safe wet/bypass policy remain later
B5 work. Reserve unavailability retains the existing
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

## Limits and remaining G3b2 consumers

This slice activates a precise native grid only through explicit control API
acceptance. Normal loading, manual/TAP controllers and project restore retain
their existing behavior. Source content is an observed loader digest; the
complete mono digest and Arc/timebase checks verify the actually loaded PCM, not
an immutable historical original-to-decoder relationship.

The existing Key Lock warmed-state pool is source-neutral;
key_lock_source_preparation is test-only, not productive source priming or timed
native-history handover. G3b2f is incomplete: required NEXT G3b2f2 must integrate
source-specific worker priming, retained prepared native/FIFO ownership, full
source/current-accepted-revision/rate/epoch permits and timed transactional
adoption with catch-up. Continuous f1 identity is insufficient for that prepared
ownership. This native integration precedes G3b2g accepted source-verified
SampleAnalysis/ProjectState persistence and loader schema with
fresh runtime adoption, source/accepted-bound controller global START/STOP batch
launch including MIDI, and explicit acceptance/derived loop/master refresh
orchestration remain open. Original-hash association does not prove immutable
copy-first/ABA lineage (C1). G3c retains separate musical/rounded physical loops
over 75/1000 cycles, fractional periods/rates, callback partitions/wrap and
rendered/onset/device/listening gates. Later B5 audible crop/delay/transition
compensation is separate from required f2 integration. This slice claims no audible SYNC.
