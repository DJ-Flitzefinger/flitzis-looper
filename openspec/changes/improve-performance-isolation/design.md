# Performance audit and implementation design

## Initial audit boundary

Baseline: `69a8098d84ae58d616f988f5662e6038afdcf0e0`, inspected 2026-10-30.
The first step is a source audit and plan, not a fresh benchmark or runtime fix.
The reported direction is specifically slowed non-master PADs with BPM Lock and
Key Lock; faster PADs and the master are controls. Generation, postprocessing,
component loading and live adoption must be measured separately.

## Current Human clarification and STEP10 impact

STEP11 has now assessed the principal DSP response and retains the current
runtime without a new cutover. Its bounded profile measurements and separate
source-prefeed tradeoff are recorded below; no DSP speedup or optimality is claimed.

The Human subsequently required an approximate BPM fallback with a visible
inaccuracy warning when the precise method fails. STEP12 implements that separate
scalar-only contract, followed by every existing stem/resource/provider/inference/
final duty in steps 13–19. STEP11 itself introduced no such runtime change.

## STEP15 bounded numeric conversion

The implementation retains the established Python file/commit/controller chain
and replaces only block arithmetic with the dependency-free portable
`flitzis-stem-conversion` core and thin PyO3 bridge. No engine/control borrow,
file access, Python callback or new scheduler enters the core. Raw immutable
spans cross synchronous detach; global binary64 positions, CPython 3.14-style
compensated channel averaging, ties-even endpoints/nonfinite policy and ordered
pre-quantization instrumental clipping preserve the byte contract. Maximum block
size remains 4096, with the existing conservative 1-GiB PCM cap and adaptation.

Before source EOF probes or block reads, a single-use opaque reservation promises
raw input and all overlapping native/Python output/write buffers plus 64-KiB host
metadata/scratch on the existing shared host ledger. Windows retains the existing
2-GiB commit/512-MiB physical stem entry floors plus the prospective delta; resource
recovery/critical thresholds are unchanged. Terminal close occurs after synchronous
work/writes end, including retained exceptions; repeat/concurrent use cannot
multiply an allowance. Other OS host monitoring is explicitly unsupported, while
the platform-independent core's hard extent cap remains. No OS settings change.

Cancellation checks surround blocks and precede writes. Native code finishes its
bounded current block; the real worker/job keeps source, processing and generation
custody until return. Existing exclusive writers, RIFF/RF64 geometry, full hash/EOF
verification, private replacements, descriptor-last commit and later own adoption
ACK remain separate authorities. A supported complete version3 producer proof
additionally binds the loaded converter's embedded complete core/bridge/Cargo
sources, lock and platform/profile, rather than current source-path bytes. The
native component identity is positively checked, separately from all other roles.
This is compiled-source lineage, not full binary/toolchain attestation.

The extra complete-host positive probe exposed an unchanged parent limitation:
both manifests fail closed at `StemExecution._coherent_fallback`'s Pydantic
`@model_validator`, which the existing decorator policy cannot attest. The exact
parent and current probes both reject that role after the preceding roles pass.
Every production manifest role and guard remains; full producer/fingerprint stays
UNKNOWN. New positive artifact/PCM/process-role checks are subset evidence only;
two explicit full-host tests retain the unsupported result. Narrow actual
validator/schema/Pydantic dependency attestation is a mandatory FINAL19 gate.

### STEP15 verification and measurement limits

Five independent pure-core regular tests pass. Thirteen focused Native tests pass
with no failures or ignores on the original stack configuration, including the
new actual worker/cancellation and public-control cases plus retained window
guards. The held-core observer stops after real allocation and one converted
frame. Unrelated PAD215 preserves its original T and real START, all 64 stereo
PCM frame bits against an independent dry renderer, and public STOP; the whole
headless fixture roundtrip was 1363 us (n=1), not event-loop/device latency.
The cancellation case retains actual Native source/processing readers, rejects
new capacity while all 34 slots are occupied, writes no new output after cancel,
joins its executor and reclaims every permit before reporting worker retirement.
The asset-retirement recorder in that probe is not a physical deletion proof.

The initial 17-file Python selection has 601 passes, 7 failures and 1 skip. All
seven failed leaf IDs and exact original assertion/value classes reproduce on
unchanged parent `a5992b0d` (7 failures, 1 model control pass). Five explicit public
Demucs Settings inputs restore those tests' declared Demucs-to-BS-to-Demucs or
producer setup before capture; defaults, shared fixtures and every assertion stay.
The final nine-file affected selection passes 443 cases with 10 existing skips;
it overlaps the initial successes. It includes 96k/44.1k channel/block byte parity,
WAV/RF64/failure cleanup, both backend seams, intent, cancellation and Portable API.
Nine final native binding/lineage/preflight cases and six static checks pass in
a separate overlapping run. Positive seven-role artifact/PCM/process evidence is
separate from the two explicit full-host UNKNOWN cases. The original speculative
full-host-positive pair failed on both parent and current at the same unsupported
validator; those receipts remain, without removing any production role or guard.
Ten existing skips remain, including the opt-in greater-than-4-GiB LONG case;
no device opt-in was enabled. Owned child cancellation tests use explicitly
non-neural fixture models. No production separator inference or App/device session
was run here.

The same five-second synthetic 48k stereo inputs, block4096, complete five-file
writer/verification/private replacement path were measured before and after:

| Target rate | Before wall / process CPU | Native wall / process CPU | Complete file identity |
| --- | --- | --- | --- |
| 48 kHz | 1.147695 s / 1.125000 s | 0.043664 s / 0.031250 s | All five SHA256 values identical |
| 44.1 kHz | 1.100147 s / 1.093750 s | 0.040544 s / 0.031250 s | All five SHA256 values identical |

This is one sample per rate, about 26–27x faster for this conversion fixture,
including unchanged file work. CPU observations have coarse timer granularity.
It is not cold-disk, competing-inference, callback-tail or universal throughput
evidence. Reproduce with the existing `_outputs` fixture (48k,2ch,240000 frames),
`AudioShape(rate,2,rate*5)`, default block/cap, wall/process timers surrounding
`write_project_cache_artifacts`, then SHA256 of every output. The independent
legacy oracle, not timing, decides byte correctness. Rust focused recipes select
only `window_control_isolation_tests::` and `stem_conversion::tests::`; the pure
core runs separately, excluding other workspace test crates. The Windows wrapper
always adds `--workspace`, so `-p` alone does not isolate a crate. The initial
overbroad compile was cancelled before any tests and contributes no pass receipt.
Final format/lint, strict mypy for the four changed Python modules, strict
OpenSpec, local Markdown targets and whitespace pass;
anchors were not separately rescored. External Release Native/API retains the
22 known library warnings and existing einops pin/warning.

This port alone removes no Torch and establishes no inference, CUDA, musical/device
or entire GIL-isolation acceptance. STEP16 budgets,17 PCM/providers,18 practical
native inference/Torch removal with mandatory CUDA,19 combined gates remain.

## STEP14 source/window and Python isolation result

Window preparation now reserves the complete source/component byte, owner,
assignment-reader and source-history bundle before source I/O. History promises
follow an immutable accounting row identity through Move; removed rows refuse
commit, and Copy retains independent target history. Explicit worker pins keep
historical PCM charged once to its own scope after its original views end; a
foreign PCM reference alone cannot revive an ended owner. The raw snapshot is
released immediately after transferring custody to its accounting token.
Complete legacy conversion also promises its full transient host envelope before
source I/O and retains that promise until its actual complete/converter/crop
buffers end. The existing 1-GiB legacy cap and host-headroom guards remain.

Owners is released across source/component I/O and registration. Short sections
revalidate the original content/lifetime, source/window, bank placement, exact
proposal and intent before and after preparation, including Pending mirrors.
Immutable source, pair and reader custody stays with the worker. The shared
worker-only preparation gate precedes owners and ends before publication; the
existing final CAS and each operation's own effective ACK remain authoritative.

Owned shared control contexts release the Python Engine borrow before native
reconciliation, binding preflight, both registrations and admission waits. Public
names and parameters remain unchanged. The original gesture/T and capture order
are retained across detach. Successful explicit, policy and registration admission
have separate watermarks; a failed newer capture does not revoke older viable
work. Registration rechecks source/window, placement, discard, order and the
original lane's lifecycle after its final lock waits, including shutdown/restart.
No whole Engine, stream or loader is cloned.

### STEP14 verification and remaining gates

All ten new integration cases and three new accounting cases pass. Actual held
source chunks and component reads allow mutable public Python controls and an
independent captured START/STOP to progress, preserving the captured original T and
final PCM. Tests cover both registration paths, binding/request preflight, newer
successful versus failed admission, Move before/after the Pending mirror, Copy,
pre-I/O byte/owner/reader/history and legacy-host refusal, cancellation through
actual worker end, scoped historical pins and retired-row refusal. Compatible
source/window, ABA, sharing, own-ACK, last-reader and production 2-MiB constructor/
callback regressions also pass. The selected Python controller/residency/Copy/
transport files pass all 283 cases against final-source external Release Native.
Native/API verification passes with the existing 22 library warnings; the einops
pin and warning are unchanged. Changed-source Rust format, strict OpenSpec
validation, local Markdown targets and diff whitespace checks pass.

The initial 440-case Rust selection aborts after 164 passes, two ignored cases and
one registry failure, followed by a default-stack overflow in the next lifecycle
test. The unchanged parent reproduces the same overflow. Three narrow test carriers now use the existing direct boxed
mixer constructor; original PCM/Native/ACK/history assertions and the explicit
production 2-MiB guard remain unchanged. The separate rest selection, with no stack override, completes
with 310 passes, 33 failures and 15 ignored cases. The isolated registry replay
passes, but its initial failure body was lost at process abort: cause UNKNOWN.
Do not infer a leak, capacity fix or successful combined registry rerun.

A name-based audit of these three logs yields 475 distinct successful Rust cases
and 17 distinct ignored cases; initial and rest successful sets do not overlap.
This is source-compatible evidence across selections, not one clean matrix run.
Of the 33 remaining failures, 32 are compared with the unchanged parent: all 32
also fail there, with 31 matching normalized leaf bodies. The duplicate-216 case
fails the same `one material inferred twice` assertion but has different async
PAD/counter/fingerprint snapshots; it is not an identical-state comparison.
Normalization changes only generated sample-root prefixes and received timestamp
keys, not PCM, counters, identities or assertions. The comparison uses a process-
only larger parent test stack to get past its separately reproduced stack abort.

The remaining distinct-216 batch is UNCOMPARED and incomplete: its existing
600-second complete-phase test-host watchdog expires while progressing, reporting
177 declared nonneural generations, 145 saved and zero failed. This is synthetic
output/stopped-bank orchestration, not 216 voices, model inference or a throughput
acceptance. Its semantic stall guard, all-216 completion assertions and production
resource guards remain unchanged. The 32 parent failures include ready-E/silence
expectations, missing test InputRuntime, Copy-journal custody failures in Bank/UI/
recovery/last-reader composition, old Hold/Piano/overwrite scheduling expectations
and duplicate-batch provenance. None is declared fixed or waived by STEP14.

Systems and the same independent actual-diff review chain find the changed STEP14
invariants covered. The 32 failure classes, unmatched watchdog and unexplained
initial registry exception remain explicit final STEP19 regression gates. This
scoped lock/borrow result does not establish complete musical/device quality,
whole-GIL/CPU isolation, total latency, crackle cause or resolved commit pressure.
Conversion, budgets, providers, native inference/Torch parity and portable
Windows/Linux/macOS distribution remain mandatory steps 15–19.

## STEP13 committed component isolation

The controlled positioned-read baseline holds the real first component chunk.
Owners remains unavailable and another native request cannot return during a
roughly 212–215 ms observation. A public Python request and independent Python
controls also cannot progress during the held read. These are deterministic
control-stall observations, not device/audio latency or proof of crackle cause.

Committed components now use a separate cancellable worker-only preparation
gate. It preserves the previous serialized shared lookup, memory peaks and
complete reader registration. Owners is held only to validate current source,
content placement, exact proposal/status owner and intent before preparation,
after reads and after registration. Existing scoped byte/owner/reader promises,
immutable pins, cancellation checks and final publication CAS remain authoritative.
The gate is released before control publication. FullMix and unrelated controls
never acquire it. A cancelled waiter can retire while another reader remains held.

The public residency request captures owned post-preflight admission state,
drops the Python Engine borrow and detaches native admission. The same bounded
cold lane is used; no new worker pool is created. Under-lock revalidation checks
source/epoch, placement, launch, copy and selection before proposal mutation.
Separate per-content explicit/candidate and policy capture sequences retain
admission order across detach. Only successful reserved capture enqueue advances
the admitted watermark. A late older context cannot supersede a newer admitted
intent; a failed/Pending newer admission leaves both older viable contexts and
already admitted proposals/readers intact. Capture order is not a live proposal
epoch and grants no audio acknowledgement.

At the STEP13 boundary, WindowWork still held owners across source/window/
component I/O to protect physical-slot PCM history and peak/copy accounting. It
acquired the same worker-only gate BEFORE owners, preserving cross-worker serialization.
Removing that owners span needs a separate complete reservation/history/placement
proof. Borrowed reconciliation and binding-capture preflight could still wait there;
registration wrappers also remained borrowed. STEP14 above resolves those residual
control/GIL stalls. The full plan grows to nineteen steps: conversion15,
budgets16, PCM/providers17, native inference/Torch18 and combined regression19.
All original functionality, CUDA/CPU fallback, quality and portability duties remain.

### STEP13 verification boundary

Focused checks cover held component reads, actual unrelated START/STOP, public
Python and mutable Engine progress during detached admission, same-PAD cancel/
FullMix supersession, concurrent sharing with independent own ACK, reservations,
placement Pending/move and equal-PCM lifetime ABA. Existing source/window,
admission, failure, history, effective-ramp and last-reader checks remain required.
Nine new regular checks and two explicitly invoked diagnostics pass. During the
held component chunk, owners and an independent native admission remain available
(the latter reports 180 microseconds in the final affected run); public Python
controls and actual unrelated START/STOP also progress. FullMix requires its own
effective ACK and produces the independent 1024-frame PCM reference while the old
read remains held. This is controlled progress evidence, not total audio latency.

The first selected integrated Rust run reports 215 passes, 42 failures and two
ignored diagnostics. An unchanged-parent comparison reproduces 41 failures with
matching leaf assertions/messages and printed values. The remaining reader-registry
case passes independently on both versions and in the affected rerun; its earlier
cause is unproved. Narrow fixture repairs restore explicit model/global-mode
inputs, observe actual due-ready E with genuine empty callbacks before consuming
PCM, retain a real future grid T for scheduled sharing, and initialize independent
range-reference domains and launch coordinates. Original PCM, Native, source,
provenance, ordering, ACK and reader assertions remain intact. The affected
66-case run reports 61 passes and five further fixture failures; those exact five
then pass after the final setup corrections. Compatible earlier successes are
reused; these overlapping results are not a single clean integrated-run claim.

The selected Python controller/reservation/transport/Portable checks initially
report 572 passes and nine failures, all nine reproduced on the unchanged parent.
Explicit Demucs and global-batch fixture inputs preserve the original assertions;
the three affected files then pass all 93 cases. The distinct selected union is
581 successful cases, not the sum of overlapping runs. These controller/mock
checks supplement the actual Native probes. Final-source external Release
Native/API verification passes with the existing 22 library warnings; the einops
warning and pin are unchanged. Format, lint and specification checks accompany
the final diff.

No model inference, App/device session, installed Human Portable update, total
latency gain, whole-GIL/CPU isolation or resolved system commit pressure is claimed.

## STEP12 approximate BPM implementation

Version 11 retains the failed precise v4/v9 header and its original mathematics,
source/model/actual execution identities and optional captured range. A usable
nonlimited v9 period is selected directly; supported median raw intervals are
the simpler alternative. Neither changes any failed precision diagnostic. When
no usable neural candidate exists, bounded source-energy novelty supplies a
coarse estimate on the same admitted off-RT worker, including inference failure.
Three deduplicated fixed windows use at most 2000 approximately 100-Hz bins each;
positive mean-absolute multichannel energy changes avoid stereo cancellation.
Nine-bin triangular smoothing and mean-centered normalized autocorrelation over
20–200 bins require score >=0.35. The shortest local peak within 95% of the
strongest peak is eligible; the strongest eligible window wins. Smoothed novelty
must reach 10% of the window mean level to reject small stationary-carrier
fluctuations. Python matches Rust's sequential binary64 accumulation, including
near ties between different-tempo windows. Selection is independent of request
range. Sources shorter than eight seconds, silence,
constant/invalid PCM and unsupported rhythm return no coarse source estimate.
Neural interval support needs eight intervals and 60% support within 15% of their
median. These are approximate engineering heuristics, not musical ground truth.

Python strictly replays the retained evidence, scalar and range. Saved failed
source evidence can be reused only after the current Native original digest
matches, preserving current source/request and performer timing intent. No model
rerun is needed for such evidence. Valid same-source precise results win before
Native scalar mutation. Manual/Tap/half-double and explicit grid intent remain
authoritative. The approximate result publishes an empty beat grid, no accepted
timing, and a defensive anchor guard; raw neural observations remain diagnostic.
PAD/details show Approximate BPM / may be inaccurate from persisted provenance,
also after restore/copy. Existing v10 mean/precise-QM requirements stay unchanged.

Startup, loop/master resolution and manual-clear fallback also exclude saved
approximate scalars until their actual loaded source matches. Automatic cached
publication captures and binds the Native source generation. An unavailable
digest holds valid saved evidence without publishing it; a proven changed source
invalidates its stale automatic scalar. Explicit performer choices stay separate.
Scalar admission precedes project mutation. A later derived grid/loop refresh
failure retains the admitted scalar and reports the error, rather than silently
restoring an inconsistent old project value. Saved-residency planning waits for
source verification before deriving an approximate automatic loop.

### STEP12 verification and limits

The reported saved version-9 `insufficient` packet replays to approximately
90.004297279 BPM without another inference. Its failed precision diagnostics and
original execution receipt remain unchanged. This does not establish the true
musical tempo. Six Native fallback tests pass, including 44.1/48/96 kHz opposite-
polarity stereo at approximately 90/120 BPM, quiet intro and nearly tied 100/120
sections, stationary tones/noise, silence/DC/nonfinite/short input, cancellation,
budget refusal and the explicit saved-period/interval wrapper replay. All six
energy packets, the mixed-window packet and both wrapper packets validate under
the strict Python contract. The wrapper replay is scalar/provenance evidence;
actual source admission and current-generation publication remain separate guards.

The regular analysis closure passes 41 cases; five overlap the six above, yielding
42 distinct successful Rust cases. Three older optional cached-QM/mean diagnostics
remain unrun; the new replay test was explicitly executed. The focused integrated
Python/check closure has 809 distinct successful cases and two existing skips:
806 compatible integrated successes, two corrected legacy fixture cases and one
new accepted-Native precedence test. The final 177-case controls/schema/metadata
run overlaps that count. The two old Global START fixtures failed identically on
the unchanged baseline: they expected a two-PAD batch without enabling MultiLoop.
Explicitly enabling that tested mode preserves every source/receipt/deferred and
publication assertion. No runtime guard was weakened.

Changed-file Ruff, five-file strict mypy, Rust/Python formatting, strict OpenSpec,
local documentation links and whitespace checks pass. External Release Native/API
verification passes with the existing 22 library warnings; the einops warning and
pin are unchanged. Systems PRE/frozen POST, Methods and one independent final-diff
review chain are clear. Initial fractional-bin aliasing, a test mock setup, style
and type errors were corrected rather than accepted or hidden. Python bounded
evidence replay runs during load/save/copy, never render; this is not a measured
background-isolation or latency improvement.

Reproduce through the configured adapter's `rust-test` action with
`-p flitzis-looper --lib app_analysis:: --release -- --test-threads=1`.
The source is [app_analysis_approximate_tests.rs](../../../rust/crates/looper/src/audio_engine/app_analysis_approximate_tests.rs).
For explicit wrapper replay, set `FLITZIS_APPROXIMATE_EVIDENCE_DIR` to an owned
absolute generated-output directory containing validated `period-input.json`
and `interval-input.json`, then run the `app_analysis::approximate` filter with
`--include-ignored`. Feed exported JSON through `AppTempoMethod.model_validate_json`
with `strict=True`; do not substitute that replay for actual source/ACK proof.
Run the corresponding Python controller/transport/loader/restore/copy/UI tests
through the same configured adapter. Saved source evidence requires no new model
inference. No App/device/listening or whole-musical-quality acceptance is claimed.
The separate stem resource-pressure report remains open in loading/resource
steps; BPM fallback does not fix it. Steps 13–19 retain their complete scope.

On 2026-10-31 the Human clarified that 30 ms was a suggestion, not a mandatory
acceptance threshold. Seek the shortest practical latency with complete quality
and functions and proportionate complexity. Do not force a DSP redesign for a
small numerical miss or claim technical optimality from bounded measurements.
Earlier target-unmet statements below retain their historical measurement
meaning; they do not create a mandatory gate. STEP9's 31 ms was saved scheduling
wait for the fourth original, not its total latency.

A real deferred group retry may reuse an already prepared physical beginning
only when every Work was in group custody before the current due attempt.
Require the exact header/member sequence, owner, original T, content, epoch and
planned E, no possible preceding original, and unchanged replan/delay bounds.
Capture all fresh source/permit/DSP requests and validate capture/immutable STOP
before and after planning, then bind every member through a separate later-E
PREPARED/SCHEDULED-to-PREPARED CAS with the exact expected E. Only complete success
moves common E/pending capture S, consuming one existing replan allowance per
member. The same already-popped DeferredEvent retains scheduler order/capacity;
it supplies no queued-completion receipt. Final effect/capture claims are unchanged.

This is one bind per unconsumed bundle. Occupied displaced-request custody,
missing/stale metadata, unavailable proof, live predecessors, AttackAtGrid and
actual/history/retained-feed paths keep real legacy reprepare. No Native reset,
prepare, source consumption or heavy allocation enters the callback. A lost
member bind rejects the whole group before audio/spool stores; Work and every
old/failed request pin retire through existing off-RT custody. Principal DSP and
all remaining stem/conversion/budget/provider/native-inference duties stay in
steps 11–17, under the clarified practical-benefit standard.

## STEP9 impact and retained scope

Visit reserved originals in captured (T, sequence) order. A successful whole
fresh completion bind and exact same-event retarget supplies a scalar receipt
only for this bounded pass. A later independent birth may use a live predecessor
only through that receipt and its actual queued header event, which must sort
before the proposed later event using the scheduler's real retained order.
Preserve captured spacing with E=max(T, callback, predecessor E + T - predecessor T),
using checked arithmetic. Equal T gains no minimum interval; distinct originals
retain their gaps rather than collapse into one attack. Every original retains
its own request, any required mutable Native and final effect claim. Group receipts require
every member bind, common E and the existing capture/immutable STOP checks.

Unreturned, unknown, unbound, deferred and partially bound predecessors remain
barriers. Final execution still uses the unchanged preceding-endpoint rule, so
a predecessor's transient failure or replan cannot be overtaken. No heavy prepare,
reset, source consumption, early retirement or fallback/replan-budget change.
Verify complete piecewise physical PCM, chronological slot reuse, actual event
order, busy and final-deferred predecessors, mixed ordinary/Piano/groups and
recording bits. Measure readiness separately from scheduling and principal DSP.
Genuinely deferred group-held Work requires separate later-E prepared-state/CAS
proof and remains open as step 10. Principal DSP becomes 11; the full plan grows
to seventeen steps without dropping stem, conversion, budgets, providers, native
inference/Torch parity or combined regression work.

## STEP8 impact and retained scope

An actually rejected member proves its exact retained atomic group cannot pass
all member effect claims. Polling now attempts each peer's existing rejection
CAS before readiness acceleration. Only actual terminal owners stop ordering;
a lost CAS remains a barrier until a subsequent successful retry. Missing owner
metadata, epochs and deadlines are not proof. Slots, queued events, Work, worker
and Native/source pins, capacity and recording lifecycle remain untouched until
normal cleanup. Applying/applied effects stay protected by the existing CAS.
This serves ordinary, Piano and whole-group successors on a different old peer.
Live earlier originals and group-held deferred Work keep their strict paths and
become step 9; principal DSP becomes step 10. All remaining work is retained in
the sixteen-step plan. Verify real held/returned workers, full physical PCM,
source/request identity, allocation/capacity custody, CAS loss and live/unknown/
epoch-only negative controls, plus recording no-prefix failure. Measure the
affected peer seam separately from readiness compute and principal DSP.

## STEP4 impact and retained scope

Remove only the ordering barrier of a proven terminal rejected original in
`PitchRuntime::preceding_endpoint`. Rejection is monotonic and cannot win over
an applying/applied effect. A live or unobserved owner remains a barrier. The
old slot, queued event, worker, Native/source pins and retirement capacity stay
owned until their existing cleanup; no callback reclamation is introduced.
This shared ordering helper affects ordinary/Piano and atomic-group ordering,
while fresh request/source/DSP/OwnACK guards and recording S/E checks remain.
Verify returned and worker-held rejected owners, live-order negative controls,
whole physical PCM, atomic groups and callback allocations. Measure readiness,
scheduling E and principal attack separately on the targeted cancellation seam.
Piano/recording/late-result acceleration remains STEP5; principal DSP is STEP6.
The complete plan grows to twelve steps without dropping the remaining work.

## STEP5 impact and retained scope

Extend initial-interval completion to constant independent Piano SetRetrigger
FirstSoundAtGrid physical births. Reuse the current pitch guard, exact retained
start snapshot, fresh action-owned history permit and DSP intent to capture a
whole E-bound request. Exact playback plus endpoint/source/render equality must
hold; no actual/retained feed or history-bound reset is eligible. Retain the
displaced request in the original Work until off-callback retirement, then move
the same scheduler event with its original order and capacity. Original T stays
immutable. SET, AttackAtGrid, glides and any legacy replan stay on existing paths.
Verify held workers, full physical PCM/EQ, rapid originals, future T, stale/cancel/
feedback/capacity and callback allocations. Measure ready completion, scheduling
and principal attack separately. Recording S/E/STOP, late genuine reprepare and
remaining live/group ordering become STEP6; principal DSP becomes STEP7. The
remaining stem, conversion, budget, provider, Torch and regression scope remains
in the now thirteen-step plan. No scheduling result passes the 30 ms DSP goal.

## STEP6 impact and retained scope

Extend initial-fallback completion to attached recording groups. Validate the
original pending capture/session, geometry, cancellation and immutable admitted
STOP at the proposed common E before binding any member. Reuse every member's
whole fresh source/permit/DSP plan and retain displaced requests off realtime.
After all binds succeed, retarget the same scheduler event and update pending
capture S to the same E in this callback. Preserve OriginalOrder, STOP reservation,
capacity and final all-member/capture effect claims. No worker reset or source
consumption enters the callback. Verify genuine partial readiness, independent
full PCM and exact spool bits, equal-E/immutable STOP, future T and failure fences.
Late genuine reprepare and still-live original/group-peer ordering remain STEP7;
principal DSP becomes STEP8. The full plan grows to fourteen steps, retaining all
stem, conversion, budget, provider, practical Torch removal and final parity work.

## Source findings

### STEP7 impact and retained scope

Permit one fresh completion bind after a genuinely successful off-RT legacy
reprepare of an independent FirstSound physical birth. Ordinary and Piano Work
record the exact rebuilt E only after full preparation and prepared-status
publication succeed. The worker then releases displaced completion-request pins;
the callback never replaces an occupied displaced-request slot. A new reprepare
or successful completion consumes the receipt. Replanned runtime slots require
that exact E-bound receipt, while every atomic group member also matches the
header's planned E. Existing whole fresh request/permit/DSP comparators, physical
beginning, no actual/retained feed, capture/session/immutable STOP checks and
same-event order/capacity remain authoritative. Replan count and maximum launch
delay do not reset. Work already moved into a deferred group's own custody remains
on its existing path. Live predecessors remain strict barriers; optimizing live
originals/nonterminal old-group peers is a separate step 8. Principal DSP becomes
step 9, followed by all existing stem/conversion/budget/provider/Torch/final work:
the full plan now has fifteen steps. Verify genuine second worker holds, repeated
completion/reset cycles, complete PCM and exact recording spool, cancellation/
source/control fences and RT allocations. No 30 ms or readiness-speedup claim.

This table records the initial audit baseline. The STEP2 receipt below records
the implemented subset and supersedes its corresponding reserve-miss observations.

| Finding | Evidence | Implication and next verification |
| --- | --- | --- |
| Fixed preparation scheduling lead | `pitch_runtime.rs`: `PREPARATION_LEAD_FRAMES`, `execution_frame`, `next_replan`, `accept_ordinary` | An already-due start needing a worker is planned at callback +8192 frames: 185.76 ms at 44.1 kHz, 170.67 ms at 48 kHz, 85.33 ms at 96 kHz. These are arithmetic scheduling costs, not measured input latency. Measure hit/miss and E-T on the actual ordinary route. |
| Single-use, selected-only reserve | `ordinary_endpoint_reserve.rs`: `transfer_to`; `mixer/endpoint_reserve.rs`: `capture_selected_normal_endpoint` | Only the selected Normal endpoint has one candidate; consumption requires replenishment. Unselected/repeated starts can miss. Bounded multi-PAD readiness needs explicit memory/ownership admission. |
| Two-stage capture and groups | `audio_stream.rs`: `CapturePadStartTarget`, `PreparedStart`; `pitch_runtime.rs`: `accept_ordinary`, `accept_group`; Python `transport/residency.py`: captured preparation/ACK/publication | A past original target excludes even a matching reserve. Atomic groups OR their members' preparation needs without a reserve-hit route. Preserve original input/T and one common actual E; measure GUI and native MIDI separately. |
| Unity bypass and direction | `mixer/ordinary_start.rs`: `requires_ordinary_start_preparation`; `pitch_intent.rs`: `native_pitch_scale` | Native scale is h/r with Key Lock and h without it. Master r=1/h=1 bypasses preparation; slowed r<1 requires upward Native compensation. The fixed lead affects both nonunity directions and alone does not explain a slower-only symptom. |
| Prepared does not mean attack-compensated | `retained_native_start.rs`: physical-birth silent priming; `key_lock_physical_onset_tests.rs`; existing latency example | Warmed handles do not prove principal-attack alignment. Separate first sound, significant onset and principal attack from nominal Native delay. Existing onset/quality failures remain open. |
| Serial preparation and serial render | `pitch_actions/preparation.rs`: worker loop; `key_lock_preparation.rs`: worker lanes; `mixer.rs`: `render_rt_chunk` | Independent worker work is partly serial, and voices render serially in the callback. Measure worker queue tails and callback cost before adding concurrency; a blocking general threadpool inside the callback is unsuitable. |
| Stem I/O holds global owners | `committed_stem_residency.rs`: `current_owners`, `prepare`, `request_committed_stem_residency_with_interest_and_producer`; public wrappers in `audio_engine/mod.rs` | Preparation retains the global owners guard across component reads/allocation. A request can acquire the shared producer then wait for owners under the GIL. Test slow component reads while another PAD triggers/stops; reduce lock scope without weakening admission. This is a control stall risk, not evidence of a callback mutex. |
| Stem postprocessing | `controller/stem_artifacts.py`, `flitzis-stem-conversion`; both generation backends | STEP15 replaces productive per-sample Python arithmetic with a detached bounded core. Python retains file verification/private commit; scoped byte, control and throughput evidence does not certify inference or full producer attestation. |
| Inference separation lacks CPU budgeting | `stem_workers.py`, `stem_generation.py`: `demucs_cache_environment`; `stem_process.py`; `analysis/process.py`: `_environment`, `run_owned_command` | Two stem workers can run broad child/native thread pools. Beat-worker BLAS caps do not apply to the generic stem route. Process-tree custody is already implemented; it is not a CPU reservation. Measure inference, conversion and loading separately. |
| Finite/shared PCM already implemented | `stem_pair.rs`: component view preparation; `resident_accounting.rs`; `cold_jobs.rs`; `committed_stem_residency.rs` | Direct Arc allocation, immutable reuse, scoped unique accounting, pending promises, host headroom and bounded cold workers already exist. Profile process RSS/private commit/paging and last-reader plateaus rather than rebuild these foundations. |
| CUDA and fallback already exist | `bs_roformer_generation.py`, `stem_generation.py`, offline worker code | Preserve actual device/fallback receipts. Additional offline acceleration needs transfer-inclusive evidence; no measured case currently supports moving small callback blocks to CUDA. |

Paths above are under
[`rust/crates/looper/src/audio_engine`](../../../rust/crates/looper/src/audio_engine/)
or [`src/flitzis_looper`](../../../src/flitzis_looper/), as qualified.

## STEP2 production response and exact-ready scheduling

Measured 2026-10-30 on Windows x64, Ryzen 9 9955HX3D, optimized Release, pinned
Rubber Band 4.0.0. Runtime baseline `ef59ae49`; the probe/test setup was added
before changing runtime behavior. The paired device-free probe uses the public
GUI-origin captured ordinary/Piano/group APIs, real control admission, preparation
worker, scheduler and mixer. It paces output callbacks rather than freezing the
timeline while workers run. This excludes the physical input/UI event pump,
device/driver and acoustic path. Single-PAD cases have two loaded PADs; group
cases actually render 2/4/6 voices. No inference/background-job load was added.

Each side covers 30 ordinary cases (48 kHz dense; 44.1/96 kHz representatives;
512-frame and 127/383/257/769 irregular partitions), six Piano cases and twelve
2/4/6-PAD groups. One observation per declared configuration is a pilot, not
p95/p99 evidence. Key Lock ON/OFF and r=.8/1/1.16 controls are included. The
slowed/faster cases use BPM Lock with a 120 BPM master. "Cold" means no prepared
selected endpoint, not a cold process; "late" deliberately publishes one 512-frame
callback after capture. All 48 cases applied before and after; none were omitted.

The fixture has one 1 kHz burst at the selected beginning with a 4 ms exponential
decay. First output uses an absolute 1e-7 threshold, significant output uses 10%
of peak, and the declared principal burst landmark is the centre of the strongest
1 ms energy window. This independent synthetic landmark is not general perceptual
or musical acceptance. Matched dry rendering retains the same source/rate/silence;
all output samples, including pre-ringing, remain in the measurements.

| Ready selected case, T=0 | Before E / significant / principal (output frames) | After E / significant / principal |
| --- | --- | --- |
| Ordinary r=.8, 48 kHz | 8192 / 11019 / 11183 | 0 / 2827 / 2991 |
| Ordinary r=1.16, 48 kHz | 8192 / 10947 / 11059 | 0 / 2755 / 2867 |
| Ordinary r=1, Key Lock ON | 0 / 1 / 24 | 0 / 1 / 24 |
| Ordinary r=.8, publication at 512 | 8704 / 11531 / 11695 | 512 / 3339 / 3503 |
| Ordinary r=.8, 44.1 kHz, irregular | 8192 / 11044 / 11172 | 0 / 2852 / 2980 |
| Ordinary r=.8, 96 kHz, irregular | 8192 / 13201 / 13247 | 0 / 5009 / 5055 |
| Piano r=.8, 48 kHz | 8192 / 11019 / 11183 | 0 / 2827 / 2991 |
| Piano r=1.16, 48 kHz | 8192 / 10947 / 11059 | 0 / 2755 / 2867 |

At 48 kHz this removes 170.667 ms of selected-ready scheduling cost. The slowed
significant/principal response remains 58.896/62.313 ms, faster 57.396/59.729 ms:
the 30 ms goal is **unmet**. The slowed first tiny output is already at 22.458 ms;
it cannot pass the later principal attack. Matched dry principal is 30 frames at
both nonunity rates, leaving 61.688/59.104 ms wet-minus-dry displacement. This
does not establish a slower-only regression or explain every reported symptom.

For r=.8 with 960 source frames of authored silence, the dry principal is 1230 output
frames. Late warm wet principal changes 12852->4660, with E changing 8704->512. Subtracting
the dry landmark and actual E leaves 2918 frames/60.792 ms excess; silence is retained.
The final ordinary probe records host render-block time spans separately: the
ready slowed significant/principal block was produced at 53.608–53.942 ms elapsed,
while its audio-timeline principal is 62.313 ms. Across 30 ordinary cases, maximum
pacing lateness was 676 us and harness-inclusive render/drain time 1309 us, with zero
harness overruns. These include test allocation/PCM collection and are not a CPAL
callback deadline, device latency or acoustic measurement. Baseline host spans
were not collected, so no paired host-timing speedup is claimed.

Warm groups with exactly one wet member and dry/unity peers share E=512 after
one-block-late publication, versus 8704 before, at 2/4/6 voices. No-ready and two-wet
controls retain 8704; a single reserve cannot serve two mutable Native owners.
Group mix landmarks may be dominated by dry peers; per-wet principal attack is
not accepted from those mix values. Cold/unselected, refill/rapid misses and
multiple-wet scheduling still need step 3, together with the actual DSP attack.

The implemented fix keeps phase normalization in reserve comparison copies only.
An exact silent PhysicalBirth with no actual/retained feed offset predates any
performer launch coordinates. Its complete fresh action request replaces the
cache request at transfer; general source/history/adoption equality stays strict.
Past-T ordinary hits retain the source beginning at E=max(T,callback). One-wet
groups capture every fresh request before consuming the single reserve and keep
the existing atomic effect. Future quantized T is never accelerated. Quantize OFF
adds no musical 1/64/minimum interval or coalescing; four wet originals 16 frames
apart remain distinct in regression, with actual worker costs still explicit.

Focused verification reuses 101 compatible ordinary/endpoint/group passes and
includes a final 30-pass ready-clock/captured-global run (five existing codec/controller
integration tests remain ignored). Exact initial PCM matches independent worker
owners; existing raw-Native physical-birth/partition tests retain their limits.
The new original recording test proves S=E at 0/512, Rust callback allocation
guard, all 1024 spooled output-frame bits and actual writer join/RT release.
Another 165 Python controller/opaque Native-boundary checks pass with verified
external Release Native/API. Paced diagnostics pass all three explicit tests.
No App/device/hearing, background
stem-generation or whole quality acceptance is inferred. The plan now has nine
steps so readiness and a justified prepared-attack DSP change remain explicit.

Run the paced probe through the configured workspace adapter's `rust-test` action
with `-p flitzis-looper --lib paced_production_ --release -- --ignored --nocapture --test-threads=1`.
The test source is
[`response_probe.rs`](../../../rust/crates/looper/src/audio_engine/ordinary_endpoint_reserve_tests/response_probe.rs).

## STEP3 implementation boundary

Early worker completion can accelerate ordinary FirstSoundAtGrid physical births
and playback-only atomic groups. Retain the admitted fallback deadline until all
members are ready; do not put an unfinished worker into the scheduler's single
deferred-due cell. Reuse the existing event in place with its original input order.
Before changing E, recapture the complete action request/permit from the current
source, validate the retained physical beginning, render, source controls and DSP,
then retain the displaced request for off-callback retirement. Mutable Native/FIFO
state stays unique and unconsumed. General exact/history/adoption matching stays
strict. Future T, earlier pending originals and callback capacity remain fences.

This is a one-time acceleration before the admitted fallback deadline. Recording
groups, Piano, worker results after that deadline and principal-attack DSP require
separate follow-up proof. The full remaining plan is preserved and split further;
no scheduling-only result passes the 30 ms principal-attack goal.

### STEP3 paired result

The unchanged paced production probe was run before runtime edits at `51017d49`
and after this implementation: 30 ordinary, six Piano and 12 actual 2/4/6-PAD group
configurations per side, all 48 applied. Reference host: Windows x64, AMD Ryzen 9
9955HX3D (16 cores/32 logical processors), rustc 1.95.0, Release, Rubber Band 4.0.0.
There is one observation per configuration. "Cold" here means no selected ready
Native reserve with source PCM already resident; it is not cold disk loading.

| Ordinary case at 48 kHz, no selected reserve | Before E / significant / principal (frames) | After E / significant / principal |
| --- | --- | --- |
| r=.8, admission at T=0 | 8192 / 11019 / 11183 | 512 / 3339 / 3503 |
| r=.8, publication one callback late | 8704 / 11531 / 11695 | 1024 / 3851 / 4015 |
| r=1.16, admission at T=0 | 8192 / 10947 / 11059 | 512 / 3267 / 3379 |
| r=1.16, publication one callback late | 8704 / 11459 / 11571 | 1024 / 3779 / 3891 |

These cases remove 7680 frames/160 ms of scheduling wait. Cold slowed principal
response is now 3503 frames/72.979 ms; faster is 3379/70.396 ms. Their principal
delay after E remains 2991/62.313 ms and 2867/59.729 ms respectively. The 30 ms
principal-attack goal remains **unmet**, including the unchanged selected-ready
controls. No DSP attack compensation or output trimming was introduced.

At 2/4/6 PADs, cold one-/two-wet groups and warm two-wet groups change common E
from 8704 to 1024; selected-ready one-wet groups stay at 512. Every member, including
dry peers, retains its beginning. Group mix landmarks can still hide wet attack.
Piano, dry/unity, selected-ready ordinary, and the existing 44.1/96 kHz/irregular/
authored-silence controls retain their prior frames. Cold early-completion timing
at those other sample rates is not measured by this matrix.

Across the 30 ordinary observations per side, maximum measured harness drain/render
span is 1262 us before and 1183 us after, with zero observed harness block overruns.
These include fixture overhead and are not a CPAL deadline distribution or a
statistically established speedup. Event-pump, device, acoustic, background-stem,
p99 and whole-quality acceptance remain separate.

Five deterministic completion tests cover a genuinely held worker, unrelated STOP
progress, full independent raw-Native PCM, four 16-frame-apart originals, 2/4/6
groups including mixed wet/dry, future T, stale source/control guards and feedback
pressure followed by real legacy reprepare from the fresh request. Rust allocation
guards cover both ordinary and six-member completion callbacks; they do not count
Native C++/FFT allocations. The strict initial-interval gate leaves later replans
unchanged. Pending predecessors, including terminal owners awaiting old-slot
cleanup, can still delay subsequent originals; this remains follow-up work.

The impacted Rust regression supplies 148 compatible passes, followed by the
corrected 17-case public controller closure (16 passes plus one focused restore
pass). Three paced diagnostics pass explicitly; the separate Native reserve
feasibility probe remains ignored. Initial failures exposed stale test assumptions:
waiting until fallback E consumed early PCM, an older ordinary oracle used periodic
prehistory/catchup, and legacy controller fixtures omitted real content/material
identities. Those fixtures now observe actual E without emitting a prefix, use
silent physical-birth history, and retain strict public identity validation.
Live-history oracles, negative source/ABA tests and complete PCM/EQ/owner assertions
remain intact. No ABI, dependency, installer, App/device or inference change.

Another 183 Python controller/opaque-Native-boundary checks pass; two existing
real-device cases are skipped without `--audio-devices`. External Release Native/
API verification passes with the existing 22 library warnings. Changed-file
Rust formatting, strict OpenSpec, six Markdown files/63 local targets/three anchors
and whitespace checks pass. Systems PRE/frozen/affected, Methods and the single
independent final-diff review chain cover the implementation and fixture repairs.

## STEP4 terminal-predecessor result

Only an owner conclusively marked rejected stops being an ordering barrier.
Its old event, slot, worker and heavy source/Native owners remain reserved until
normal cleanup. Live or missing owners remain barriers; this also means that
other members of an older group remain barriers until their own terminal state
is established. No preparation deadline, request comparator or DSP is weakened.

The targeted public ordinary-capture fixture reuses the 48 kHz single burst and
reference host/profile above, with source PCM resident and no selected reserve.
The older prepared original is cancelled, then a distinct original is captured
at T=16. Both sides admit fallback E=8208 and observe readiness at the frozen
software coordinate 16, before rendering any PCM. There is one observation per
ratio per side. The residual host wait starts after publication/drain; it is
neither complete worker runtime nor input-to-ready latency. Its small difference
is not a measured readiness speedup.

| Rate | Residual readiness wait before / after (us) | Actual E before / after | Significant / principal from T before | Significant / principal from T after |
| --- | --- | --- | --- | --- |
| .8 | 2954 / 2848 | 8208 / 16 | 11019 / 11183 | 2827 / 2991 |
| 1.16 | 3996 / 2968 | 8208 / 16 | 10947 / 11059 | 2755 / 2867 |

This removes 8192 frames/170.667 ms of scheduling wait in this cancellation seam.
Principal response remains 2991 frames/62.313 ms and 2867/59.729 ms respectively:
the 30 ms goal is **unmet**. No DSP, device, acoustic, background-load or p99 gain
is inferred. The STEP3 48-case paced matrix is retained rather than repeated.
Run `completed_cancelled_predecessor_response` explicitly through the adapter's
`rust-test` action with `--release -- --ignored --nocapture --test-threads=1`;
the source is the existing `response_probe.rs` linked above.

Three deterministic regressions cover returned rejected ordinary owners at equal
T and 16-frame gaps, genuinely worker-held ordinary/Piano owners with independent
dry Piano successors, and cancelled single/group-member predecessors at 2/4/6 PADs.
They retain complete independent PCM/EQ comparisons across old queued cleanup,
source/request identity, exact start counts and reserved worker/slot capacity.
Rust callback allocation guards do not count Native C++/FFT allocations.

Broader regression exposed six failing legacy cases reproduced on untouched
STEP3 `e8e0725f`. Generation-boundary fixtures now settle the real MultiLoop OFF
collapse while silent, then create two genuine explicit nonexclusive ordinary
setup voices before the tested exclusive Piano action. Mixed ordinary/Piano
fixtures hold the actual worker until their intended E=T or T+1 race. Three
playback-group fixtures observe common actual E with an empty callback before
any PCM, retaining the admitted fallback assertion separately. All generation,
peer, source/OwnACK, Native, full PCM/EQ, first-wet history/FIFO and local failure
assertions remain. The new held-worker fixture also adopts genuine SourceACK
before testing the production feedback ring. An empty callback is a real due
observation, so the mixed race adds none before releasing its worker at E.

The resulting regular Rust closure has 258 unique passing cases: 166 compatible
earlier passes plus 92 final focused cases. This covers 71 ordinary, 102 pitch
runtime, 25 captured-group, 31 scheduler and 29 endpoint cases. The corrected
groups all pass; five existing controller cases remain ignored by default.
The pitch runtime's existing ignored gate and the endpoint diagnostics retain
their explicit status; the new cancellation diagnostic passed on both sides.
Another 31 Python capture/cancellation/MIDI tests pass with verified external
Release Native/API. Release retains the existing 22 library warnings. Changed
Rust formatting, strict OpenSpec, local links and whitespace are checked
separately. No App, device, inference or listening session was started.

Piano/recording/late-result completion remains STEP5; principal-attack DSP is
STEP6. The remaining stem-loading, conversion, budgeting, PCM/providers, native
inference/Torch parity and combined regression work remains explicit in the
twelve-step plan. Existing quality, listening and distribution gates stay open.

## STEP5 Piano completion result

Constant independent FirstSoundAtGrid Piano SetRetrigger births now use the same
one-shot initial-interval completion boundary. The mixer captures a whole fresh
request, current action-owned permit and DSP intent at E=max(original T, callback),
checks exact retained playback and source/render identity, then retains the old
request in Work before retargeting the same ordered scheduler event. No Native
reset, feed consumption, source trimming or callback preparation is introduced.
SET, AttackAtGrid, gliding/history-bound births and legacy replans remain excluded.
Recording common S/E/STOP and remaining completion/order work is STEP6; principal
DSP is STEP7. All later work remains in the thirteen-step plan.

The unchanged six-case `paced_production_piano_response` probe ran before runtime
edits at `a2c2d348` and after this implementation, on the reference host/profile
above. Each configuration applied on both sides, n=1/configuration. Two PADs are
loaded; source PCM is resident. Cold means no selected ready Native reserve,
not cold disk or model loading. T=0, callbacks are paced in 512-frame partitions.

| Cold Piano at 48 kHz | Before E / significant / principal (frames from T) | After E / significant / principal |
| --- | --- | --- |
| r=.8, Key Lock ON | 8192 / 11019 / 11183 | 512 / 3339 / 3503 |
| r=1.16, Key Lock ON | 8192 / 10947 / 11059 | 512 / 3267 / 3379 |

This removes 7680 frames/160 ms of scheduling wait. Principal response is now
72.979/70.396 ms from T; after E it remains 62.313/59.729 ms. The 30 ms goal is
**unmet**. Warm reserve controls keep E=0 and principal=2991/2867; both unity
controls keep E=0 and principal=24. No DSP attack improvement is claimed.

Readiness is separate: deterministic real-worker tests observe returned work at
the frozen coordinate 512, then effect at 512 without consuming any prior source
sample. That is a completion/ordering proof, not an input-to-ready duration.
The paired paced probe did not timestamp worker completion, so no readiness-speed
gain or paired input-to-ready latency is inferred. Admission, T, E and complete
rendered landmarks are retained. The unchanged ordinary/group paced matrix is
reused. Other-rate cold Piano timing, background overlap, input event-pump, CPAL,
device/acoustic, distributions and whole-quality acceptance remain open.

Four deterministic regressions cover real unfinished work/unrelated STOP,
complete independent raw-Native PCM, Key Lock OFF with nonzero pitch, four equal
and 16-frame originals, future quantized T, cancellation/source/control ABA and
feedback pressure followed by real legacy reprepare from the fresh request.
Rust callback allocation observations exclude Native C++/FFT allocations.

Affected legacy fixtures now observe actual E before any PCM, or hold a real
worker at the intended boundary. Prepared-return gates retain the genuinely
prepared Work/Native on the worker until release; they never block the callback.
Hold old-PCM windows, T/T+1 races, independent master references, exact complete
PCM/EQ/FIFO/source/lifetime/OwnACK assertions and failure guards remain intact.
The internal 64-claim fixture still proves simultaneous distinct ready Native
owners, the 65th refusal, actual retirement and full independent PCM. Its positive
one-frame effect batches retain every original and exact ordered content tags;
immutable admitted E=T+8192 is recorded separately from actual E.

The regular Rust closure has 282 distinct passing cases: 106 pitch runtime,
71 ordinary, 29 endpoint, 25 captured-group, 31 scheduler and 20 pitch-action
unit cases, reusing source-compatible successes between the focused final runs.
The final corrected ordinary/endpoint/request/completion run passes all 124 cases;
the earlier six Hold/glide/rate failures are closed. Another 159 Python Piano/
pitch/controller checks pass with verified external Release Native/API and the
existing 22 library warnings. Five capture-controller cases remain ignored by
default; Native diagnostics and the separate feasibility probe retain their
limits. The six-case Piano diagnostic was explicitly run on both sides.
Systems, Methods and one independent final-diff review cover this coherent step;
formatting, strict OpenSpec, local navigation and whitespace are checked at delivery.
No ABI, dependency, installer, App/device or inference change is introduced.

## STEP6 initial recording completion result

Attached recording groups now validate their exact pending capture/session before
and after planning every fresh member request. Only after every bind succeeds does
the callback retarget the same event and update pending capture S to common E.
OriginalOrder, reserved capacity and the original admitted STOP remain unchanged.
Final member/capture claims still precede any audio stores. A pre-start cancelled
capture cannot revive; whole-group capacity failure remains terminal. Late genuine
reprepare and live original/nonterminal group-peer ordering remain step 7 of 14.

The paired `completed_recording_response` diagnostic uses the public captured
GLOBAL/capture route on the reference Windows/Ryzen/Release host above. Both PADs
are independent wet members with the same single-burst source and rate; dry peers
cannot mask the wet landmark. Source PCM is resident, with no selected reserve
consumption or background inference. A real worker is held while output advances
silently to frame 512, then released while the software clock stays frozen there.
Both sides admit E=8192 and observe returned readiness at frame 512,
n=1/ratio/side. This is
targeted scheduling/attack evidence, not paced input-to-ready or device latency.

| Two-wet recording group, 48 kHz, T=0 | Before E / significant / principal (frames from T) | After E / significant / principal |
| --- | --- | --- |
| r=.8, Key Lock ON | 8192 / 11019 / 11183 | 512 / 3339 / 3503 |
| r=1.16, Key Lock ON | 8192 / 10947 / 11059 | 512 / 3267 / 3379 |

Capture first-frame equals all-member E on both sides, with all 24000 recorded
sample bits matching the rendered output. This removes 7680 frames/160 ms of
scheduling wait. Principal response is 72.979/70.396 ms from T; after E it remains
62.313/59.729 ms. The 30 ms goal is **unmet**. First tiny output changes 9268->1588
and 9277->1597; it does not pass the late principal attack. Residual host wait
from worker-gate release to observed readiness is 4469/5031 us before and 4624/4520 us
after. It excludes earlier queue/preparation/input time and supplies no measured
readiness speedup. DSP, CPAL, event-pump, device/acoustic, background, distributions,
other-rate recording timing and whole-quality acceptance remain unproved.

Four deterministic regressions cover genuine partial readiness at 2/4/6 PADs with
mixed/two/all-wet members and both nonunity directions, unrelated STOP progress,
independent Native identities, complete 24000-frame raw-Native/dry references,
bit-identical spool data and actual RT release/writer join. They also verify
equal-E/short recording STOP, the original later STOP at full scheduler capacity,
public same-target STOP idempotence/different-target refusal, raw duplicate STOP,
held cancellation, stale source/control ABA and whole-group feedback rejection.
Rust allocation guards exclude C++/FFT allocations. The existing 48000-frame
recording oracle retains actual contiguous pre-E samples and its complete Native,
spectral, continuation, source/lifetime and spool assertions while observing E
before rendering. Existing future-T, atomic claim and late-STOP fences still pass.

The current regular Rust closure supplies 71 distinct successes: 29 captured-group,
31 scheduler, 7 ordinary-completion and 4 Piano-completion cases. The combined 70 passes
are reused with the corrected STOP case's focused final pass. Another 215 unchanged
STEP5 cases remain compatible, giving 286 distinct regular checks including the four
new regressions. The paired diagnostic passes explicitly on both sides; five
existing controller integrations remain ignored. Another 138 Python controller/recording
checks pass with verified external Release Native/API; these controller/opaque
boundary checks do not substitute for Native/audio proof. Release retains 22 known
library warnings. No ABI, dependency, installer, App/device or inference change.

Run the paired diagnostic through the actual-checkout adapter's `rust-test` with
`-p flitzis-looper --lib --release -- completed_recording_response --ignored --nocapture --test-threads=1`.
Regular focused filters are `captured_global_start`, `scheduler::` and
`early_completion_tests`; the source is
[`early_completion_tests.rs`](../../../rust/crates/looper/src/audio_engine/captured_global_start_tests/atomic_capture_tests/early_completion_tests.rs).
Systems PRE/frozen/affected, Methods and one independent final-diff review cover
this step. Formatting, strict OpenSpec and local navigation are checked separately.

## STEP7 genuine legacy completion result

Successful legacy reprepare now produces the exact E-bound receipt described
above for ordinary/Piano and playback/recording groups. A previously completed
bundle cannot rearm itself: only the real off-RT worker resets its original
physical beginning, finishes guarded preparation and releases displaced pins.
All fresh-request, source/DSP/OwnACK, original-order, capacity and retirement
checks remain; live predecessors and deferred group-owned Work remain strict.
The separate live-original/old-group-peer step 8 retains that remaining work.

The explicit `late_reprepared_recording_response` diagnostic uses the public
two-wet captured GLOBAL/recording route, resident single-burst PCM and the same
48 kHz reference Windows/Release host above. It holds the first real worker past
admitted E=8192, returns it at frame 8193, observes genuine legacy reprepare at
E=16385, then holds the second worker while rendering silence to 8705. Readiness
returns while the software clock is frozen at 8705; completion moves E there.
There is one observation per ratio, no selected-reserve consumption or background
load. These are observed plans and effects within one run, not a paired empirical
before/after host-latency or readiness-speed comparison.

| Rate | Legacy planned E / observed ready frame / actual E | First / significant / principal from original T | Principal after actual E | Residual second release-to-ready (us) |
| --- | --- | --- | --- | --- |
| .8 | 16385 / 8705 / 8705 | 9781 / 11532 / 11696 | 2991 | 996 |
| 1.16 | 16385 / 8705 / 8705 | 9790 / 11460 / 11572 | 2867 | 982 |

This retarget removes 7680 frames/160 ms of remaining scheduling wait after the
real reset. Principal response after E remains 62.313/59.729 ms; the 30 ms goal
is **unmet**. Original-T response includes the deliberate worker holds. Residual
host time excludes the first preparation, earlier input/queue time and everything
before the second release. It cannot establish a compute/readiness speedup, input
latency distribution, DSP improvement, CPAL/device/acoustic latency, background
headroom or whole-quality acceptance. Capture first-frame equals actual E and
all 24000 spool sample bits equal rendered PCM. Other-rate late recording timing
remains unmeasured.

Six added regular regressions cover genuine second workers, identical reused
Native handles, full independent physical PCM in both directions and Key Lock OFF
with nonzero Piano pitch; returned cancellation/source/control fences; and all
2/4/6-PAD mixed/all-wet recording members with full PCM and exact spool/STOP.
Ordinary/Piano feedback tests now perform two real reset/completion cycles.
The recording STOP is admitted before completion at 32705 and retained as S moves
from 16385 to 8705. Existing equal/short/immutable STOP, future-T, terminal capacity,
source/OwnACK and last-reader proofs remain compatible and pass. Rust callback
allocation observations exclude C++/FFT allocations.

The regular closure has 292 distinct passing cases: 290 successful integrated
cases reused plus two corrected legacy fixtures passing in the focused final run.
Those two fixtures had rendered toward saved fallback E and discarded real
earlier PCM. State-driven empty callbacks now observe ordered effects first,
advance only to a freshly queried scheduler target with all participating workers
returned, then retain every sample from actual E. All eight originals, original T,
planned-span bound, Native/request/FIFO/EQ and complete source-phase comparisons
remain. These empty observations are ownership proofs, not paced throughput or
device evidence. Twelve default ignored cases retain their limits; the new
diagnostic passed explicitly. Systems PRE/frozen/affected, Methods and the single
independent final-diff review chain cover the change. Source formatting, strict
OpenSpec, local links and whitespace pass. Another 90 Python captured/MIDI/
recording-controller checks pass with verified external Release Native/API;
these opaque/control checks do not establish Native/audio acceptance. Release
retains the known 22 library warnings. No ABI, dependency, installer, App,
device or inference change is introduced. Run the new diagnostic through the
actual-checkout adapter's `rust-test` with `-p flitzis-looper --lib --release -- late_reprepared_recording_response --ignored --nocapture --test-threads=1`.

## STEP8 terminal atomic-group peer result

Polling now propagates an actual rejected member through its exact retained
atomic group using the existing peer rejection CAS. A lost CAS remains live
until a later successful retry. Live originals, missing owners, epochs and
deadlines provide no terminal proof. No epoch, queued event, slot, Work, worker,
Native/source pin, capacity or capture custody is released by this propagation.
Every member still claims before any group audio store; applying/applied owners
remain protected. Recording uses its existing failure and retirement lifecycle.
Live-original and group-held deferred-Work optimization remains step 9 of 16.

The identical `rejected_group_peer_response` public captured-route diagnostic
ran before runtime changes at STEP7 `ec9cd178` and after this implementation,
using the reference Windows/Release host above. Two wet members hold resident
single-burst PCM, with no selected reserve or background work. One older member
is cancelled; the later ordinary start uses the OTHER, formerly live peer's
content. Both sides admit E=8208, T=16 and observe returned readiness with the
software clock frozen at 16, n=1/ratio/side. Landmarks below are PCM-local offsets
from T=16; add 16 for absolute output frames. No initial samples are removed.

| Rate | Actual E before / after | First / significant / principal from T before | First / significant / principal from T after | Residual post-publication readiness wait before / after (us) |
| --- | --- | --- | --- | --- |
| .8 | 8208 / 16 | 9270 / 11019 / 11183 | 1078 / 2827 / 2991 | 4535 / 4355 |
| 1.16 | 8208 / 16 | 9285 / 10947 / 11059 | 1093 / 2755 / 2867 | 3015 / 3005 |

This removes 8192 frames/170.667 ms of scheduling wait in this terminal-peer
seam. Principal attack after E remains 2991/62.313 ms and 2867/59.729 ms: the
30 ms goal is **unmet**. Residual readiness observation starts after publication,
excluding prior input, queue and preparation time. Its small difference is not
a readiness/compute speedup. There is no DSP, CPAL/device/acoustic, background,
p99, other-rate peer-latency or whole-quality acceptance claim. The earlier
ordinary/Piano/group matrices retain their own independent limits.

Five new regular regressions cover actual different-peer and whole-group
successors at 2/4/6 PADs in both rate directions, real held old workers with an
independent dry Piano successor, full 24000-frame raw-Native/dry PCM, distinct
Native owners, source/request identity, retained old capacity and Rust callback
allocations. Three of those tests isolate CAS loss, missing/epoch-only metadata
and genuine live-group ordering. An existing recording cancellation matrix also
now cancels an individual held member and proves no audio/spool prefix.
Rust allocation observations do not cover Native C++/FFT allocations.

There are 282 distinct regular Rust successes: 279 successful integrated cases
reused with the three corrected private fixtures' final passes. The integrated
invocation initially failed those three fixtures because a dry group at immediate
T already effects during admission's due drain. The corrected real Grid16 setup
at frame 17000 keeps original T=24000 pending, retaining every CAS/metadata/order
assertion. The held unity-Piano fixture's earlier Native-unity oracle was also
incorrect for dry bypass; its final comparison is bit-exact against all 24000
resident source samples, with the resident start explicitly checked. No product
contract or guard was weakened. The final four-case invocation passes all three
corrected regulars and the explicit diagnostic. Thirteen default ignored cases
retain their status; the new diagnostic ran explicitly, while the other twelve
retain their previous limits. No ABI, dependency, installer, App/device or
inference change is introduced.

Another 110 Python captured/MIDI/recording-control checks and three verification
checks pass (113 total): changed five-file Rust formatting, strict OpenSpec and
six Markdown files/64 local targets/three anchors. These control/opaque-boundary
checks do not replace Native/audio evidence. External Release Native/API passes,
retaining the 22 known library warnings and existing einops warning. Systems
PRE/frozen/affected, Methods and one independent final-diff review chain cover
this step; whitespace is checked before publication.

Run the diagnostic through the actual-checkout adapter's `rust-test` with
`-p flitzis-looper --lib --release -- rejected_group_peer_response --ignored --nocapture --test-threads=1`.
The focused regular filter is `group_peer`; broader compatible filters are
`ordinary_start_tests`, `pitch_runtime_tests`, `captured_global_start`,
`scheduler::`, `ordinary_endpoint_reserve_tests` and `pad_start::`.

## STEP9 ordered live-original result

Completion visits the fixed 64-slot custody in captured (T, sequence) order.
Only a successful whole fresh bind and exact same-event retarget creates a
current-pass receipt. A later original requires that exact predecessor header,
sequence and E, the actual retained scheduler order, and checked captured
spacing. Every group member must bind before its header supplies any receipt.
The final preceding-endpoint barrier is unchanged: a predecessor's transient
failure, replan or deferred effect cannot be overtaken. Unknown, busy, unbound
and genuinely deferred Work remain strict; no Native reset, heavy callback
prepare, early retirement or fallback/replan-budget change was added.

The identical `live_original_chain_response` public ordinary-capture probe ran
before runtime edits at `a373ff28` and after the change on 2026-10-31, using the
reference Windows/Release host above. Source PCM is resident, no selected reserve
is consumed, and there is no background inference. Four distinct originals have
T=0/16/32/48. A real worker is held until frame 64, then released while the
software clock stays frozen at 64 until all work returns. Both sides admit
E=8192/8208/8224/8240. Rendering then uses 512-frame partitions, n=1/ratio/side.

| Rate | Actual effects before | Actual effects after | Last attack first / significant / principal after its own E, both sides | Residual release-to-ready before / after (us) |
| --- | --- | --- | --- | --- |
| .8 | 64 / 576 / 1088 / 1600 | 64 / 80 / 96 / 112 | 1078 / 2827 / 2991 | 7880 / 8377 |
| 1.16 | 64 / 576 / 1088 / 1600 | 64 / 80 / 96 / 112 | 1093 / 2755 / 2867 | 7892 / 7956 |

All four originals apply on both sides. Their captured 16-frame gaps now survive
inside one callback rather than accumulating a 512-frame wait for each effect.
The second/third/fourth effects remove 496/992/1488 frames of scheduling wait;
the fourth saves 31 ms at 48 kHz. Equal-T originals retain zero captured gap and
their separate ordered effects; no musical minimum or coalescing is introduced.
Residual host readiness excludes time before release and does not demonstrate a
compute or input-to-ready speedup. Principal delay after the last E remains
62.313/59.729 ms; the 30 ms goal is **unmet**. No DSP, device/acoustic, CPAL/p99,
background, other-rate chain timing or whole-quality acceptance is inferred.

Nine new regular regressions cover ordinary/Piano captured gaps, equal T,
chronological slot reuse, exact receipt/order/metadata and overflow negatives,
a genuinely held wet predecessor with an independently ready dry Piano follower,
final feedback deferral, and old-group peer/header mapping at 2/4/6 PADs in both
directions. Complete piecewise PCM retains intermediate rendered prefixes and
the final retrigger's full 24000-frame continuation against independent Native
references. Simultaneously held Native owners and final request identities stay
distinct. Recording preserves common S/E=80, immutable STOP=24080, independent
160/320 Hz sources, all 24000 spool sample bits and actual writer/RT settlement.
Rust callback allocation checks exclude C++/FFT allocations. The focused source is
[`live_original_tests.rs`](../../../rust/crates/looper/src/audio_engine/ordinary_start_tests/live_original_tests.rs)
and [`completion_tests.rs`](../../../rust/crates/looper/src/audio_engine/pitch_runtime/completion_tests.rs).

The final regular closure has 311 distinct successes: 309 source-compatible
integrated passes plus the two corrected legacy fixtures' final targeted passes.
All nine new regulars pass in that integrated input. Fourteen cases remain ignored
by default; the new paired diagnostic ran explicitly, and the earlier thirteen
retain their independent limits. The new recording oracle initially assumed two
160 Hz sources; the actual fixture has 160/320 Hz, now both independently verified.
One legacy equal-T fixture had required one effect per callback. It now retains
all four E=0 effects, four simultaneous Native owners, exact starts/generations,
request/FIFO/history and the complete final 48000-frame Native reference.
The 64-owner GUI/MIDI custody harness had treated a bound-but-pending E as the old
admitted E. It now proves monotone E in [T, admitted E), one captured-gap-preserving
shift per content, every actual scheduler cut and positive one-frame effect.
All intervening PCM, exact feedback, independent Native/source/EQ/FIFO checks,
64-owner/65th-refusal and off-RT retirement remain; all sixteen child rows pass.
This is ownership/PCM pressure evidence, not a 64-owner performance workload.

Another 155 Python captured/MIDI/recording-control and opaque API checks pass with
verified external Release Native/API. They do not replace Native/audio proof.
Release retains the 22 known library warnings and the existing einops warning.
There is no ABI, dependency, installer, App/device or inference change. Systems
PRE/frozen/affected, Methods and one independent final-diff review chain cover
this step. Changed-source formatting, strict OpenSpec and local navigation are
checked separately before publication. Run `live_original_chain_response` through
the actual-checkout adapter's `rust-test` with `--release -- --ignored --nocapture --test-threads=1`.
The affected regular filters are `ordinary_start_tests`, `pitch_runtime_tests`,
`pitch_runtime::`, `ordinary_endpoint_reserve_tests`, `captured_global_start`,
`scheduler::`, `pad_start::` and `pitch_actions::`.

The full plan now has seventeen steps. Genuinely deferred group-owned Work remains
step 10 and needs its own later-E prepared-state/request/CAS proof, followed by
principal DSP and every existing stem/conversion/budget/provider/native-inference
and final-regression duty. No later-E reuse is inferred from earlier-E completion.

## STEP10 held deferred-group result

The later-E path described above now reuses a wholly retained, unconsumed
FirstSound group at its actual retry frame. The existing no-predecessor gate
remains unchanged. Every member gets a complete fresh request/permit/DSP envelope;
all plans and capture fences precede binding, and only all-bind success moves
common E/S. Each member consumes one existing replan allowance. A later member
bind lost to cancellation rejects the whole group without an audible or recorded prefix.
Already displaced pins, missing proof and unsupported states keep genuine worker
reprepare. A runtime occupancy check also prevents accidental pin replacement.

Eight new regular regressions prove actual prior group-owned custody, no extra
worker roundtrip, unchanged independent Native identities, full 24000-frame
physical PCM at 2/4/6 PADs with mixed/all-wet members and both .8/1.16 rates.
Recording retains common S/E=8193, original STOP=32193, all 24000 spool sample
bits and writer/RT settlement. E exactly equal to original STOP=8193 preserves
the ordered group effect and empty recording. Earlier STOP/cancellation,
source/control/epoch staleness, missing or wrong metadata, delay/replan bounds
and a later member's lost bind cannot publish a prefix. Occupied displaced pins
demonstrably require the real blocked worker again. Existing live-original,
stem, history, OwnACK, capacity and retirement regressions remain authoritative.
Rust callback allocation checks do not count C++/FFT allocations.

The identical `deferred_group_response` diagnostic ran before the runtime edits
at STEP9 `85185140` and after this change. The public two-wet recording route
uses resident burst PCM at 48 kHz with no selected reserve or background work,
n=1/ratio/side, on the Windows/Release reference host above. A real worker is held
while silence advances to 8192; first readiness returns with that clock frozen.
A genuine publication marker refuses the due event at 8192, leaving all Work in
the group and the same event deferred. The retry starts at 8193. Subsequent worker
waits also freeze the software clock on the baseline side.

| Rate | Actual E before / after | First / significant / principal after E, both sides | Residual first release-to-ready before / after (us) |
| --- | --- | --- | --- |
| .8 | 8193 / 8193 | 1076 / 2827 / 2991 | 5950 / 4929 |
| 1.16 | 8193 / 8193 | 1085 / 2755 / 2867 | 5282 / 4945 |

Both sides apply and record all 24000 sample bits correctly. With the clock
frozen during waits, this probe establishes no scheduling-frame reduction. The
regular test holds any additional worker while the new path applies immediately,
proving removal of the redundant real preparation roundtrip. No paired duration
of that removed roundtrip was measured. The table's residual readiness starts
at the FIRST release, excluding earlier input/queue/preparation, and cannot prove
a compute/readiness speedup or headroom distribution. Principal DSP remains
2991/2867 frames (62.313/59.729 ms); the Human's advisory 30 ms reference is not
a mandatory gate. No CPAL/device/acoustic, background, p99, other-rate deferred
timing or whole-quality acceptance is inferred. This bounded change does not
claim technical optimality; step 11 assesses practical DSP benefit versus complexity.

The final integrated Rust closure passes 325 regular cases with zero failures;
15 default ignored cases retain their limits, including the new diagnostic run
explicitly on both sides. The earlier 20-case focused success is overlapping
evidence, not an additional count. The first Native compile caught the new
module method's private visibility; `pub(super)` corrected it, and subsequent
Release Native/API and tests passed without weakening a guard. Release retains
the existing 22 library warnings and existing einops warning. No ABI, dependency,
installer, agent App/device or inference session changes are introduced.

Run the explicit probe through the actual-checkout adapter's `rust-test` with
`-p flitzis-looper --lib --release -- deferred_group_response --ignored --nocapture --test-threads=1`.
The focused filter is `deferred_`; the integrated filters are
`ordinary_start_tests pitch_runtime_tests pitch_runtime:: ordinary_endpoint_reserve_tests captured_global_start scheduler:: pad_start:: pitch_actions:: deferred_`.
The new custody tests are
[`deferred_group_tests.rs`](../../../rust/crates/looper/src/audio_engine/pitch_runtime/deferred_group_tests.rs).
Another 280 Python captured-input/MIDI/Piano/group/recording-control and opaque
Native/API checks pass with verified external Release Native/API. They do not
replace audio evidence. Systems PRE/frozen/affected, Methods and one independent
final-diff review chain cover this step. Eight changed Rust files, strict OpenSpec
and six Markdown files/68 local targets/three anchors pass the static checks;
whitespace is checked before publication. Principal DSP and all other work remain
steps 11–17 under the advisory, proportionate-complexity latency standard.

## STEP11 practical principal-attack assessment

Decision on 2026-10-31: retain LiveShifter Short + ChannelsTogether and silent
physical-birth preparation. The alternative window is slower in the measured
cases; nominal-delay trimming loses genuine response energy. A new source-prefed
birth needs a wider ownership/control change for a limited packing-only gain.
Under the Human's advisory 30 ms reference and proportionate-complexity instruction,
that change is not justified in this step. This bounded decision does not prove
that no different DSP design could improve latency.

The opt-in `practical_attack_profile_assessment` uses the existing independent
direct-C-ABI RawNative oracle, not the production adapter, scheduler or detector.
It records 36 finite uncropped captures: 44.1/48/96 kHz × .8/1/1.16 source rates ×
Short/Medium × zero/20 ms authored SOURCE silence. Pitch is 1/rate. Source input
is a declared 1 kHz, 4 ms exponential burst with independent integer-sample linear
interpolation and stereo levels 1/-.73. Negative input is strictly zero. The
32768-frame preroll and at least half-second positive capture retain every
captured output sample; no samples within that window are cropped or shifted.
One observation per configuration uses the Windows x64/Ryzen/Release/Rubber Band
4.0 reference host. This stereo fixture differs from the earlier mono production
burst, so their frame landmarks are not interchangeable.

First output uses max-channel amplitude >1e-7; significant output uses 10% of
that response's peak. Principal is the center of the strongest
floor(sample_rate/1000)-frame pooled stereo-energy window, approximately 1 ms.
Values below are relative to independently declared physical source zero:
raw audio coordinates, not production E or host readiness timestamps.

| Sample rate / source rate, no authored silence | Dry principal | Short first / significant / principal | Medium principal | Short nominal delay | Energy omitted by nominal-delay prefix cut, Short |
| --- | --- | --- | --- | --- | --- |
| 44100 / .8 | 27 | 573 / 2342 / 2470 | 2983 | 2371 | 5.348% |
| 44100 / 1.16 | 27 | 576 / 2241 / 2350 | 2856 | 2348 | 45.555% |
| 48000 / .8 | 30 | 567 / 2317 / 2481 | 2985 | 2371 | 6.050% |
| 48000 / 1.16 | 30 | 582 / 2244 / 2356 | 2859 | 2348 | 19.452% |
| 96000 / .8 | 59 | 610 / 4498 / 4544 | 5674 | 4419 | 0.028% |
| 96000 / 1.16 | 59 | 594 / 4617 / 4663 | 5768 | 4723 | 42.352% |

Medium's principal is later in all 18 pairs, including unity and authored silence.
At 48 kHz, Short/Medium raw unity principal is 2166/2678; productive unity bypasses
Native and retains the earlier dry principal of 24. Raw unity is not a production
latency regression. Authored output wait is 20 ms/rate for the source-silence case
and stays in both capture and dry reference. The last 20 ms contains at most
6.225e-69 of total response energy across the captures, supporting the finite
denominator, not infinite tail retention or general musical quality. The
hypothetical cut is exactly
[capture start, physical zero + nominal delay); its fraction uses the SAME full
uncropped response energy. At 96 kHz/1.16 Short it also removes the principal
window's center. That flag checks the center, not the whole window or raw peak.
No candidate cut is applied to playback.

There is a distinct, potentially valid adapter-only opportunity: consume its
initial B-1=511 silence frames off-RT while feeding 511 actual source frames,
retaining the complete raw response. Removing only that packing lead would save
11.587/10.646/5.323 ms at 44.1/48/96 kHz. These are arithmetic bounds for that
opportunity, not measured gains or bounds on all possible DSP designs. It leaves
logical source zero and a different actual input endpoint/FIFO. Current
`PhysicalBirth` rejects declared cuts/catchup/DSP; reserve/completion paths refuse
actual or retained feed. Accepting that state therefore needs a new physical-birth
contract and complete-source/output proof, fresh ordinary/Piano/group completion,
live pitch/rate and stem changes, current permits/OwnACK, cancellation and actual-feed
last-reader retirement. Existing SourceHistory replay evidence does not establish
those semantics. Keep these guards rather than trade fast completion for unproved
prefeed. This is the concrete cost/benefit reason for retaining the current runtime.

Readiness, scheduling and DSP remain separate. Compatible production STEP2–10
evidence is reused: actual E and full PCM/spool guards remain as recorded; 48 kHz
principal after E remains 62.313/59.729 ms for .8/1.16. Frozen-clock residual
release-to-ready observations exclude earlier input/queue/prepare time. No new
input-to-ready, host preparation, p99 or callback-headroom speedup is measured here.
There is no new production DSP, ABI, dependency, installer or device change.
Unchanged full physical-birth ordinary/Piano/group and retained-feed continuation
checks are the proportional regression. Whole quality, background overlap,
listening/device and distribution gates remain open. Step 12 implements the newly
requested approximate BPM fallback; stem loading follows in step 13. Approximate
status must stay explicit rather than masquerade as precise timing acceptance.

Run through the actual-checkout adapter's `rust-test` with
`-p flitzis-looper --lib --release -- practical_attack_profile_assessment --ignored --nocapture --test-threads=1`.
The source is
[`key_lock_physical_onset_tests.rs`](../../../rust/crates/looper/src/audio_engine/key_lock_physical_onset_tests.rs).
Focused regular filters are `key_lock_physical_onset_tests`,
`key_lock_tonal_onset_tests`, `physical_birth_history_tests` and
`prepared_native_history::retained_start::tests`. The explicit diagnostic passes
all 36 rows; nine regular checks pass (six physical/tonal/production-birth checks
and three retained-feed continuation/bounds checks). The new diagnostic stays
opt-in by default. The initial retained-file-name filter matched no tests; the
three actual test names were subsequently run and passed. Changed-file Rust
formatting, strict OpenSpec and the affected Markdown local targets/anchors
pass. A cast line wrap was corrected after the first formatting check; no runtime
guard or assertion was weakened. External Release Native/API verifies with the
existing 22 library warnings and einops warning. Systems PRE/frozen POST, Methods and one independent
final-diff review accompany the assessment; formatting, strict OpenSpec and local
navigation are checked separately. Historical quality failures are not converted
into successes by this diagnostic passing.

## Measurement contract

Record original monotonic input capture, control admission/publication, requested
output target T, actual effect E, first significant rendered output and principal
attack against a matched dry/source reference. Keep authored silence and genuine
source attack position; do not trim audio to improve a latency number. Report
first nonzero and relative-threshold onset separately so pre-ringing cannot pass
as a responsive attack. Declare each fixture's authored onset/silence. Record
rejected/cancelled starts and missing or ambiguous attacks.

The advisory reference is <=30 ms input-to-significant-rendered-output response for
ready, quantization-OFF starts on a declared reference machine/profile at 2/4/6
PADs, with attack displacement reported separately and no lost beginning or audible
dry/wet substitution. Quantization-ON reports intentional grid wait separately,
then excess lateness relative to T. Device/driver/input transport buffering and
physical loopback are separate components, never silently subtracted or claimed
from a software result. Cold/refused/unavailable domains remain explicit and are
not dropped from aggregate reports to make a warm result look universal.

For fixtures with an authored attack at the beginning, compare principal-attack
response with the same advisory 30 ms reference. For later authored attacks,
compare displacement against the matched dry/source reference. Significant output
alone cannot establish principal-attack timing. Report both landmarks without
turning the reference into a mandatory gate; ambiguous or missing measurements
remain unresolved/failed evidence.

Use 44.1/48/96 kHz where supported, actual and irregular callback partitions,
rates 1, 0.8 and 1.16 plus existing 0.5/0.75/1.5/2 DSP cases, Key Lock ON/OFF and
BPM Lock ON/OFF. Include ordinary PAD/Piano retrigger, START/STOP, selected and
unselected PADs, cold/warm/rapid repeats, atomic groups, pause/resume, loops,
one-shot, Intro/Tail, full mix and selected stems. Add Stop/cancel, source or stem
replacement, rate/pitch changes and queue/reserve pressure where touched.

Compare no background work, inference, conversion, component loading and mixed
jobs. Report sample counts, p50/p95/p99/max capture-to-effect/onset and callback
durations, callback deadline misses, FIFO underflows, reserve hits/refusals,
worker queue waits, CPU usage, working set/private commit, native owners/handles
and repeated-cycle plateaus. Stress at most 10 PADs; 2/4/6 is the productive
target. Internal 32-owner correctness is not a performance workload.

Reuse `examples/key_lock_latency_probe.rs` for Native/adapter diagnostics and
existing production ordinary-start, endpoint, group, onset and source-history
tests. Extend the missing captured ordinary end-to-end route rather than invent
a second scheduler. Reuse `c3_process_metrics.rs` for host counters; do not rerun
the historical 200-PAD loader program as audio acceptance. Native/FFT allocations
are outside the existing Rust allocation counter's scope.

## Proposed implementation boundaries

### Responsive starts

Prefer preparation before a performer press and bounded exact-source readiness.
First measure the two-phase ordinary path and selected reserve rejection for
past-T inputs. A safe immediate late birth must retain its physical beginning,
fresh effect-time checks and one common E for atomic groups. Never just reduce
8192 or move preparation, reset, allocation, blocking or file reads into realtime.

If warmed resources still fail the attack target, evaluate source-bound prepared
attack material or another CPU DSP design using the same production-route
measurements and retained quality gates. Share only immutable data; each attack
owns its mutable Native/FIFO/filter state. Existing isolated backend experiments
are evidence, not an accepted backend or authority to bypass source history.

### Independent stem preparation and controls

Separate per-content admission/reservation from slow I/O, then revalidate source,
placement, intent and memory under short publication locks. Merely detaching a
blocking wrapper frees Python but can leave the shared control producer blocked;
the test must prove unrelated trigger and STOP progress during an injected read.
Retain same-PAD serialization, OwnACK, cancellation, candidate identity, pending
promises, shared reuse and outgoing/history last readers.

### Detached Rust conversion

Port the bounded model-output conversion path, keeping Python orchestration.
Keep Rust domain types, cancellation/progress and processing logic independent
of PyO3; expose thin validated bindings rather than Python objects inside the
core. Reuse the native logic from Rust tests and future native orchestration.
Keep common logic portable across Windows, Linux and macOS. Put process-tree
control, device/backend integration and OS-specific resource observations behind
platform adapters/conditional modules. Use typed paths and standard portable file
semantics; do not spread Windows DLL/path/priority assumptions into the core.
CPU inference/rendering is the portable baseline; CUDA use is hardware-conditional.
Preserve binary64 resampling/channel mapping, exact PCM16 MIN/endpoints and
ties-even rounding, nonfinite handling, ordered intermediate clipping of the
instrumental sum, RIFF/RF64 and output geometry. Retain exclusive private output,
cooperative cancellation, failure cleanup, provenance/fingerprints, complete hash
verification and descriptor-last eligibility. Version processing identity when
the executed implementation changes even if tested bytes are equal.

### Background resources and robustness

Coordinate separator, BLAS/Torch, FFmpeg, conversion and analysis concurrency
against available host resources, reserving audio/control headroom. Apply child
settings before native libraries initialize and verify effective settings; do
not mutate global App numerical settings. Keep process-tree cancellation and
same-model CPU fallback. If bounded overlap cannot meet callback deadlines,
defer/gate heavy generation during playback with truthful pending UI and specs;
do not silently stop active PADs. The Human explicitly accepts a robust gated
policy over unreliable concurrent generation.
Fallback must not disguise cancellation or malformed/corrupt input as a CUDA
device failure; preserve those terminal outcomes.

### Memory, multicore and CUDA decisions

Measure unique PCM versus process/native/model memory, preparation bursts, sharing
and final-reader release. Consider bounded independent preparation lanes or
native batch/SIMD work only where callback tails and memory improve. Do not
parallelize merely to occupy every core. Keep one time authority and deterministic
effects. Offline GPU additions require transfer-inclusive benefit, bounded VRAM,
actual CUDA execution and a verified CPU path; retain a usable CPU-only install.

The Human additionally authorizes as much sensible Rust migration as useful for
performance, dependency footprint and eventual Windows EXE distribution. Audit
actual PyTorch model/operator use and replacement seams rather than assume a
language port removes the tensor runtime. A replacement must preserve model
weights/processing identity, numerical/quality behavior, device receipts, CPU
fallback, cancellation and distribution/recovery. Compare package size, startup,
RAM, inference speed and maintenance cost. Keep further migration easy, but do
not delete PyTorch or promise a smaller/faster package before those proofs.
Future packaging targets Windows EXE as well as Linux and macOS distributions.
Review native audio/inference libraries, CPU architectures and provider availability
on each OS; distinguish actual platform build/run evidence from design intent.
Do not infer Linux/macOS support from Windows-only tests or require CUDA
installation on unsupported/CPU-only systems.
Keep GPU selection capability/provider-based rather than hard-code NVIDIA versus
CPU. Investigate AMD-capable providers and implement a practical equivalent path
when operator support, packaging, quality and transfer-inclusive performance fit.
Provider availability depends on OS/architecture/GPU/driver; absence or provider
device failure must retain CPU fallback without hiding corrupt input/cancellation.
The official ONNX Runtime [DirectML](https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html)
and [MIGraphX](https://onnxruntime.ai/docs/execution-providers/MIGraphX-ExecutionProvider.html)
provider references are candidate starting points, not proof our pinned runtime
or models support them. Record actual AMD execution separately from mocked tests;
do not require a new GPU runtime in the common CPU-only installation.

The final Human refinement also includes Intel/AMD integrated GPUs and automatic
background choice of the best practical execution path for the user's hardware.
Discover actual adapters/provider/model/operator capabilities outside realtime,
distinguishing discrete and integrated/shared-memory devices. Compare end-to-end
model/frontend/transfers/synchronization cost, memory/VRAM/shared-RAM pressure and
concurrent playback deadlines. Keep CPU as a candidate even when a GPU exists;
the preferred provider can differ by model/workload and available headroom.

Any calibration is bounded, off the audio/control hot path and must not start an
uncontrolled competing inference load during performance. Reuse compatible
measurements keyed to hardware/driver/runtime/model identities and revalidate
capabilities after changes. Missing libraries, unsupported operators, allocation
or device failure must produce a usable CPU route with truthful device/fallback
receipts. Do not retry corruption/cancellation as provider failure, oscillate
between devices during a job or treat successful provider creation as proof of
actual acceleration. A safe initial CPU path may run while optional acceleration
is evaluated. User-facing controls should remain simple; diagnostics identify
the selected route and reason. Test capability detection, ranking, model-specific
fallback and CPU-only installs without claiming real iGPU performance from mocks.
The Human explicitly prioritizes proportionate engineering: weigh measured gains
against implementation/validation effort, installer size, drivers, maintenance,
quality and stability. Reject or defer marginal/high-cost provider additions;
automatic selection need not guarantee acceleration on every GPU. Prefer a robust
CPU path when extra backend complexity does not produce worthwhile benefit.
The final clarification makes NVIDIA CUDA a mandatory supported acceleration
path, including after any PyTorch replacement. Do not remove or defer CUDA on
the optional-provider cost/benefit rationale. That discretion applies to extra
AMD/Intel/iGPU paths. CPU-only systems retain CPU operation without a mandatory
CUDA installation. Validate actual CUDA model execution/failure-to-CPU behavior
on compatible hardware; do not equate an available provider with that evidence.

The follow-up request makes practical PyTorch removal an explicit investigation
and implementation target, conditional on feasibility and retaining at least
current functionality, model quality and interactions. KeyNet already uses Rust
CQT plus ONNX Runtime; its offline Torch export is not productive inference.
Stem separation and the separate BeatThis workers still use full Torch model
pipelines. A LibTorch binding would not eliminate that runtime. Investigate
replacement artifacts/operators using the existing native inference pattern;
preserve exact frontend/chunk/tail/overlap semantics, pinned-weight provenance,
CPU and available CUDA execution/fallback, resource limits and quality evidence.
Remove runtime/dependency closures only after parity and distribution checks;
split substantial model migrations into coherent chats rather than weaken the
no-regression condition. If replacement is not practical, retain the functioning
path and record the concrete operator, quality or packaging barrier.

Primary upstream context, checked during this audit:
[Rubber Band LiveShifter](https://www.breakfastquay.com/rubberband/code-doc/classRubberBand_1_1RubberBandLiveShifter.html)
documents configuration-dependent processing delay and per-instance thread safety;
[PyTorch CPU oversubscription](https://docs.pytorch.org/docs/2.15/notes/multiprocessing.html#cpu-in-multiprocessing)
and [thread initialization](https://docs.pytorch.org/docs/2.15/generated/torch.set_num_threads.html)
support setting child budgets before work. These explain mechanisms, not measured
application results or an instruction to change pinned dependencies.

## Verification and architectural records

Each implementation step updates the affected current spec/module, preserves the
single clock, immutable source/content identity, independent PAD/voice state,
effect-time validation, OwnACK and off-callback last-reader retirement. Review
GUI/native MIDI, load/restore/persistence, Hold/pause/EOF, failure/stale/ABA and
shared-resource interactions proportionally. Portable review accompanies actual
ABI/dependency/runtime-path changes; no pin or toolchain update is assumed.

Use bounded Systems PRE/frozen POST, Methods review and one independent final
actual-diff review. Root serializes writes/Git/build/install. Run focused checks
for each coherent change and the combined matrix at the final boundary; reuse
unchanged compatible evidence. Protect running Apps, held Native and private
audio/settings. Headless checks do not establish listening/device acceptance.

## STEP17: bounded provider selection and measured retirement

Reuse immutable scoped PCM backing/reader accounting, the existing bounded pools,
STEP16 shared heavy admission, half-allowance child promise and original host floors.
Measure actual private commit, working set, page faults and process/reader tails
separately from unique PCM; neither independent peaks nor free physical RAM explain
the reported commit-pressure cause. No callback parallelism or new pool is planned.

Both separator adapters share bounded passive CPU/CUDA cost policy: at most 32
exact source/model/configuration/complete-producer/runtime/driver/device keys,
ten-minute expiry, verified complete model-attempt-through-artifact costs, and a 15% CPU
advantage before changing the cold CUDA preference. Actual observed CUDA allocator
peak plus 256 MiB device margin may veto CUDA using fresh queried VRAM. This is
not total VRAM enforcement or a second host ledger; existing host/playback guards
remain authoritative. No extra calibration inference/thread or persistent cache.
Unknown source/producer/driver/UUID stays uncalibrated. In particular the existing
full-host validator UNKNOWN still disables producer-bound learning; it is not
bypassed to advertise automatic optimization. New roles remain in full attestation.
Explicit CPU never queries/initializes CUDA; a recent queried identity may label
its passive observation, while every later Auto plan freshly queries identity and
headroom. First CPU without a recent identity remains uncalibrated. The common
decision/preflight cost is excluded consistently from both route totals; model
launch/import/load, inference, transfers, outputs and actual tree/reader/artifact
settlement are included. This is passive calibration, not paired quality evidence.
Candidate data remains serializable and owns no selector, reader or lock. Only
the original controller's final positive source/producer verification, successful
`finish_read`/resource closure and uncancelled non-playback outcome promote it to
the captured backend. A normally demoted UNKNOWN result cannot train preferences.
Repeated promotion retains its original timestamp and cannot renew the TTL.

The disposable query binds Torch/toolkit, OS/architecture, actual device UUID,
CUDA API capability and the installed graphics-driver release. The latter comes
from [NVML's driver query](https://docs.nvidia.com/deploy/nvml-api/latest/api/group__nvmlSystemQueries.html);
`cuDriverGetVersion` alone would only identify supported CUDA API capability.
Missing driver/UUID information leaves calibration unknown, while a tentative
available CUDA route can still execute. No NVML Python package is added.

Only a narrow recognized CUDA placement/transfer/inference fault emits the dedicated
failure exit/marker. Model, source, decoding, PCM, encoding, cancellation and host/
playback failures remain terminal. Completed execution receipts still require real
successful model execution; query availability and policy plans grant no OwnACK.
Its marker uses the existing independently bounded stdout-receipt channel and
dedicated exit 70; readable stderr compaction cannot erase it. Duplicate or
oversized receipts remain invalid, including when diagnostics exceed the tail.

### Scoped verification and complete-process observations

The selected nine Python/vendor files passed 187 tests. A final overlapping
31-test selection passed: 27 provider/controller checks plus four static/full-host
checks, including strict mypy on 11 product files, Ruff/format on 17 Python files,
strict OpenSpec and then-current Markdown links/whitespace.
The final documentation-only check passed on five Markdown files and 46 local
targets, with whitespace clean; anchors were not separately rescored.
Both complete manifests remain UNKNOWN at the ORIGINAL
`StemExecution._coherent_fallback` validator; new
provider/resource roles are retained and need honest final attestation. Earlier
unsupported scalar-expression/default-factory declarations were represented with
equivalent literals and explicit timestamps, without changing the verifier.
The first combined vendor collection collided on `tests`; importlib collection
fixed the invocation. Initial lint/format failures are retained in the logs.

Five existing small Native tests passed across two fresh-process selections,
with original stack settings and 602 byte-equal Rust/Cargo/embedded-Python/probe
inputs. The reused test executable SHA256 is
`5cc2f0b0ec906676c34aba481b7b123d63c5da7ea661331088ecf0b23161c8ad`.
This is compatible component/test-source evidence, not compiler/image or whole
producer attestation. One embedded-Python attempt initially lacked the private
environment's site-packages; matching the existing Rust wrapper fixed that setup.

| Exact Native leaf | Wall s, n=1 | Samples | Owned-tree private peak bytes | Concurrent observer + tree private peak bytes | Concurrent working-set peak bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| `concurrent_components_share_backings_and_keep_independent_own_ack_and_reservations` | 0.8277832 | 28 | 304304128 | 397705216 | 425795584 |
| `cancelled_component_gate_waiter_retires_before_slow_reader_returns` | 0.7923033 | 27 | 304148480 | 397549568 | 425226240 |
| `cancelled_real_source_reader_releases_promises_only_after_worker_return` | 1.0416305 | 35 | 592613376 | 686014464 | 697692160 |
| `conversion_cancel_keeps_real_worker_source_and_processing_until_native_return` | 2.9191317 | 95 | 656220160 | 751235072 | 769093632 |
| `rejected_created_material_retires_only_after_last_native_and_actual_marker_reader_return` | 0.4389963 | 15 | 8122368 | 103137280 | 136110080 |

The first three use real component/source holds, independent OwnACKs and four
shared immutable Arc backings at two PADs; no additional mutable voice sharing.
Cancellation preserves actual worker/reader promises until return. The fourth
joins the actual Python pool and reclaims all 34 admission permits; the fifth
checks physical deletion only after the real Native/marker reader ends. All five
owned scopes/trees/readers retired with zero pending scopes and no sampling errors.
Observer private bytes returned to their exact per-process baseline in these
short attempts; this does not establish a production memory plateau or leak bound.
Root test-process cumulative faults were respectively 76736, 76614, 143768, 163543
and 5420; these combine soft and hard faults, not measured pagefile traffic.
Host caller/system counters are sequential observer-context samples. Concurrent
sampled sums are distinct from individual lifetime peaks; no independent peaks
were added. Existing PCM/pool/callback architecture is retained: these observations
do not justify another cache, pool, callback parallelism or broad port by themselves.

### Actual CUDA and same-model failure-to-CPU

A generated one-second 44100-Hz stereo PCM16 source, SHA256
`2fb0179c14bb7b968c700b2c3adad9991f1071f517107f2e064562436a76a06d`, ran through
the real Demucs htdemucs adapter on the RTX 5090 Laptop with Torch 2.12.0+cu130.
The actual driver/UUID query produced a complete identity, with 24275582976 free
device bytes. Successful CUDA inference reported numerical pools 1/1 and a
968884224-byte Torch reserved-allocation peak. This is not total device memory.

A separate disposable CUDA child applied Torch's local allocator fraction 0.001
before the same model placement/inference: actual 2-MiB allocation failed with
24.44 MiB allowed on the 23.86-GiB device. The original marker/exit traversed the
owned command and actual adapter, then a fresh child completed the SAME model,
source and quality configuration on CPU with a true fallback receipt and pools
1/1. No fake exception or OS/driver/pagefile setting change was used. Five complete
output WAVs on each route had the required geometry and passed full verification.

| Scenario, n=1 | Complete backend wall s | Observed owned private peak bytes | Concurrent observer + tree private peak bytes | Concurrent working-set peak bytes | Closed scopes |
| --- | ---: | ---: | ---: | ---: | ---: |
| Successful CUDA | 33.8149413 | 3467931648 | 3543519232 | 1756336128 | 5/5 |
| Actual CUDA failure followed by CPU | 26.1269543 | 2214486016 | 2290876416 | 1743724544 | 6/6 |

These wall spans include preflight/query and complete artifacts; the fallback
span also includes its failed CUDA attempt. They are NOT comparable CPU/CUDA
speed samples, inference-kernel timings or calibration training. Minimum observed
system commit headroom was 10483814400/11836661760 bytes. All tree/reader scopes
closed; observer private bytes rose 864256/839680 bytes in the two attempts, which
neither proves a leak nor attributes the historical pressure cause. Uncontrolled
Demucs random shifts confound the five output differences (maximum PCM16 code
differences 53/495/67/1606/1124 in vocals/drums/bass/melody/instrumental order).
No device numerical equivalence, musical/audio-quality acceptance or improvement
is inferred. No extra NN run is needed merely to improve these numbers.

### Additional-provider decision and remaining gates

The observed AMD 610M is hardware presence, without AMD inference evidence.
[DirectML](https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html)
is a practical Windows DX12 Intel/AMD route, requiring matching ONNX operators,
sequential execution, disabled memory patterns and one Run per session; new Windows
deployment development favors [WinML](https://onnxruntime.ai/docs/get-started/with-windows.html).
[OpenVINO AUTO](https://docs.openvino.ai/2026/openvino-workflow/running-inference/inference-devices-and-modes/auto-device-selection.html)
can choose Intel devices and CPU startup, but its precision/batching policy does
not supply this product's source/quality/cost/headroom proof. BS currently uses
STFT/ISTFT, complex tensors and attention, with no ready ONNX model artifact.
Adding these providers now would require exported-model/operator/precision parity,
pinned native closures, installer/recovery and actual hardware validation before
the STEP18 inference boundary. That cost is disproportionate for this delivery;
no measured iGPU slowdown or universal incompatibility is claimed.

The [released ROCm matrix](https://rocm.docs.amd.com/en/latest/compatibility/compatibility-matrix.html)
does not list this 610M/9955HX3D configuration. Development
[TheRock targets](https://github.com/ROCm/TheRock/blob/main/SUPPORTED_GPUS.md)
are not release/driver/model acceptance. No Intel hardware execution is available
in this result. UMA/shared RAM remains the existing host commit/physical domain,
never an additional free GPU allowance. Extra AMD/Intel providers are therefore
deferred with these concrete export/package/parity costs; CPU fallback stays.
NVIDIA CUDA is mandatory and is not included in that discretion.

KeyNet keeps its existing model/runtime and local thread limits. Its ORT rc12
[builder defaults](https://raw.githubusercontent.com/pykeio/ort/v2.0.0-rc.12/src/session/builder/mod.rs)
include an efficiency policy; lack of GPU Cargo features alone is not attested
effective CPU-provider execution. No new effective KeyNet EP claim is made.
STEP18 retains practical independent native inference/Torch removal with full
parity and no LibTorch relabel. FINAL19 retains full-host attestation, all exact
32 parent failures/watchdog/registry unknowns, quality, BS/BeatThis CUDA and real
fallback, post-replacement CUDA, CPU-only installation, platform distribution and
combined 2/4/6-PAD/<=10 stress/overlap gates. Playback gating is not overlap proof.

Reconstruction uses the existing routed test entry with the actual writer,
the exact five Native leaves above (resolve their full names from `--list`), the
existing `stem_batch_neural_native_probe._OwnedProcessObservation` and original
Windows DLL/embedded-Python environment. Hardware runs use the stock captured
Demucs CLI options and same source; only the failure child gets its local Torch
allocator quota. Logs `performance-step17-{python-second,static-final,static-memory,
static-memory-final,cuda,format-final,docs-final}.log` preserve the precise successes and setup
failures. The scoped runs overlap; they are not a new clean whole-product matrix.

## STEP16: coordinated background resource admission

The existing stem pool, six admission lanes and single analysis owner remain
authoritative. One shared heavy resource permit spans a stem backend through
actual source/processing `finish_read`, including child trees, output readers and
conversion. A separate original analysis admission retains duplicate refusal;
its admitted supervisor waits cancellably for heavy capacity. Ordinary resource
contention cannot select approximate BPM. Same-thread, still-active nested scopes
reuse the permit; copied or expired contexts cannot borrow another owner's slot.
The optional prior uses the same permit. Recording finalization keeps its existing
independent owner so a take does not wait behind separator inference.

Child-only environments bound numerical pools before imports. Separator children
set and verify Torch intra/inter-op counts of one before model work, recording
the observed counts only in completed execution receipts. FFmpeg decoder, encoder
and filter pools are bounded explicitly. The particular KeyNet ORT session uses
one intra/inter-op thread without changing global ORT or App numerical settings.
KeyNet's existing parallel analysis branch remains separate; this is no physical
core-isolation or total-process-thread-count claim.

On Windows, the shared Native host ledger grants a dynamic child commit ceiling
before suspended Job activation. Let `A` be the minimum caller/system-commit and
physical allowance after the original stem floors, and `P` all existing Native
prospective promises. The ceiling is `(A - P) / 2`, with checked/saturating extents
and refusal below the minimum useful admission. The full ceiling remains promised
until the tree and both pipe readers finish, while actual child memory is also
visible to the OS. Halving preserves room for new resident/control work even when
the child reaches its entire ceiling; promising all of `A - P` incorrectly starved
new cold loads under this deliberate double charge. Original 2 GiB admission,
3 GiB recovery, 512 MiB critical commit and physical floors remain unchanged.
This is an enforced resource budget, not a model-memory estimate or protection
against unrelated processes consuming later headroom.

New inference displays **Waiting for playback to stop** while the existing active
PAD projection is nonempty. Saved-set lookup and component preparation keep their
own lanes. Playback observed during generation ends that attempt with a distinct
playback-headroom error after actual teardown; it never stops a PAD. Polling and
abort/retirement have a tail. No reliable live inference overlap or device deadline
is certified, so heavy generation is conservatively gated. A resource waiter
reports **Waiting for background resources**, without an effective execution claim.

Portable preactivation now requires the matching Native child-budget API. The
canonical budget type, conversion type and compiled bridge/host-ledger sources
participate in component identity. This does not establish full binary/compiler
attestation. The unchanged full-host validator policy still refuses complete
producer identity at `StemExecution._coherent_fallback`; all producer roles,
including the new resource helpers, must pass the FINAL19 attestation gate.
Windows Job enforcement is not Linux/macOS host-monitoring or POSIX whole-tree
retirement evidence. No dependency, lock, model, ABI version or OS setting changed.

### Bounded CPU-model observations

Exactly one serial CPU configuration pair per pinned separator used a deterministic
one-second, 44.1 kHz stereo source, seeds 1234 and unchanged model/default chunk
options. All four input byte hashes were
`c0b59a584e817b9c36375e236c3db4b1066e544734e1b5d585028238bb1f2cb6`.
Torch getters verified configurations 16 then one. **Both inherited the current
`OMP_THREAD_LIMIT=1` and one-thread MKL/BLAS environment.** These are not actual
16-way parallel execution, an unrestricted parent baseline or a general speedup.

| Phase | Demucs configured 16 -> 1, wall / CPU seconds | BS-RoFormer configured 16 -> 1, wall / CPU seconds |
|---|---|---|
| Import | 3.234161 / 2.968750 -> 3.151916 / 2.703125 | 3.195252 / 2.718750 -> 3.169446 / 2.750000 |
| Model load | 0.642269 / 0.562500 -> 0.631187 / 0.578125 | 1.445437 / 1.312500 -> 1.369220 / 1.359375 |
| Inference | 4.219265 / 4.171875 -> 3.516783 / 3.500000 | 55.291579 / 55.140625 -> 55.420762 / 55.265625 |
| Raw output copy | 0.003812 / 0.015625 -> 0.003902 / 0 | Included in the preceding phase |

BS inference includes its existing PCM16 output. The unchanged STEP15 Rust
conversion observations remain separate. Process peak-pagefile counters were
2,695,356,416 -> 2,221,436,928 bytes for Demucs and 3,626,786,816 -> 3,620,900,864
for BS. Peak working sets were 1,551,056,896 -> 1,634,082,816 bytes for Demucs
(an increase) and 2,951,340,032 -> 2,945,544,192 for BS. These high-water counters
are cumulative within one child, not phase allocations or peaks to add across
models. The diagnostic's `native_live_bytes=0` is not App Native accounting.

Demucs raw float hashes differ: maximum absolute sample difference
`1.1920928955078125e-7`, largest component RMS `2.0159592071840294e-8`.
Demucs PCM16 identity was not measured. All four BS WAV hashes match, with zero
changed PCM16 codes among 88,200 samples per component. No tolerance was chosen
after observing these results. CUDA availability was queried only; these CPU
observations establish no CUDA execution/fallback, listening, general quality,
live-overlap, source-second throughput or historical Portable-pressure cause.
System commitment returned near preceding observations, but unrelated activity
prevents leak or causal attribution.

These model observations preceded the final half-allowance correction. Reuse
their numerical/phase evidence only. Final admission proof is provided separately
by full-ceiling accounting cases and the allocated-child cold-control regression.

Reconstruction: create 44,100 float64 time points divided by 44,100; left is
`0.2*sin(2*pi*220*t) + 0.1*sin(2*pi*431*t)`, converted to float32, right is reversed
left times 0.7. Set Python/Torch seeds to 1234. In isolated diagnostic children,
set/verify Torch counts 16 or one before loading the existing checkpoint. Demucs
uses its standard mono-reference mean/std normalization, default shifts/overlap,
split mode and zero extra workers; restore mean/std and retain four raw float
components. BS uses its pinned configuration and default `ChunkPlan` through
`separate_pcm`, retaining four complete PCM16 WAVs. Run serially through the
existing command scope/guard, sample per-phase CPU/wall and the retained child's
Windows counters plus timestamped caller/system/physical observations, and compare
complete output hashes, maximum/RMS differences and source geometry. Do not rerun
inference merely to improve a number.

### Verification boundary

The cold-control regression holds an owned, committed and touched 2 GiB child
under the same test binary's real budget ledger. It then performs a fresh Native
full-source cold load, public captured START, actual admission/original T, applied
callback state, all 64 stereo frame bits against an independent dry reference,
public STOP and actual child/reader/ledger retirement. This is headless manual
callback correctness, not full-ceiling physical allocation or CPAL latency.
Deterministic physical/caller/system-limiting cases separately cover allocation
of the full ceiling plus a fresh 256 MiB Native promise.

Initial probe failures retained their real failed outcomes: the embedded Rust
test executable was mistaken for Python, original T was observed before callback
admission, and the initial finite-window fixture mismatched the public full-source
request. The final fixture uses the configured interpreter and actual matching
source/admission boundaries. No production source/window/timing guard or assertion
was relaxed. The original 32 Rust failures, distinct watchdog and registry unknown,
full-producer UNKNOWN and mandatory CPU/CUDA/distribution/combined FINAL19 gates
remain open. STEP17-19 scope is retained in full.

Affected validation is deliberately scoped. The initial 15 Python files had
448 passes and two new fixture/resume failures, corrected with 82 passes across
five affected files. A later selection had 371 passes and five failures: strict
Torch module typing and four stock-batch fixtures whose process recorder rejected
the new commit-limit keyword before launch. The recorder now forwards the real
limit without weakening any custody assertion; its two affected files passed all
25 cases. Both numerical entry points pass the four exact typed Torch functions,
retaining the same set/set/get/get checks. The final selected run had 72 passes,
including actual stock entrypoints, batch cancellation, budget and processing
identity cases, and one Ruff type-only import error; that import was moved into
the existing supported type-checking convention. These overlapping runs are not
additive or a clean whole-program matrix.

The final ten static/runtime checks passed in 18.70 seconds after that correction:
Rustfmt/Ruff/format, strict typing of eight affected product files, strict OpenSpec,
seven Markdown files with 88 local targets, canonical Native/budget and Portable
gates, the original full-host UNKNOWN boundary, source-equal model receipts and
effective numerical runtime observations. No broader repeat was justified after
these directly affected checks passed.

Native evidence comprises five final ledger cases, thirteen KeyNet cases and the
one allocated-child cold-control case, with `RUST_MIN_STACK` absent.
The ledger was freshly executed from the existing Debug test artifact only after
byte-comparing all 594 tracked Rust/Cargo/lock and included Python probe inputs
against its compiled source stage. Its executable SHA256 was
`5cc2f0b0ec906676c34aba481b7b123d63c5da7ea661331088ecf0b23161c8ad`.
This is source-compatible test reuse, not full compiler/image attestation. The
KeyNet local single-thread session selected class 7 against an independently built
default-pool session; maximum logit difference was `1.1920928955078125e-7`.
Current child observations also report ATen intra/inter-op, OpenMP and MKL pools
all at one. No App-global pool was changed and no additional model run is inferred.
