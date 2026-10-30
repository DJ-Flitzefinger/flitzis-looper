# Performance audit and implementation design

## Initial audit boundary

Baseline: `69a8098d84ae58d616f988f5662e6038afdcf0e0`, inspected 2026-10-30.
The first step is a source audit and plan, not a fresh benchmark or runtime fix.
The reported direction is specifically slowed non-master PADs with BPM Lock and
Key Lock; faster PADs and the master are controls. Generation, postprocessing,
component loading and live adoption must be measured separately.

## Source findings

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

## Measurement contract

Record original monotonic input capture, control admission/publication, requested
output target T, actual effect E, first significant rendered output and principal
attack against a matched dry/source reference. Keep authored silence and genuine
source attack position; do not trim audio to improve a latency number. Report
first nonzero and relative-threshold onset separately so pre-ringing cannot pass
as a responsive attack. Declare each fixture's authored onset/silence. Record
rejected/cancelled starts and missing or ambiguous attacks.

The engineering goal is <=30 ms input-to-significant-rendered-output response for
ready, quantization-OFF starts on a declared reference machine/profile at 2/4/6
PADs, with attack displacement reported separately and no lost beginning or audible
dry/wet substitution. Quantization-ON reports intentional grid wait separately,
then excess lateness relative to T. Device/driver/input transport buffering and
physical loopback are separate components, never silently subtracted or claimed
from a software result. Cold/refused/unavailable domains remain explicit and are
not dropped from aggregate reports to make a warm result look universal.

For fixtures with an authored attack at the beginning, evaluate principal-attack
response against the same 30 ms goal. For later authored attacks, compare their
displacement against the matched dry/source reference. Significant output alone
cannot pass the target if the principal attack remains late; report that case
as unmet, and ambiguous or missing attacks as unresolved/failed evidence.

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
