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
final duty in steps 13–18. STEP11 itself introduced no such runtime change.

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
steps; BPM fallback does not fix it. Steps 13–18 retain their complete scope.

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
| Python postprocessing | `controller/stem_artifacts.py`: `_read_span`, `_align_block`, `_payload`, `_instrumental_payload`; both generation backends | Bounded 4096-frame blocks still perform per-sample Python work in App worker threads. Port the arithmetic/streaming path under a detached Rust boundary; measure GUI/control progress as well as throughput. |
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
