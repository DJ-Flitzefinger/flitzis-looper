# Performance audit and implementation design

## STEP40 result: current shared-job playback participants

An obsolete subscriber can retain its physical job and source readers after its
PAD's source or content changes. The previous playback fence treated that PAD as
a current participant solely because its interest had not yet retired. Its new
playback could indefinitely defer a stopped surviving subscriber or abort their
shared calculation. The control-side fence now reuses the existing complete
subscriber currentness check, including exact source ownership, Native source
currency, content, path and intent. Active interested Pair-deferred rows remain
conservatively fenced until their placement mirror settles.
Obsolete playback yields only when a nondeferred interested participant is proved
current; a job with no such survivor retains its previous playback fence.

This changes only playback eligibility and its existing worker stop Event. It
does not detach stale subscribers, release readers, alter inference admission,
reorder heavy FIFO work or bypass cancellation, actual memory pressure or final
publication. Five new controller cases cover source/content replacement, retained
heavy admission, current active participants, unsettled Pair mirrors and all-stale
ownership through shutdown. The four replacement cases failed on the original
predicate. A first post-fix check exposed an incorrect new-test assumption about
an existing unavailable cache placeholder; the corrected assertion checks exact
object and serialized metadata preservation instead of expecting its absence.

96 distinct controller cases passed across the focused runs:22 shared-material
cases on the final test source and74 unchanged compatible interest, admission,
Pair, resource and batch cases. Three final static checks cover formatting,
strict controller types and OpenSpec/whitespace. Owned Ruff checks passed. Initial
writer setup built and verified its isolated Release/API with the22 known Rust
warnings; existing einops0.6.1 warnings remain. This is no whole-matrix, inference,
latency, device or hearing claim. ENTIRE former nativeREST40 and FINAL41 follow as
STEP41 and FINAL42, preserving all original failures and UNKNOWN gates.

## STEP39 result: shared heavy-job admission and scoped playback fencing

One native FIFO capability now covers complete manual/automatic BPM-Key work,
Stem inference and its existing UI-to-pair-preparation continuation. Waiting
heavy Cold jobs leave both ordinary workers available. An already admitted
physical job executes its native continuation under the same bounded offline
worker/capability, so a full queue of independent waiting analyses cannot block
its own completion. Captured operations/readers retire before Cold reservations
and finally the last heavy capability. Same-job borrows survive cancellation,
worker return and delayed process/tree/pipe/temporary cleanup independently.

Stem admission is nonblocking before submission to the existing worker pool.
Ineligible waiting requests withdraw their FIFO entry; cancelled/disposed job
carriers cannot register again. Transferred metadata retains no active handle.
KeyNet retains only session serialization between calls, rather than a model
session/arena. Native API reflection rejects older bindings before app startup.
The Beat child duplicate guard remains subordinate to whole-job admission; its
early probe never holds that guard while waiting. CPU/CUDA calls on one adapter
share a nonblocking request guard and cannot mutate another call's retirement
Event. Complete PAD analyses still queue through their original Native owners.

Stopped-target Stem generation now continues while unrelated PADs play, subject
to actual resource guards. Starting a current requested subscriber still fences
that physical job. Provider calibration retains its separate playback exclusion.

Verification: Release Native/API passed with the22 known compiler warnings.
355 focused Python tests passed, with the unchanged real Torch-pool check
deliberately deselected;233 additional Portable capability tests passed.56 Looper
Rust tests and one owned KeyNet session-loading contract test passed. Tests cover
actual FIFO/last-borrow release, queued cancellation, ordinary Cold progress,
manual analysis coupling, saturated continuation, panic/cancel/shutdown reader
retirement, production Stem handoff, playback scope, batch/source/provenance,
real controlled child failures and duplicate requests. The old analysis ABA test
now additionally requires a queued successor until prior retirement, retaining
its original token/current-owner and complete settlement assertions.

Owned Python lint and seven strict Mypy modules passed. Source formatting passed
for the checked files/new analysis test; three inherited unformatted Rust files
were not broadly rewritten. Fresh, separate Clippy targets found91 unique current
diagnostics against92 on the actual parent, with no additions and one eliminated
KeyNet builder diagnostic. Whole-library strict Clippy remains unsuccessful on
existing Looper diagnostics; this is not an overall clean lint/quality claim.

No new neural/oracle/CUDA inference, device/listening test, numerical model change
or8GiB ceiling was introduced. Existing floors/caps, runtime/dependency pins,
quality and productive Torch routes remain. OS working-set shrinkage, measured
live overlap, native model cutover and the complete final matrix remain open in
STEP40/41, including all earlier explicit failures and unknowns below.

## Human priority: sequential heavy jobs and transient ownership

Multiple PAD stem requests, including Generate all Stems, and complete BPM/Key
analysis must share one sequential heavy-work admission. Keep the existing
bounded Stem and Cold queues and their source/request/cancellation authority.
Do not reduce the ordinary Cold worker pool: playback preparation must remain
eligible while heavy analysis waits. Admission precedes complete PCM/model work
and remains held through both analysis branches, subprocess trees/pipe readers,
temporary cleanup and destruction of captured transient owners. A queued success
or cancellation notification alone never releases that admission.

The intended boundary is a shared native FIFO ticket above the existing Python
child-process slot. Native analysis and its scoped BPM branch explicitly borrow
the same active owner. Existing Python scopes acquire the whole-job permission
before the subordinate process slot; queued cancellation never releases another
owner. KeyNet sessions become job-owned rather than retaining a session/ORT arena
between independent jobs. Repeated model loading is an explicit cost of releasing
that owner; the model, frontend, numerical options and quality remain unchanged.
Persistent playback sources and published results retain their legitimate owners.
Owner retirement does not promise a particular OS working set or complete ORT/
Windows runtime memory return. No global collection or working-set trimming.

Normal hardware targets16-32GiB. Existing memory admission already scales with
actual available physical memory/commit and pending reservations; no global8GiB
machine ceiling was found. Preserve its floors, caps and refusal/retirement rules.
The existing single-thread numerical policy is a separate performance question,
not changed by this scheduling/lifetime step. Verification needs controlled mixed
jobs, last-reader/drop witnesses, cancellation/failure/shutdown and ordinary Cold
progress, not repeated model/CUDA inference.

The Human also tested the published App with two playing PADs and requested
stems on a third stopped PAD. The previous global any-active-PAD playback Event
aborted that request when START/STOP activity became visible. The new explicit
Human decision permits generation for a stopped target while other PADs play,
subject to real resource checks. Playback fencing therefore follows the physical
job's surviving requested subscribers, not unrelated active PADs. Provider cost
learning still excludes background playback; this does not certify CPU/CUDA
overlap, measured deadline headroom or universal glitch-free performance.

This directly requested step takes priority over the previous native-only next
step. Its implementation is STEP39; ENTIRE former nativeREST39 follows as STEP40
and ENTIRE former FINAL40 as STEP41. All original unfinished numerical, native
CUDA, quality, platform, distribution and final gates below remain unpassed.

## STEP38 impact: owned Windows bootstrap allocations

The actual standalone learned CLI's global allocator now uses a dedicated
VirtualAlloc mapping for EACH pre-seal Rust allocation, instead of System's
opaque Windows heap. This includes argument strings, prepared I/O storage and
the unchanged selected RustFFT Arc/twiddle/window/cache owners when they allocate
through this global allocator. Native allocations made internally by Windows,
CRT, TLS or the loader are NOT intercepted or bounded by this adapter.

The new bootstrap adapter checks payload+header+alignment slack before the SAME
actual-page-rounded backing call. It aligns the payload within that region,
including alignments above page/allocation granularity, and stores the ORIGINAL
mapping base and extent immediately before it. Each original Rust owner releases
its mapping only after its final reader, even if this happens after seal. No
arena mapping is released in production. GlobalAlloc's existing default realloc
allocates, copies and then releases; a refused new allocation preserves the old
owner and bytes. After seal, every new allocation still uses ONLY the unchanged
logical pool; a migrated bootstrap pointer never opens an exhaustion fallback.

Bootstrap counters describe current live Rust payload, region count and mapped
bytes, including headers/slack/page padding, under the SAME allocator lock.
They are observations of owned backing, NOT a finite startup recipe or successful
prospective process allowance. A failed native release does not decrement these
counters; the global allocator aborts without formatting/unwinding because
deallocation has no error channel. This is terminal HOST failure, never successful
settlement or a CUDA fault. Non-Windows retains System with extent UNKNOWN.
Per-allocation pages and API calls may INCREASE cost; no memory/runtime/RSS saving
is claimed. Bootstrap and arena mappings are already inside PrivateUsage and
must never be added twice. API overhead remains UNKNOWN.

The Pool/Header/first-fit/logical capacity and complete original sealed schedule
are unchanged. STEP36's BOTH-residue768857616B/5183960-request arena-only proof is
source-compatible REUSED without replay; at4096 pages its768860160B mapping is
still derived, NOT an actual full-model allocation. No selected FFT/kernel/math,
geometry, precision, dependency, productive path, installer or original guard
changes. Complete retained PRENN/native process backing, original2MiB stack,
images/COW, CRT/native TLS/loader and every later native I/O/close/stdio/exit/error/
panic state remain unproved. No original NN admission query, model, oracle or
unchanged constrained CUDA retry follows this correction.

This necessary bootstrap-ownership split grows the plan to40. NEXT39 retains
ENTIRE former nativeREST38 and FINAL40 ENTIRE former FINAL39, including all
negative/UNKNOWN evidence and ALL original learned/full485100/1101/B1/B2/fulltrack,
mask atol/rtol3e-4 and PCM atol3e-5/rtol3e-4 acceptance, mandatory actual native
NVIDIA CUDA under meaningful changed model/operator/runtime AND fresh complete
ORIGINAL admission, real same-model device fault -> actual Tree AND Reader END
-> fresh CPU/fallback/CPU-only, COMPLETE practical Demucs, all roles/platforms/
CPU-CUDA releases/licenses/installer/recovery/footprint/quality/functions/cancel/
AMP and final combined gates documented below. Separate App/Beat closures and
trial-only provenance stay unchanged; installed savings remain zero. This is
ownership progress, not completed PRENN/native closure or learned parity.

The existing STEP37 Microsoft API links also apply here; VirtualFree requires
the original reserve base, size0 and MEM_RELEASE. The production arena still
survives through process exit. Isolated tests release it only after all readers
end and live arena bytes reach zero.

### STEP38 verification receipt

Fresh Windows x64 Release example controls PASS20/0 failed/0 ignored, with ONE
unchanged complete-original arena recipe deliberately filtered. Three new tests
cover checked prefix overflow/refused acquisition without owner publication,
controlled release refusal with unchanged ownership/payload followed by real
release, and eight actual alignment classes1/2/8/16/32/4096/131072/262144 with
odd payloads, initial zero bytes, nonaliasing sentinels and actual VirtualQuery
original-base/extent/private/committed/read-write checks. Successful VirtualFree
and settled local counters prove scoped release; a post-free snapshot cannot
require MEM_FREE because another thread may reuse that address.

The expanded original migration test covers pre-seal zero/grow/shrink, oversized
realloc refusal preserving the old owner/bytes, refused cross-seal growth and
successful migration into the SAME arena with bootstrap settlement. Existing
arena alignment/fragmentation/zero/realloc/concurrency/failed-backing and typed/
nonflushing I/O controls pass again. Test-only System access for the unchanged
sparse proof stays explicit; production Windows bootstrap uses no System heap.
Controlled injected refusal is NOT an actual host VirtualAlloc/VirtualFree
failure. The production release-failure abort branch remains source-backed,
not an induced OS failure or whole-process panic-free acceptance. Existing
caught fixture panics remain intentional disposal controls.

Strict all-target Clippy, focused Rustfmt and Release learned example PASS;
outer build harness PASS1 in13.76s. Final outer verification PASS2 in5.06s includes
strict OpenSpec, four Markdown files/11 local targets, unchanged productive
closures/pins/locks, default/optional/App dependency isolation, actual nonthreaded
packing and byte-compatible Pool/proof/kernel source. No complete5M recipe or
unchanged numerical suite was rerun. The earlier13 learned+10 spectral+5 track
results remain compatible reused evidence, not a new whole-suite run.

FOUR actual scoped CLI controls pass: SGEMM64/FFT8 retains logical/mapped1048576B,
259 requests/peak368208B/live0/fail0; five-cycle retained spectral retains logical
1065040B/mapped1069056B,6 working requests/113672 payload bytes,15 later requests,
peak1065008B/live0/fail0. Its bootstrap owners before -> after workspace disposal
are12 ->4 regions,83508 ->33972 payload bytes,122880 ->49152 mapped bytes. These
are SMALL FIXTURE ownership observations, not complete original retained PRENN
or process bounds; remaining argument/PCM/runtime owners still exist. Actual
inverse.accumulation refusal remains24068 requested bytes/live0/fail1. Retained
I/O writes all four exact float32 files[.25,-.5,1,-0] and keeps actual Windows
read-only-handle error5. ONE observer precedes ONE joined executor:4/4 original
scope closes, pending0/inflight0/sampling_errors[]. No learned NN ran.

Actual Windows x64 Release learned EXE1236480B SHA256
d603ef021e3f5e4a1301f3dd8f6824fb4c2fdec83f21304ed882cd018d4c2488 is COMPONENT
evidence only, not Compiler/FullProducer/MappedImages/other OSes/learned/quality/
distribution acceptance. External Native/API PASS with unchanged22 warnings and
einops0.6.1 warning/pin. Source/pins/tests and these recipes retain the durable
facts; generated logs/build/control output are reconstructible. Systems PRE/
frozen POST, Methods BEFORE and ONE independent final actual-diff chain apply;
all jobs/readers end before checked normal branch publication. Native API costs,
CRT/TLS/loader/I/O/exit and COMPLETE process/original admission remain UNKNOWN.
ALL inherited NEXT39/FINAL40 gates remain required without omissions.

## STEP37 impact: dedicated Windows arena backing

The diagnostic allocator previously obtained its entire arena with
System.alloc_zeroed. Windows now obtains that one owner directly with
VirtualAlloc(NULL, checked_page_extent, MEM_RESERVE|MEM_COMMIT, PAGE_READWRITE).
GetSystemInfo supplies the actual commitment page size, distinct from address
allocation granularity. The same logical capacity and SAME first-fit pool remain;
rounded page padding never enters contains, free lists or capacity statistics.
A separate optional mapping extent describes ONLY this owner's pages. At4096B
pages,768857616 logical bytes require768860160 mapped bytes, including2544B
inaccessible-to-the-pool padding. This is an API-derived extent, not an actual
full-model allocation or an estimate of complete process overhead.

No pool state is published if backing acquisition fails. Fresh mapped pages
supply zero initialization; they are not a physical residency guarantee. The
production mapping deliberately survives until process exit, as the previous
arena did; there is no new release/unseal or System exhaustion fallback. Only
isolated tests release their mapping with VirtualFree after all pointers/readers
have ended and live bytes are zero. Non-Windows retains the original System
owner and an unknown native extent; no non-Windows execution is claimed.

The unchanged pool algorithm, logical extent, payload/layout/lifetime sequence
and both16-aligned base residues reuse STEP36's complete arena-only proof without
another5M-request replay. This correction changes backing ownership, not FFT
selection, math, neural operators, geometry, precision or quality. Other retained
PRENN System/FFT/Arc/twiddle/windows/cache-RandomState/native heap costs, original
2MiB stack, full image/COW, CRT/native TLS/Loader and later read/seek/write/flush/
sync/close/stdio/exit/error/panic remain UNKNOWN. The mapping and its padding are
already inside PrivateUsage; never add them twice. Neither this one-region
contract, VirtualQuery nor Job failure caps establish successful process admission.

This necessary native-backing split grows the plan to39. NEXT38 retains ENTIRE
former nativeREST37 and FINAL39 ENTIRE former FINAL38, including EVERY remaining
requirement and negative/UNKNOWN receipt below. Complete actual retained/native
process closure and sufficient fresh ACTUAL ORIGINAL admission must precede true
LearnedMasks/PCM/full485100/1101/B1/B2/fulltrack at unchanged mask atol/rtol3e-4
and PCM atol3e-5/rtol3e-4. CPU allocation/I/O/FFT/math alone never permits another
unchanged failed CUDA attempt: meaningful changed model/operator/runtime AND
fresh complete admission are required for mandatory ACTUAL native NVIDIA CUDA.
Real same-model device fault -> actual Tree AND Reader END -> fresh CPU/fallback/
CPU-only; COMPLETE practical Demucs; ALL source/frontend/graph/weights/exporter/
runtime/bootstrap/manifest/provider/native-dependency/compiler/binary/full-producer/
mapped-image/OSarch roles; portable Windows EXE AND honest actual Linux/macOS;
CPU/CUDA releases/CRT/licenses/LGPLsource+three patches/NVIDIA notices/installer/
recovery/full footprint; ALL quality/functions/interactions/resources/cancel/AMP/
final gates remain. Separate App3.14/Torch2.12/cu130 and Beat3.12/Torch2.8/cu128-or-
CPU closures, independent Rust cores/thinPyO3 and trial-only provenance remain.
Fullhost/narrow validator/schema/Pydantic/32keys UNKNOWN,32 parent failures,
distinct216 watchdog UNCOMPARED, registry/pressure UNKNOWN, Overlap OPEN and
combined2/4/6<=10 gates remain. No floor/cap/Jobbudget/OS/pagefile/foreign-process/
productive dependency/installer change; no installed saving/runtime/RSS gain.

API contracts:
[VirtualAlloc](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualalloc),
[SYSTEM_INFO](https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/ns-sysinfoapi-system_info),
[VirtualFree](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualfree)
and fixture-only
[VirtualQuery](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualquery).
They describe this region, not the implementation's other native memory costs.

### STEP37 verification receipt

Fresh focused Windows x64 Release example controls PASS17/0 failed/0 ignored,
with ONE compatible complete-original arena recipe deliberately filtered. Three
new tests cover checked page rounding/overflow, controlled backing refusal with
unchanged allocator state followed by successful seal, and the actual private
committed read/write region via VirtualQuery. The region test checks original
allocation base/extent/protection/type, initial zero bytes, exhaustion at the
logical boundary despite mapped padding and complete reader cleanup. Existing
fragmentation, zero/realloc, preseal System migration, concurrent ownership,
small sparse/native parity and typed/nonflushing I/O controls pass again.
The intentional caught fixture panics remain disposal controls, not whole-process
panic freedom. Original library/spectral/track math and STEP36's full BOTH-base
768857616B recipe are source-compatible reused evidence, not a fresh full suite.
Strict all-target Clippy, focused Rustfmt and Release learned example PASS.
The first invocation stopped at the new extern declaration's format only;
corrected format and full focused build PASS1 in13.84s. No assertion was relaxed.

Final outer verification PASS2 in4.70s includes strict OpenSpec, four Markdown
files/11 local targets, unchanged productive closures/locks/pins, dependency
isolation/actual nonthreaded packing and exact unchanged pool/proof/kernel source.
FOUR actual small CLI scopes pass: SGEMM64/FFT8 uses logical/mapped1048576B,
259 requests/peak368208B/live0/fail0; five-cycle retained spectral uses logical
1065040B/mapped1069056B,6 working requests/113672 payload bytes,15 later requests,
peak1065008B/live0/fail0; a real inverse.accumulation prefix refusal retains
requested24068B/live0/fail1; retained I/O writes all four exact float32 files and
preserves real Windows read-only-handle error5. ONE original observer precedes
ONE joined executor:4/4 scope closes, pending0/inflight0/errors[]. These are
FIXTURES ONLY. The full-model768860160B mapping was NOT allocated/executed here;
its page extent above follows the source/API recipe, not an observed NN peak.

Actual Windows x64 Release learned EXE1234944B SHA256
3026b441d5ad3e71c028ff812ebe7dd78fc47fb5c1e810647bbda93e9abe6884 is COMPONENT
evidence only, not Compiler/FullProducer/MappedImages/other OSes/learned/quality/
distribution acceptance. External Release Native/API passes with the unchanged22
warnings and einops0.6.1 warning/pin. No weights/model NN/oracle/CUDA or full-model
promise query ran; productive Torch/installer and installed savings remain unchanged.
Source, pins and these recipes retain the facts; generated build/control logs
are reconstructible. Systems PRE/frozen POST, Methods BEFORE and ONE independent
final actual-diff chain apply to this coherent step; all readers/jobs must end
before its normal branch publication. Every remaining native/final gate above
and below remains required under NEXT38/FINAL39, without omissions.

## STEP36 impact: arena-owned spectral working storage

The actual learned CLI now prepares an opaque SpectralPlan containing both
selected RustFFT plans, immutable scratch lengths and both original periodic
Hann windows before seal. Consuming that plan after seal allocates ONLY the
forward buffer/scratch, inverse buffer/scratch and maximum inverse envelope/
accumulation vectors, in that order, before Model::load. No planner, cache,
twiddle construction, Arc clone or Hann cosine runs in from_plan. Geometry is
bound into the consumed plan; track mode still prepares the original485100
frames even for a short source. Zero-length scratch has no allocation event.

This transfers six retained working-vector owners from System to the existing
arena, without changing FFT selection, expressions, reduction order, maximum
geometry, precision, masks, model or track ownership. The ordinary allocating
constructor preserves its original planner/cache lifetime and allocation order.
Both paths share numerical kernels; an independent ordinary-prepared path is
retained for exact-bit comparisons. Fallible partial construction releases all
successful arena prefixes, and later error/cancel invalidates the entire private
attempt. Plans/windows still dispose normally through their original owners.

The complete SAME-production sparse-header recipe now starts with the exact
nonzero layouts supplied by the actual retained SpectralPlan. That persistent
prefix stays live through every original all-four diagnostic, optional-original
forward, single-mask, B1/B2 and complete-track episode. The old764894800B arena
receipt is historical for the old topology, not reused as sufficient here.
The full new address-highwater derivation and concrete replay must cover both
16-aligned base residues, exact packing/free-list restoration and cleanup.
This is an arena-only proof, never a measured process/RSS or successful model.

The selected FFT Arc/twiddle graph, windows, planner/cache/native RandomState
retention, prepared I/O/arguments/stdio, original2MiB stack, images/COW, native
heaps/CRT/TLS/Loader, File NtReadFile/NtWriteFile/optional waits/error conversion,
seek/write/flush/sync/CloseHandle, normal exit/error/panic remain OPEN. System
frees do not prove native retention disappears. Arena backing inside PrivateUsage
is counted once. Snapshots/no-growth/private heaps/Job failure caps/import lists
remain insufficient successful prospective process allowances.

This necessary ownership split grows the plan to38: NEXT37 retains the ENTIRE
former nativeREST36 and FINAL38 the ENTIRE former FINAL37. FIRST complete actual
retainedPRENN/native/process closure and sufficient fresh ACTUAL ORIGINAL
admission, THEN genuine original full LearnedMasks/PCM/full485100/1101/B1/B2/
fulltrack at mask atol/rtol3e-4 and PCM atol3e-5/rtol3e-4. No standalone model
promise query, NN/oracle for numbers or unchanged constrained CUDA retry.
CPU math/allocator/I/O/FFT alone is no meaningful changed CUDA condition.
ALL inherited mandatory actual native NVIDIA CUDA/real same-model device fault
-> actual Tree AND Reader END -> fresh CPU/fallback/CPU-only, COMPLETE practical
Demucs, all source/frontend/graph/weights/exporter/runtime/bootstrap/manifest/
provider/native/compiler/binary/full-producer/mapped-image/OSarch roles, portable
Windows EXE AND honest actual Linux/macOS, CPU/CUDA releases/CRT/licenses/LGPL
source+three patches/NVIDIA notices/installer/recovery/full footprint, quality/
functions/interactions/resources/cancel/AMP and final gates remain unchanged.
Fullhost/narrow validator/schema/Pydantic/32keys UNKNOWN,32 parent failures,
distinct216 watchdog UNCOMPARED, registry/pressure UNKNOWN, Overlap OPEN and
combined2/4/6<=10 remain. Separate App3.14/Torch2.12/cu130 versus Beat3.12/
Torch2.8/cu128-or-CPU, independent Rust/thinPyO3/trial-only provenance remain.
No floors/caps/Job budgets, foreign processes, OS/pagefile, geometry, precision,
quality or productive dependency/installer changes; no installed/runtime/RSS gain.

### STEP36 verification receipt

Fresh Release tests PASS42/0 failed/0 ignored:13 learned-library,10 spectral,
5 track and14 example controls. The expanded spectral checks compare the split
workspace against the ordinary prepared route and original allocating kernels,
including exact feature/all-four complex-mask PCM bits at full485100/1101 and
subsequent short-tail reuse; single-stem layout, malformed extents/rates, early/
mid preparation cancellation, partial reconstruction, overflow and clean reuse
remain covered. These are spectral/operator/ownership controls, not learned NN.

The entire new original recipe derives AND explicitly replays768857616B for
BOTH16-aligned base residues. Actual selected-plan working layouts are four
16384B Complex32 requests plus two1948592B inverse float requests, all align4:
3962720B payload plus allocator overhead. Address highwater/charged peak is
768857568 at residue0 and768857584 at residue16, followed by the structural32B
minimum splittable tail. Each exact replay performs5183960 verifier requests,
not an observed NN allocation count. Model metadata remains5456/6272/96/7936B
align8 from actual private types. Every original packing/band/transformer/sample/
B1/B2/track restoration and terminal coalescing remains checked. Complete
process admission, other selected FFT branches and other OSes remain unproved.

The initial outer build harness stopped at an overly broad workspace format
check on unchanged Looper files. The corrected focused Rustfmt/strict all-target
Clippy/Release example checks PASS; build harness PASS1 in20.05s. The first actual
spectral fixture then honestly FAILED with its former1MiB arena at inverse.output
request131168B, because the new persistent prefix now also occupies its backing.
Its original1/1 scope closed. No model ran. The FIXTURE-only correction derives
1065040B from113792B charged prefix +951216B maximum simultaneous feature/mask/
output requests +32B structural tail, retaining4099 frames and all five cycles.
An added SAME-allocator test explicitly validates both residues, all cycles,
exact persistent-state restoration and terminal cleanup. Delta harness PASS1
in2.51s: that ONE new Release test passed,14 unrelated example tests filtered;
fresh strict Clippy/format/build PASS. The prior42 remain compatible reused
results, making43 distinct successful controls, not a new final43-case run.

Final outer checks PASS2 in4.69s: strict OpenSpec/four Markdown files/11 local
targets, default/optional/app isolation, actual nonthreaded packing and unchanged
productive closures/pins/locks; ONE actual spectral reuse process plus SIX
actual typed allocation-prefix refusals. Reuse reports6 working requests,
113672B fixture payload,15 later requests/five cycles, capacity1065040,
charged peak1065008 and final live0/failures0. Each refusal preserves its exact
phase/requested bytes and retires every successful prefix: live0/failures1.
ONE original observer precedes ONE joined executor:7/7 scopes closed,
pending/inflight0, sampling errors[]. These counters are not native no-growth,
model capacity or a successful prospective process allowance.

Actual Windows x64 Release learned EXE1236992B SHA256
1d713ed80af638585458b4b6f02414e7ac678b717480183ecb9b1560779e21ca is component
evidence ONLY, not Compiler/FullProducer/MappedImages/non-Windows/learned/quality/
distribution acceptance. No weights, model, oracle, full-model promise query or
CUDA attempt occurred. Compatible STEP25/26/28/29/30/31/32/34/35 and older numerical/
NN/CPU-cancel/CUDA-negative/role evidence remain reused within original limits;
STEP33's old topology is superseded by the complete NEW arena-only recipe here.
Source, pins, source-owned layout API, controls and recipes retain durable facts;
generated logs/reports remain reconstructible. Productive Torch/installer and
installed savings ZERO remain unchanged. ALL native/process and inherited final
gates stated above stay OPEN.

## STEP35 impact: explicit private-output disposal

The standalone learned CLI now owns every prepared output buffer through a
small nonflushing owner. Explicit writes and the existing successful
flush-then-sync_all completion retain their order and error phases. Disposing
of pending output on an early return, write/flush failure or unwind dismantles
BufWriter with into_parts; it never retries buffered writes from Drop. The
actual read-only-handle control uses this SAME owner rather than a separate
manual disposal path. File ownership still ends normally after synchronous
readers, and every failed attempt invalidates ALL private output files.

This removes a real, previously permitted native write path. STEP31 explicitly
allowed best-effort destructor flushing; this is a source-motivated
simplification, not a newly discovered historical publication defect. The
8192-byte I/O buffers and handles remain prepared before arena seal. No neural,
spectral, track, model metadata or packing allocation episode changes, so the
complete STEP33 arena-only proof and STEP34 math controls remain compatible.

Focused fault writers verify pending abandonment, partial write failure, sink
flush failure, explicit successful flush, caller unwind and inner writer panic.
The actual retained-file fixture verifies exact successful bytes and a numeric
read-only-handle error. These establish disposal behavior only. Native
NtReadFile/NtWriteFile, optional WaitForSingleObject/RtlNtStatusToDosError,
seek/flush/sync/CloseHandle, stdio, CRT/DLL exit, TLS/Loader, original2MiB stack,
full images/COW and retained preNN FFT/System backing remain OPEN. Dropping the
File is still native I/O. Neither snapshots/no-growth, a private heap nor Job
failure enforcement supplies a sufficient successful prospective process bound.
Arena backing already inside PrivateUsage must never be added twice.

This necessary native-I/O ownership split grows the plan to37: STEP36 retains
the ENTIRE former nativeREST35 and FINAL37 the ENTIRE former FINAL36. FIRST
complete actual retained PRENN/native/process closure AND sufficient fresh
ACTUAL ORIGINAL admission, THEN real original full LearnedMasks/PCM at
full485100/1101/B1/B2/fulltrack under unchanged mask atol/rtol3e-4 and PCM
atol3e-5/rtol3e-4. No independent promise query or NN/oracle runs for numbers.
CPU math/allocator/I/O/FFT work alone is no meaningful changed CUDA condition.
Mandatory ACTUAL supported native NVIDIA CUDA still requires meaningful changed
model/operator/runtime AND fresh complete Original admission; real same-model
device fault -> actual Tree AND Reader END -> fresh CPU/fallback/CPU-only;
COMPLETE practical Demucs normalization/shifts/overlap/hybrid spectral-time/
complex STFT-ISTFT/conv/recurrent/attention/resampling; ALL Source/Frontend/Graph/
Weights/Exporter/RuntimeBootstrap/Manifest/Provider/NativeDependencies/Compiler/
Binary/FullProducer/MappedImages/OSarch roles; portable Windows EXE AND honest
actual Linux/macOS; complete CPU/CUDA releases/CRT/licenses/LGPL source+three
patches/NVIDIA notices/installer/recovery/full footprint; ALL quality/functions/
interactions/resources/cancel/AMP/final gates. Fullhost UNKNOWN, narrow validator/
schema/Pydantic and32 keys,32 parent failures, distinct216 watchdog UNCOMPARED,
registry/pressure UNKNOWN, Overlap OPEN and combined2/4/6<=10 remain. Separate
App Python3.14/Torch2.12/cu130 and Beat3.12/Torch2.8/cu128-or-CPU closures,
independent Rust cores/thin PyO3 and trial-only provenance are unchanged. No
resource floor/cap/Job budget/geometry/precision/quality or installed-saving claim.

### STEP35 verification receipt

Fresh Windows x64 Release example tests PASS7/0 failed/0 ignored, with four new
disposal controls and three existing typed-error controls; seven unrelated
example tests were filtered, not rerun. Counter writers establish no additional
write on pending abandonment/caller unwind/partial-error disposal, preserved
partial bytes and numeric error5, explicit exact successful bytes, sink flush
error6 and one close after an injected inner writer panic. The two caught fixture
panics are intentional tests, not evidence of a panic-free process. Rustfmt,
strict all-target Clippy and Release learned example build PASS. The outer build
harness PASS1 in13.13s after external Release Native/API preparation, retaining
the same22 known Native warnings and unchanged einops0.6.1 warning/pin.

Final outer checks PASS2 in3.93s: strict OpenSpec/local Markdown targets,
default/optional/app dependency isolation, actual nonthreaded packing config and
unchanged productive sources/closures/pins/locks; ONE actual retained-file scope
with four exact little-endian float32 files [.25,-.5,1,-0] and real Windows
read-only-handle error5 using the SAME output owner. The original observer
preceded one joined executor:1/1 scope closed, pending/inflight0, sampling
errors[]. Its1MiB fixture and unchanged Rust-arena counters do not establish
native no-growth, full-model capacity or prospective process admission.

Actual learned Release EXE1230336 bytes, SHA256
bd136a9768fe9ad3dad4dc5add62862d26407e72bac9c13d8e58ac81e79331d8,
is Windows x64 component evidence ONLY, not Compiler/FullProducer/MappedImages,
non-Windows, learned NN, quality or distribution acceptance. No learned model,
oracle, standalone full-model admission query or CUDA attempt occurred.
STEP25/26/28/29/30/31/32/33/34 compatible numerical/lifetime/arena/math and older
NN/CPU-cancel/CUDA-negative/role evidence remain reused within original limits;
no repeated full matrix/model/arena schedule. Productive Torch/installer remain
unchanged, installed savings ZERO, and no runtime/RSS improvement is claimed.
Source, pinned into_parts/Drop behavior, these controls and reconstruction recipe
retain the durable facts; generated logs/reports are reconstructible cache.

## STEP34 impact: source-owned learned transcendental math

The independent learned CPU core now uses the EXISTING optional pinned
libm0.2.16 for adjacent rotary sin/cos, softmax and sigmoid exp, and mask tanh.
The f32 expressions, evaluation order, geometry, checkpoint frequencies, gates,
cancellation and private-output invalidation remain unchanged. This removes
specific late CRT transcendental paths rather than warming their lazy state.
It adds no dependency, heap owner or lifetime, so the complete STEP33 ARENA-ONLY
allocation recipe remains source-compatible and is reused without replay.

Pinned source closure includes sincosf -> k_sinf/k_cosf/rem_pio2f ->
rem_pio2_large -> scalbn/floor; expf -> scalbnf; tanhf -> expm1f. Existing exact
GELU uses erff -> erfc1/erfc2 -> fabsf/expf. Large finite loaded frequencies
remain supported: argument reduction cannot be assumed absent from ordinary
position samples. The large-angle reduction uses fixed stack arrays, including
four20-element scratch arrays; these are NOT heap owners or a full stack bound.
The selected x64 default arch path must be verified with the actual Release cfg;
no_std alone is not evidence of a complete loaded native/process closure.

Verification compares the previous std kernels and new Rust kernels over finite
and exceptional float classes, neighboring algorithm boundaries, original1101
rotary positions with synthetic small/large signed frequencies, nonuniform dense
versus tiled attention with both query-tile tails, and all62/four-head synthetic
mask projections. Same-kernel exact-bit layout checks remain separate from
old-kernel numerical checks. These are operator controls, without release weights
or a learned model execution; a kernel change does NOT certify original complete
LearnedMasks/PCM. Original mask atol/rtol3e-4 and PCM atol3e-5/rtol3e-4 remain.

Actual retained FFT/System heaps, original2MiB stack, full images/COW, native TLS,
Loader and all native I/O/stdio/close/error/panic/CRT/DLL shutdown paths remain
OPEN. A snapshot, no observed growth, a fixed private heap, import list or the
original Job cap is not a sufficient successful process allowance. Arena memory
already inside PrivateUsage must never be added again. No new model promise,
NN/oracle or CUDA retry is justified by CPU math alone.

This necessary native-math split grows the plan to36: STEP35 retains the ENTIRE
former nativeREST34 and FINAL36 the ENTIRE former FINAL35. FIRST complete actual
retained PRENN/native/process closure AND sufficient fresh ACTUAL ORIGINAL
admission, THEN original full learned masks/PCM/full485100/1101/B1/B2/fulltrack
under unchanged tolerances; meaningful changed model/operator/runtime AND fresh
complete admission before mandatory ACTUAL native NVIDIA CUDA; real same-model
device fault -> actual Tree AND Reader END -> fresh CPU/fallback/CPU-only;
COMPLETE practical Demucs; ALL Source/Frontend/Graph/Weights/Exporter/RuntimeBootstrap/
Manifest/Provider/NativeDependencies/Compiler/Binary/FullProducer/MappedImages/OSarch
roles; Windows EXE AND honest actual Linux/macOS; complete CPU/CUDA releases/CRT/
licenses/LGPL source+three patches/NVIDIA notices/installer/recovery/full footprint;
ALL quality/functions/interactions/resources/cancel/AMP/final gates. Fullhost and
validator/32keys/32 parent failures/distinct216 watchdog/registry/pressure UNKNOWN,
Overlap OPEN and combined2/4/6<=10 gates remain. Productive Torch/installer/pins,
resource floors/caps/Job budgets/precision/geometry and installed savings unchanged.

### STEP34 verification receipt

Fresh Windows x64 Release library controls PASS13/0 failed/0 ignored, including
three new math tests and all ten existing learned controls. The scalar comparison
covers100151 deterministic float samples including explicit exceptional values
and signed neighbors at14 pinned algorithm boundaries; it is not exhaustive over
all f32 bit patterns. Rotary covers all1101 original positions with32 synthetic
small/large/signed frequencies and all8 heads, unchanged V bits, cancellation and
overflow rejection. Independent old-std dense attention uses17/31 positions with
query-tail widths1/15, all8 heads and full noncausal rows. These comparisons use
absolute2e-7/relative2e-6; exp uses absolute two minimum-subnormal units instead.
The all62/all4/33-position synthetic mask control preserves new-kernel exact-bit
layout checks and separately compares previous std tanh/sigmoid at original mask
atol/rtol3e-4. No release learned weights or full-model numerical pass is implied.

Rustfmt, strict all-target Clippy and Release examples PASS; the outer build
harness PASS1 in38.62s. Actual emitted Release library assembly calls the Rust
libm sincosf/expf/tanhf symbols at the learned sites; BOTH Norm functions lower
their square roots to hardware sqrtss. Actual libm cfg is arch/default, opt3,
target x64 SSE/SSE2/SSE3, rustflags empty, no unstable intrinsic or x87 selection.
This is an actual selected-build source/call-chain result, NOT an attestation of
all loaded images, a complete stack/process bound or another platform/compiler.

Final outer static/component verification PASS2 in4.09s: strict OpenSpec, three
Markdown files/10 local targets, unchanged productive source/closures/pins/locks,
default optional-feature isolation and actual nonthreaded matrixmultiply/libm cfg.
Release learned EXE1229312B SHA256
ddfd945ea20e57f7ec319336f41bcc97d8f006c93173316816ba699536eea9a2 is a Windows x64
COMPONENT ONLY, not Compiler/FullProducer/MappedImages/nonWindows/distribution or
whole learned/quality proof. Generated assembly, fingerprints, logs and component
report are reconstructible; source, pins, tests and this receipt retain the facts.

The Human explicitly paused for reboot and later resumed. The interrupted own
preparation build is not a PASS. The first resumed outer check built/verified
Native/API but then correctly rejected a source snapshot changed by the root
during copying. No guard was weakened. Subsequent source-frozen serial checks
passed, with the same22 known Native library warnings and unchanged einops pin.
No App/held Native/private data or foreign process/OS/pagefile change occurred.

STEP33's exact ARENA-ONLY proof and unchanged STEP25/26/32 spectral/track arithmetic
are reused within their limits, together with compatible older CPU/cancellation
and failed CUDA evidence. There was NO full-model promise query, NN/oracle/CUDA
execution or repeat of the unchanged complete arena proof or whole regression
matrix for nicer numbers. Systems PRE/frozen
POST, Methods and ONE independent final actual-diff chain cover this scoped step.
Complete process closure and sufficient fresh ACTUAL ORIGINAL admission remain
required FIRST; installed savings remain ZERO and no speed/RSS gain is claimed.

## STEP33 impact: complete prepared-route allocator proof

The diagnostic allocator now separates header storage from its COMPLETE original
first-fit algorithm. Production uses native headers; the prospective verifier uses
sparse offset headers. Both execute the SAME absolute-address alignment, fit,
split, tiny-tail absorption, sorted insertion and adjacent coalescing bodies.
No second simulator, weight data, neural evaluation, trace-derived prediction or
host-headroom estimate supplies this proof. Metadata Layouts come directly from
the actual target's private Band/Transformer-pair/Vec-of-Mask/Mask types.

The source recipe below is now executable as the example's Release tests. It
replays all1355 tensor and7 metadata requests in load order, all8 time/frequency
blocks at original485100/1101, all-four diagnostic masks/PCM, the optional original
feature forward, six persistent track vectors, complete B1/B2 destinations and
serial full samples. Earlier samples already occupy the destination. Every band,
transformer, selection, sample and B1/B2 batch compares its exact reachable sorted
free list AND charged live bytes with its caller. Repetition therefore restores
the identical allocator state for arbitrary track length, without allocating a
whole-track vector. Smaller diagnostic geometries remain separate schedules.

At each synchronous SGEMM state the verifier checks all actual selected-kernel
packing sizes/alignments (AVX/FMA32, SSE16, fallback4), including full/tail tiles,
AND the conservative1081344B/alignment32 pulse. Every allocation/free must restore
the exact preceding list. Identical repeated calls can consequently be collapsed;
there is no intervening Rust allocation in the pinned nonthreaded packing loop.
Checking actual layouts avoids inferring first-fit placement merely from a larger
payload. Native-versus-sparse storage controls cover absolute alignment, holes,
coalescing and16B tiny-tail absorption; existing actual realloc controls retain
old+new simultaneous backing and failure preservation.

The concrete extent is derived from the highest charged END ADDRESS of the full
recipe in a Layout-valid virtual tail for both16-aligned base residues0/16mod32.
The algorithm's minimum splittable tail, HEADER+ALIGN=32B, is retained explicitly.
The resulting exact extent is then independently replayed for BOTH residues;
capacity monotonicity and max-live-payload capacity are never assumed. This is
ARENA ONLY, without allocating the large payload or claiming actual NN execution.
Source error/cancel branches create no new owner after the failure and unwind
their existing disjoint extents through that same coalescer. They invalidate all
private output; allocator restoration does not certify native panic/exit closure.

The separate Windows investigation confirms pinned Rust1.95 synchronous File
read/write reaches NtReadFile/NtWriteFile, optional wait and NT-status conversion;
seek/sync/close reach SetFilePointerEx/FlushFileBuffers/CloseHandle. Locked stdio
still reaches native handle/console APIs. Learned math, original2MiB stack, actual
retained FFT/System heaps, full image COW, CRT/native TLS/loader and cleanup/exit/
panic paths remain OPEN. Rust wrappers and observed import symbols do not provide
a finite native allocation contract. No-growth snapshots, fixed private heaps or
the original Job cap prove sufficient successful admission; arena backing inside
PrivateUsage must never be counted twice. No fresh model promise, NN, oracle or
CUDA retry is authorized by the arena-only result.

This necessary complete allocator proof split grows the plan to35. STEP34 retains
the ENTIRE former nativeREST33 and FINAL35 retains the ENTIRE former FINAL34:
complete process/native closure AND sufficient fresh ACTUAL ORIGINAL admission
before original full learned masks/PCM,485100/1101,batch2/fulltrack at unchanged
mask atol/rtol3e-4 and PCM atol3e-5/rtol3e-4; meaningful changed model/operator/runtime
before mandatory ACTUAL native NVIDIA CUDA; actual same-model device fault -> Tree
AND Reader END -> fresh CPU/fallback/CPU-only; COMPLETE practical Demucs; ALL source/
frontend/graph/weights/export/runtime/provider/native-dependency/compiler/binary/
FullProducer/MappedImages/OSarch roles; Windows EXE AND actual Linux/macOS; complete
CPU/CUDA releases/CRT/licenses/LGPLsource+three patches/NVIDIA notices/installer/
recovery/full footprint; ALL quality/functions/interactions/resources/cancel/final
gates. Fullhost/validator/32keys/32 parent failures/distinct216 watchdog/registry/
pressure UNKNOWN and Overlap OPEN remain. No productive dependency, installer,
audio/kernel/geometry/precision/floor/cap/Jobbudget or installed-saving change.

### STEP33 verification receipt

Actual Windows x64 Release derivation yields764894800B, explicitly validated for
BOTH base residues. Charged address high water/peak is764894752B at residue0 and
764894768B at residue16; their equality here does not turn max-live arithmetic
into a general capacity proof. Each exact replay executes5183954 verifier requests
(including alternate-kernel/max pulses), NOT a measured production NN allocation
count. Actual metadata Layouts are5456/6272/96/7936B, each alignment8; the fourth
is requested four times. Total model metadata43568B and numeric526818448B bind
the actual target. No stable ABI, different geometry/feature/compiler/OS or
arbitrary larger capacity is certified by these two concrete replays.

Release controls PASS34/no ignored:10 core/learned,9 spectral,5 track and10
allocator/typed-I/O/example tests, including the three new proof controls. Strict
all-target Clippy and Release examples PASS. Twenty-six representative attention
prefix refusal/cleanup cases run across both residues; they preserve old owners
and restore the model's exact list. These are allocator/source controls, not
exhaustive cancellation injection or learned arithmetic. Existing prepared versus
allocating spectral/kernel numerics and older original/failed NN/CUDA evidence are
reused only where source-compatible; no new NN/oracle/full-model promise query.
External isolated Release Native/API PASS with the same22 known warnings and
unchanged einops warning/pin. Human App/held Native/private data are unaffected.

Final outer verification PASS2 in4.38s: Rustfmt, strict OpenSpec, four Markdown
files/11 local targets, default/optional/application dependency isolation and
unchanged productive closures/pins/locks, plus TWO actual original-owned CLI
fixture processes. The unprepared SGEMM64/FFT8 control makes259 requests with
peak368208B; the prepared five-cycle spectral control makes15 later payload
requests with peak951216B. Both use1MiB FIXTURE backing, live0/failures0. This is
NOT the full-model extent, process allowance or measured learned RSS. One observer
precedes one joined executor:2/2 scopes close through tree AND readers, pending0,
inflight0, sampling errors[]. An initial cache-harness assertion wrongly expected
adjacent `live=0 failures=0` fields although the first successful control includes
`allocations=259` between them. Its1/1 scope closed; the parser now checks exact
individual counter values. No product assertion or resource guard was weakened.
That failed harness receipt remains distinct; the final two-case run is fresh.

Reconstruction uses the example's full Release tests and existing
`bs_learned --workspace-control`/`--spectral-workspace-control` under the original
admitted owner. Generated logs/reports remain reconstructible cache; source,
pins, executable recipe and this receipt retain the facts. Systems PRE/frozen
POST, Methods and ONE independent final actual-diff chain cover this scoped step;
all readers/jobs must end before its normal branch publication.

The pinned native call-chain distinction is source-based:
[Handle](https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/pal/windows/handle.rs),
[filesystem](https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/fs/windows.rs),
[stdio](https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/stdio/windows.rs)
and [runtime cleanup](https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/rt.rs).
These sources expose remaining calls; they do NOT attest loaded images or bound
native memory growth. Complete process closure and fresh sufficient ORIGINAL
admission remain required before genuine original learned comparison.

## STEP32 impact: retained spectral execution workspace

The actual learned CLI now prepares ONE `SpectralWorkspace` before sealing its
execution allocator. It retains both directional FFT plans, buffers, scratch and
periodic Hann windows, plus maximum admitted inverse envelope/accumulation vectors.
The all-four diagnostic and single-mask track paths use that SAME workspace;
serial samples and later batches do not rebuild planners, clone plan Arcs or
allocate FFT/window/inverse backing. Existing allocating APIs remain available
and share their original numerical kernels with the prepared path. The productive
application, dependencies, model operators, geometry and precision are unchanged.

`Spectrum` independently owns its features. A reconstruction exclusively borrows
the inverse workspace and spectrum through the final synchronous stem reader.
Before every reconstruction, the exact current envelope extent is cleared and
rebuilt in the original time/window order. A prefix of the maximum-geometry
envelope would include extra trailing windows and is deliberately NOT used.
Accumulation is cleared for each channel; each transform overwrites every input
element before running. Cancellation or arithmetic failure invalidates the entire
attempt, including previously written output. A later independent invocation
reinitializes its exact working state. No source/content/OwnACK/FinalCAS/last-reader
or realtime authority changes.

This is a source-motivated lifetime correction in the real full track route,
not learned acceptance or a claimed memory/speed improvement. Both inverse vectors
now remain live during analysis and the neural forward as preseal process backing.
They MUST be counted there, even though they no longer occupy the execution arena.
Full track preparation uses485100 frames even for a shorter physical source;
diagnostic preparation uses its actual checked complete chunk geometry.

### Simplified ordered source schedule and exact remaining proof

The STEP31 table remains historical evidence. Its later FFT/planner/Arc/HashMap/
twiddle/shrink-to-Box requests are removed from the SEALED prepared CLI schedule.
All their actual selected-branch allocations are retained before NN and still
need the complete preNN process allowance; moving ownership does not erase bytes.
The original unprepared library wrappers/control remain different branches.

For the remaining source events let A(name,n) reserve exactly n float32 elements,
D(name) release that owner, S=1101, F=S*4100 and X=S*62*384. A linear call L(rows,in,
out) first allocates its rows*out result, then performs synchronous packing pulses.
Norm allocates rows*width; final axial normalization is in-place. No success-path
Vec grows beyond its initial exact reservation or reallocates after preparation.

Model load reserves62 Band records; each band width w loads [w,w*384,384]. It then
reserves8 Transformer pairs, each loading TWO ordered tensor lists
[32,384,384*1536,384*8,8,512*384,384,384*1536,1536,1536*384,384]. Final norm loads384.
It reserves4 Vec records and, for each stem,62 Mask records; each band loads
[384*768,768,768*2w,2w]. These are1355 tensor and7 metadata requests. Numeric payload
is526818448B. Candidate LP64 Band88/pair784/Vec24/Mask128 gives43568B metadata,
but actual target `size_of`/alignment STILL must bind these values before any
capacity claim. No stable Rust ABI is assumed.

| Source episode | Ordered requests/releases; caller owners remain live |
| --- | --- |
| Latent bands | A(X); for each band A(selected,S*w), A(norm,S*w), D(selected), L(S,w,384), D(norm), D(projected). Each band returns with X only. |
| Transformer attention, length N | A(norm,N*384), L(N,384,1536)=qkv, L(N,384,8)=gates, D(norm); A(attended,N*512), A(q,N*64), A(k,N*64), A(v,N*64), A(scores,16*N), A(tile,16*64); score/value packing pulses; D(tile),D(scores),D(v),D(k),D(q); D(qkv),D(gates); L(N,512,384)=attention, D(attended), residual add, D(attention). |
| Transformer feedforward | A(norm,N*384), L(N,384,1536)=expanded, D(norm), GELU in-place, L(N,1536,384)=contracted, D(expanded), residual add, D(contracted). |
| Axial traversal | Each of8 blocks runs62 time transformers with A(selected,S*384)/D(selected) around each; then S frequency transformers of length62 borrow X directly. Each transformer must restore its caller's exact free-list state. Final norm allocates nothing. |
| Projection | For each band A(selected,S*384); for each selected stem L(S,384,768)=hidden, activation, L(S,768,2w)=projected, D(hidden), GLU copy, D(projected); D(selected). X and destination masks remain live throughout. |
| All-four masks | Latent returns X; A(masks,4F); projection over4 stems; D(X) on return, retaining masks. |
| Diagnostic | A(pcm,2*frames), A(features,F), D(pcm); all-four masks; A(prediction,8*frames); after writes D(prediction),D(masks),D(features). Optional original-feature pass then A(features,F), all-four masks, D(masks),D(features). |
| Track persistent | In declaration order: input4*485100, source2*485100, result8*727650, counter727650, output8*485100, window485100. No wholetrack-length vector. |
| Track batch/sample | For actual B=1 or2, A(destination,B*8*485100); each sample A(features,F), latent returns X, A(singlemask,F), four independent single-stem projection/inverse episodes; D(singlemask),D(X),D(features). The earlier sample is already INSIDE destination. After add, D(destination) BEFORE flush. Both sample and batch must restore their exact respective caller free lists to prove arbitrary track-length repetition. |

Pinned matrixmultiply0.3.10 std/nonthreading/default packing creates one allocation
before its loops and frees it on return, with no intervening Rust allocation.
For dimensions m,k,n the request is
4*min(k,256)*(round_up(min(m,64),8)+round_up(min(n,1024),NR)) bytes. AVX/FMA uses
NR8/alignment32; SSE NR4/alignment16; fallback NR4/effective alignment4. All current
column dimensions are divisible8, or62 rounded64, or1101 capped1024, so request
sizes agree across these branches. The source maximum remains1081344B.
A conservative maximum aligned packing pulse can dominate these smaller calls
ONLY after proving allocate/free restores the EXACT preceding free list; size
comparison alone does not handle first-fit holes. The actual allocator returns
padding/tiny-tail absorption and coalesces adjacent extents, making this invariant
available but still requiring complete validation. Repeated calls can then be
collapsed by that explicit invariant rather than omitted from the proof.

Next reconcile this entire finite schedule against the SAME actual allocator's
fit/split/free/coalescing implementation, both possible16-aligned base residues
mod32, actual target layouts, headers and tiny tails. A sparse header-store replay
sharing the complete production algorithm is a possible proof without allocating
NN payloads; an independent approximate simulator is insufficient. Verify the
derived extent explicitly: first-fit success monotonicity must not be assumed
when tiny-tail absorption changes placement. The max live payload is not capacity.
No such full replay or smaller arena extent is selected or claimed in this step.

### Native boundary and complete retained remainder

The retained workspace removes later FFT planning/window-cosine calls. Learned
sqrt/sin_cos/exp/tanh, synchronous native read/seek/write/flush/close, pre-existing
heaps, original2MiB stack, image COW, CRT/TLS/loader and exit/error/panic remain
unbounded. Rust wrappers alone do not supply a native allocation contract.
[ReadFile](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-readfile)
specifies caller-buffer/completion rules without a finite user-memory allowance.
[Process exit](https://learn.microsoft.com/en-us/windows/win32/procthread/terminating-a-process)
also invokes DLL detach callbacks; permitted
[DLL initialization/cleanup work](https://learn.microsoft.com/en-us/windows/win32/dlls/dynamic-link-library-best-practices)
includes memory/TLS allocation. A fixed private heap would not bound other process
heaps. The existing Job cap enforces refusal, not sufficient successful admission.
No snapshot or after-the-fact no-growth observation closes these native paths.
Arena backing already inside PrivateUsage MUST NEVER be added a second time.

No complete process bound, sufficient fresh ORIGINAL admission, new learned/oracle
or CUDA run is claimed. CPU spectral ownership alone is NO changed CUDA condition.
This necessary source-ownership split grows the plan to34: STEP33 retains the
ENTIRE former nativeREST32; FINAL34 the ENTIRE former FINAL33. Original full
learned masks/PCM at mask atol/rtol3e-4 and PCM atol3e-5/rtol3e-4, full485100/1101/
batch2/fulltrack, mandatory actual native NVIDIA CUDA under meaningful changed
model/operator/runtime and fresh complete admission, actual same-model devicefault
-> Tree AND Reader END -> fresh CPU, COMPLETE practical Demucs, ALL roles/platforms,
Windows EXE AND actual Linux/macOS, CPU/CUDA releases/CRT/licenses/LGPLsource3patch/
NVIDIA notices/installer/recovery/fullfootprint and ALL quality/functions/resources/
cancel/final gates remain. Original fullhost/validator/32keys/32 parent failures/
distinct216 watchdog/registry/pressure UNKNOWN and Overlap OPEN remain unchanged.
Productive Torch/pins/closures/installer and installed savings ZERO are unchanged.

### Verification boundary

Release controls PASS31 with no ignored cases:10 core/learned,9 spectral,5 track
and7 existing allocator/typed-I/O units. The two added spectral controls compare
prepared and allocating features/all-four complex-mask PCM bitwise through
4099->1025->2501->4099->485100->1025, including single-stem out-of-order writes and
sentinels. They also cover extent/rate/odd-input refusal, nonfinite values,
cancellation during analysis/envelope rebuild/channel two, overflow and successful
fresh reuse after partial work. They exercise spectral kernels and ownership,
NOT the complete learned network. Strict all-target Clippy and Release examples
PASS. External isolated Release Native/API PASS with the same22 known warnings;
the existing einops pin/warning and Human App/held Native remain unchanged.

Final outer verification PASS2 in6.19s: Rustfmt, strict OpenSpec, four Markdown
files/11 local targets, default/optional/application dependency isolation and
unchanged productive closures/pins/locks, plus FIVE actual original-owned scoped
child cases. The retained spectral fixture completes five geometry cycles with
exactly15 later Rust payload requests (features, all-four masks, output), live0,
failures0 and peak951216B in1MiB backing. This proves fixture request ownership
only;1MiB is NOT a proposed full-model arena or native/process allowance.

ONE actual full485100/1101 spectral process produces byte-identical archived
prior-Release features and PCM: feature SHA256
`d28a7f55356334f279f2c1eea899bf347a6e9e274decba6ec7e65161f44d8523`, PCM SHA256
`f180af0c2fafba1a05915d0994dfe6860deb055da212267059714ab50893c0d6`.
Together with bitwise prepared/allocating Rust comparisons, this checks unchanged
spectral kernels, not learned neural outputs. Three actual CLI bad-header cases
prepare the retained workspace and then reject BEFORE tensor loading/NN; track,
chunk and chunk-with-original-features leave1,3,4 EMPTY files respectively, ALL
INVALID. The nine unchanged STEP31 preload negatives and retained-I/O control
are source-compatible reuse, not additional fresh cases here.

One observer precedes one joined executor:5/5 original scopes closed, pending0,
inflight0 and sampling errors[]. No full-model admission query, learned/oracle
or CUDA was launched. Actual Windows x64 Release diagnostic1221632B SHA256
`0dd744b0e69be1154df0fd24bcc38e53cb23565925dc5585d2e4112a86278d2c`, spectral1056768B
SHA256 `6304c92ddb110a613fd66a8bbf0bf885c22d5677fa866f5db553fb7c3d7e809f` are component
evidence ONLY, not Compiler/FullProducer/MappedImages/nonWindows/wholeNN/quality/
distribution. Reconstruction uses `bs_learned --spectral-workspace-control`, the
existing complete spectral diagnostic and bad-header recipes under the original
admitted command owner. Generated logs/fixtures remain reconstructible cache;
source/pins/this design retain durable facts. No speed/RSS or installed gain is
claimed. Systems PRE/frozen POST, Methods and ONE independent final actual-diff
chain review this scoped correction; all readers must end before publication.

## STEP31 impact: retained diagnostic I/O and allocation schedule

The CLI now moves actual directory/path creation, exclusive output
handles and their fixed buffered writers before the execution allocator seal.
Standard output/error locks are initialized before that boundary and retained
through terminal reporting. Sealed failures retain a static phase and numeric OS
code or the original core error; they do not format a platform error message.
CLI track callbacks retain their first I/O detail across the core Source/Output
boundary. The generic borrowed model decoder keeps its preexisting core Source
error; raw OS-code propagation through every library reader is not claimed.
This changes ownership in the real learned CLI, without changing a network
operator, source geometry, precision, quality tolerance or productive runtime.
Existing directories are rejected unchanged. Every newly created empty/partial
file remains private and invalid unless the ENTIRE attempt succeeds.

### Original source schedule checked before this correction

The following describes actual calls in `learned.rs`, `lib.rs`, `track.rs` and
the diagnostic, at original485100 frames/1101 steps, not the older summed
`activation_bytes` allowance. Requests are `try_reserve_exact` unless the pinned
library says otherwise. Counts are float32 elements below; multiply by4 for bytes.
Let S=1101, F=S*4100=4514100 and X=S*62*384=26212608.

| Phase | Live owners and requests |
| --- | --- |
| Model load | Monotonic1355 tensor requests, total131704612 floats;62 Band records,8 Transformer pairs,4 Vec records and4*62 Mask records in7 separate metadata vectors. Header8 is streamed, not a retained weight tensor. Decoder uses16384 stack bytes. |
| Analyze | Caller PCM, one Transform (FFT plan/Arc, window2048, Complex32 buffer2048 and actual scratch), then F features. Transform dies on analyze return. Diagnostic PCM then dies; track input remains borrowed from its persistent core. |
| Band latent | F features and X axial; selected S*width and normalized S*width overlap; selected dies before projected S*384 plus SGEMM packing. Each band temporary dies before the next band. |
| Time attention | X plus selected S*384; normalized S*384, QKV S*1536, gates S*8; normalized dies before attended S*512, three S*64 head vectors,16*S scores and16*64 tile output. Packing is live during each synchronous multiply. QKV/gates die before final S*384 projection. |
| Frequency attention | Same calls on each borrowed62*384 row, with length62; no second complete axial output. Each time/frequency reader finishes before reuse. |
| Feedforward | Selected/borrowed input plus normalized L*384 and expanded L*1536; normalized dies before contracted L*384. Expanded dies before residual add. L is1101 or62. Final axial norm is in place. |
| All-four diagnostic masks | F features, X latent,4F masks, selected S*384, hidden S*768 and contracted S*(2*width), plus synchronous packing. Hidden dies on project return; projected/selection die before the next band. Latent dies when masks returns. |
| Diagnostic inverse/output | F features+4F masks+4*2*485100 output; one inverse Transform plus two487148-float accumulation/envelope vectors. All first-forward owners die after writes and before optional original-feature forward. That forward owns F features and the same all-four mask schedule. |
| Track persistent | Six vectors: batch input2*2*485100, source2*485100, ring8*727650, counter727650, output8*485100, window485100:55301400B. Complete batch2 prediction is31046400B, allocated ONCE before serial samples. The earlier sample already lives inside it. |
| Track sample projection | Persistent core+complete batch destination, F spectrum, X latent, one inverse Transform+two487148-float vectors, one F mask and the same selected/hidden/contracted projection. Inverse remains live during projection. No returned sample Vec or additional borrowed-input copy. |
| Track batch retirement | All sample temporaries retire before the next sample. Complete prediction retires after add, before flush/next batch. No fulltrack-length output Vec exists. Retained I/O must not interleave persistent arena allocations into these transient lifetimes. |

Pinned matrixmultiply0.3.10 std/nonthreaded packing uses Rust `std::alloc` and
therefore the actual arena. Its source maximum1081344B and per-kernel alignment
requests up to32B remain separate from vectors. No MATMUL compile overrides/threading are
assumed. Model metadata must use the actual target `size_of` values, not a stable
Rust ABI assumption. On LP64 the reviewed candidate sizes are Band88,
Transformer392, Mask128 and Vec24; these are not portable attestation.

The actual first-fit arena charges a16B header, absolute-address alignment and
16B rounding; a tail shorter than32B is consumed. It returns the complete charged
extent, with sorted adjacent coalescing. Default realloc allocates the new block
BEFORE returning the old one. Thus live-payload maxima cannot prove capacity.
1355 tensor+7 metadata allocations alone need at least21792 header bytes; weight
payload+candidate metadata+headers is526883808B before fragmentation/alignment.
No smaller arena is selected from these arithmetic observations.

RustFFT6.4.1 chooses AVX/FMA, SSE4.1 or scalar planning for2048. The AVX branch
has a boxed planner, two direction-cache HashMaps, radix Vec, aligned Butterfly256 Arc,
14336B twiddle Box and mixed-radix Arc. SSE/scalar planning adds recipe/cache and
butterfly ownership; its twiddle Vec shrinks into a boxed slice, so old/new
storage can overlap in this allocator. Exact pinned HashMap/Arc layouts, Vec
growth, planner branch and the full finite allocation/free sequence still need
reconciliation against first-fit holes. The older1MiB `PLAN_ALLOWANCE` is not
that proof. Repeated track chunks can reuse coalesced space only after ALL their
readers end; post-run counters cannot establish this prospectively.

### Process boundary and retained remainder

The source-owned late native calls being removed are filesystem path conversion/
open/create, first-use stdio initialization and platform error formatting.
Rust1.95 [OS error formatting](https://github.com/rust-lang/rust/blob/1.95.0/library/std/src/sys/io/error/windows.rs)
calls FormatMessageW with a stack UTF16 buffer, optionally GetModuleHandleW;
it does not request FORMAT_MESSAGE_ALLOCATE_BUFFER. Its
[stdio](https://github.com/rust-lang/rust/blob/1.95.0/library/std/src/io/stdio.rs)
initializes stdout lazily. Preseal ownership removes those later call chains and
persistent Rust buffers from the transient arena, rather than inferring safety
from an observed no-growth run.

This does NOT close synchronous native read/seek/write/flush/close, native math/
CRT/TLS/loader or process-exit behavior. Actual before-NN retained heaps, original
2MiB stack reserve, private image COW and every later native allocation remain
unbounded here. A Windows region/image ledger is conditional on proving no new
regions/threads/loader/native backing; a snapshot alone is insufficient. Arena
backing already inside PrivateUsage must NEVER be added twice. There is no full
arena capacity, complete process allowance or fresh sufficient ORIGINAL admission
yet, so no NN/oracle/CUDA retry is authorized by this correction alone.

This necessary I/O lifetime correction grows the plan to33: STEP32 retains the
ENTIRE former nativeREST31; FINAL33 the ENTIRE former FINAL32. Genuine complete
learned original masks/PCM at mask atol/rtol3e-4 and PCM atol3e-5/rtol3e-4,
fullgeometry/batch2/fulltrack, mandatory actual native NVIDIA CUDA with meaningful
changed model/operator/runtime and fresh complete admission, same-model actual
devicefault -> Tree AND Reader END -> fresh CPU, COMPLETE Demucs, ALL roles,
Windows EXE AND honest actual Linux/macOS, CPU/CUDA releases/CRT/licenses/LGPL
source3patch/NVIDIA notices/installer/recovery/fullfootprint, quality/functions/
interactions/resources/cancel and every final gate remain. All old terminal CUDA
failures/cause UNKNOWN, fullhost/validator/32keys,32 parent failures, distinct216
watchdog, registry/pressure UNKNOWN and Overlap OPEN remain. Productive pins/
Torch/closures/installer and installed savings ZERO remain unchanged. No arithmetic,
preparation, allocator control or static schedule is learned acceptance.

### Actual verification boundary

Final outer-entry verification PASS3 in11.07s: seven Rust unit controls (the four
existing allocator cases plus three typed-error/first-callback-failure controls),
strict all-target Clippy, Release build, Rustfmt, strict OpenSpec, four Markdown
files/11 local targets and unchanged productive/dependency isolation. The original
22 core/spectral/track controls have unchanged sources and are reused, not a new
22-case run. External Release Native/API PASS with the same22 known warnings;
the existing einops pin/warning and Human App/held Native are unchanged.

Thirteen actual scoped child cases PASS: one retained-I/O control, nine preload
negatives and three bad-initial-magic cases. The control prepares/retains four
exclusive outputs, writes exact `[0.25,-0.5,1.0,-0.0]` little-endian float32 bytes,
and observes a real read-only-handle write failure, OS code5 on this Windows run.
The arena counters are unchanged across these writes and their status report.
This is fixture Rust-allocation evidence ONLY, not native no-growth or a process
allowance. Its1MiB backing is not a proposed full-model capacity.

All nine preload negatives exit1 with empty stdout, no fresh target and an
unchanged existing sentinel. Correct-size but bad-magic weight fixtures reject
before tensor loading/network execution in track, chunk and chunk-with-original-
features modes. Their prepared private directories contain respectively1,3,4
empty files: ALL are explicitly INVALID, not successful or recoverable outputs.
One observer precedes one joined executor;13/13 scopes closed, pending0,
inflight0 and sampling errors[]. No original full-model admission query, NN,
oracle or CUDA was launched. Floors, caps, geometry and tolerances are unchanged.

Actual Windows x64 Release diagnostic1210368B SHA256
`b1283065f5a81744ca967535c518a95191cffc9ef1e0ab21f02b5f8d91acebbc` is component
evidence only, not Compiler/FullProducer/MappedImages/nonWindows/learned/quality/
distribution proof. Reconstruction uses `bs_learned --io-control NEW_DIRECTORY`
under the original admitted command owner, plus the documented preload and
bad-header cases. Reconstructible build/test logs and fixture bytes stay cache;
the source/pins/design retain durable facts. This correction claims no measured
speed/RSS improvement or installed saving.

Index verification separately checks all8 LF staged files equal the authoritative
source, no unrelated work/private bindings/hidden flags, unchanged five local BAT
bridges/pointer/exclusion, and strict OpenSpec/11 local Markdown targets. Systems
PRE/frozen POST, Methods and one independent final actual-diff chain cover this
narrow ownership correction; all reader scopes end before publication.

## STEP30 impact: bounded reusable diagnostic allocation ownership

The isolated `bs_learned` executable now owns a fixed execution arena through
Rust's supported `GlobalAlloc` boundary. After activation, every subsequent Rust
allocation uses that backing, including unchanged learned/track vectors,
RustFFT planner/Arc/cache storage and pinned matrixmultiply0.3.10's internal
aligned SGEMM packing. This avoids rewriting numerical operators or vendoring
the matrix kernel. The productive application and library allocator are unchanged.

The arena keeps allocation metadata inside its backing, aligns absolute addresses,
returns alignment padding on free, splits free extents and coalesces adjacent
freed extents. Its allocation-free atomic lock serializes callers. Recycled
zeroed allocation and realloc retain the standard Rust semantics, including old
storage surviving a failed realloc. Pointers allocated before activation remain
System-owned; their realloc can migrate into the arena. Exhaustion NEVER falls
back to System. Fallible vector APIs retain their existing phase/byte errors;
infallible library or error-message allocation may abort terminally if the backing
is exhausted. Neither is a CUDA devicefault
or authority for fallback/publication. Backing is retained until child exit, after
every allocation and reader; only isolated local unit-test instances release it.

The actual learned CLI requires `FLITZIS_BS_WORKSPACE_BYTES` before model loading.
The external admitted owner must supply a source-backed aligned extent and retain
the original Job promise through actual Tree AND Reader END. The executable does
not derive a budget from free RAM or supply a guessed default. This environment
value is a backing size, NOT evidence of complete admission or a valid numerical
run. Pre-load malformed-input checks still precede arena/model allocation.

The separate `bs_learned --workspace-control` is an implementation control only:
it exercises actual pinned SGEMM packing and complete small spectral identity
reconstruction under a bounded arena, without weights or a learned prediction.
Its fixed fixture capacity is NOT a proposed full-model process allowance.

### Remaining complete-process and execution proof

The arena provides a hard bound on later Rust allocation backing/retention. It
does NOT prove that a chosen size accommodates the complete model allocation
schedule, fragmentation, headers/alignment or realloc old/new overlap. It also
does not bound pre-existing retained process heaps, remaining original stack
reserve, private image COW, CRT/TLS/loader or later direct OS/native allocations.
Current PrivateUsage plus arena capacity would double-count the already committed
backing and still miss future non-Rust growth. Post-run no-growth observations
cannot retroactively create a missing prospective allowance.

Next establish the exact full-geometry allocation schedule against this real
allocator, initialize/audit the actual remaining process closure, and bind all
later non-arena allocations. Windows reserved/committed regions and full image
extents may supply a conservative actual backing ledger ONLY with a source-backed
no-new-region/thread/loader/native-allocation closure. No arbitrary MiB margin,
smaller logical inventory or repeated admission query substitutes for that work.
Only adequate fresh ACTUAL ORIGINAL complete admission then permits the genuine
original masks/PCM/full485100/1101/batch2/fulltrack comparison. No such full-model
arena capacity or complete process allowance is claimed here; no NN was launched.
CPU allocator ownership does not qualify as changed CUDA model/operator/runtime.

This necessary allocator-ownership implementation split grows the plan to32.
STEP31 retains the ENTIRE former nativeREST30; FINAL32 retains the ENTIRE former
FINAL31, including complete practical learned execution, mandatory actual native
NVIDIA CUDA/devicefault-to-freshCPU, COMPLETE Demucs, every provenance/platform/
distribution/quality/resource/cancellation/final gate. Productive Torch/pins/
closures/installer remain unchanged; installed savings ZERO. Original tolerances,
geometry, precision, Job limits/floors and all previous failures/UNKNOWN gates
remain unchanged. This is implementation progress, not full learned acceptance.

### Actual verification boundary

Four isolated allocator controls PASS, no ignored cases: mixed alignment and
fragmented/coalesced reuse with live-payload sentinels; recycled zeroing and
realloc growth/shrink/failure preservation; pre-activation System migration and
exhaustion/overflow refusal; eight concurrent test callers with1600 settled
allocations. Strict all-target Clippy and Release example build PASS. These are
allocator tests, not a release network execution or changed model precision.
The unchanged STEP29 core/spectral/track controls are source-compatible reuse.

The first process-control harness used32 bytes for the existing-directory
negative. It aborted3221226505 while allocating168 then30 bytes for path/error
handling, BEFORE weights/model loading. Original observer7/7 scopes closed,
pending0/inflight0/errors[]. This was a deliberately bounded fixture with an
incorrectly small setup, not host exhaustion, a learned failure phase, an old
failure cause or a devicefault. The fixture alone changed to1MiB; assertions and
original Job/resource rules were preserved. Earlier partial diagnostics remain
invalid without complete successful exit. No global process limit was changed.

Final outer-entry checks PASS3 in7.49s, including comment-delta Release rebuild,
strict OpenSpec/document links/dependency isolation and actual process controls.
The final binary runs64 genuine SGEMMs and eight spectral identity reconstructions
under1MiB backing: peak368208B INCLUDING allocator metadata/alignment, live0,
259 allocations, failures0. These are fixture arena statistics, NOT process
RSS/commit, full-model capacity or measured speed/memory improvement. All9 actual
pre-load negatives exit1/stdout empty, create no fresh target and leave the
existing0B output unchanged. One observer preceded one joined executor;10/10
scopes closed, pending0/inflight0/errors[]. No weight/model NN or CUDA was run.
No new full-model admission query was justified while process closure is open.

Final Windows x64 Release diagnostic1213440B SHA256
`1ca34851d682c96880d2e54e43fef2907e6b9a4365c14c5a72ba302c2deb122d` is component
evidence ONLY, not Compiler/FullProducer/MappedImages/nonWindows/learned/quality/
distribution attestation. Outer external Release Native/API PASS with22 known
warnings and unchanged einops warning/pin; held Human Native/App remain untouched.
Systems PRE/frozen POST, Methods and the single independent actual-diff chain
cover this scoped correction. Generated logs/control outputs remain cache;
source, pins, this design and the recipes retain the durable reconstruction facts.

## STEP29 impact: stem-wise learned reconstruction ownership

The experimental CPU chunk/track path now retains one complex stem mask instead
of all four simultaneously. The unchanged complete latent forward is factored
from projection. Each independent head uses the same SGEMM dimensions/reductions,
Tanh and GLU operations; only independent stem iteration moves outside the band
loop. Public `masks` still supplies the complete original four-mask diagnostic.
`chunk_into` reconstructs each mask into the caller's exact planar stem slice and
reuses its single mask buffer only after the last synchronous inverse reader.
`track_batch` initializes
the complete batch result and fills its sample slices directly, removing the
separate returned sample Vec and append. Release485100/1101, batch2, float32,
full noncausal attention and all quality/resource policies remain unchanged.

One `Reconstruction` borrows the spectrum and owns one FFT plan/window, inverse
envelope and accumulation across all stems. The existing allocating inverse uses
the same per-channel arithmetic. Exact destination extents and nonfinite masks
reject before writes. Cancellation/overflow may leave private partial slices;
ANY later error invalidates the ENTIRE batch/attempt and previously written files.
Mask projection recopies the selected band per independent stem; no runtime gain
is claimed. No publication, fallback, source/OwnACK/FinalCAS/last-reader or realtime
authority
is added. Axial state deliberately remains live throughout inverse work.

### Full simultaneous source scope and remaining process barrier

At the original geometry, persistent weight float payload526818448B + fixed track
vectors55301400B + maximum complete batch destination31046400B =613166248B.
Borrowed sample PCM is already inside track input; earlier sample output is already
inside batch capacity. Do not count them again. The spectrum adds18056400B and
axial state104850432B, for736073080B before active stage scratch. Exact vector
requests exclude allocator capacity rounding/metadata and retained freed pages.

| Additional simultaneously live vectors | Payload bytes |
| --- | ---: |
| Widest band selection + normalization |4544928|
| Time attention selected/QKV/gates/attended/Q/K/V/scores/tile |11665888|
| Time feedforward selected + expansion + contraction |10146816|
| One stem mask |18056400|
| Widest mask selection + hidden + projected contract |9618336|
| Inverse envelope + accumulation |3897184|
| Inverse window + FFT buffer + maximum admitted scratch |90112|

The later mask-projection phase includes the already-live inverse context:
736073080 +18056400 +9618336 +3987296 =767735112B listed logical payload.
The corresponding STEP28 listed phase was817917016B without inverse scratch.
The three avoided masks remove54169200B; keeping3987296B inverse vectors overlaps
projection now, so the listed phase difference is50181904B. FFT plan/metadata are
still separate. The avoided returned sample15523200B belongs to another phase;
it MUST NOT be subtracted a second time from this projection comparison. The
inverse phase retains axial state, spectrum, single mask, batch and inverse work;
it does not allocate another sample output. Public all-mask diagnostics retain
their larger original envelope; no global allowance is silently reduced.

Existing conservative vector allowances1239700128B and conditional SGEMM packing
payload1081344B remain unchanged, total1240781472B BEFORE unproved process overhead.
The std-only/nonthreading matrixmultiply0.3.10 packing bound still requires actual
compile settings, including absent MATMUL_SGEMM tuning overrides. Rust's Windows
System allocator uses GetProcessHeap/HeapAlloc/HeapFree; source-live Vec bytes do
not bound retained heap segments/fragmentation. RustFFT plans/caches, metadata,
packing alignment, stack, CRT/loader/private image pages and I/O/runtime allocations
also need a complete allowance. The prior1MiB FFT-plan allowance is conservative
caller bookkeeping, not a proved complete process bound. No arbitrary smaller
estimate or extra MiB margin can close this gap or explain older failed runs.

A genuine preallocated/reused execution workspace, including supported SGEMM
packing ownership and all FFT/track/model work, is a possible next source strategy.
It would need to establish actual before-NN process state under the original Job
and bound every later allocation. This is remaining implementation/proof, not an
accepted loader probe, full learned execution or guaranteed next result. Until
complete allowance and genuinely sufficient fresh ORIGINAL admission are proved,
no constrained unchanged model/oracle/CUDA retry is justified. CPU lifetime work
alone never changes the failed CUDA model/operator/runtime condition.

### Verification and delivery boundary

Fresh focused Rust controls PASS22:10 unit (including a small synthetic projection
operator/layout fixture, NOT release learned execution),7 spectral and5 track.
The new layout control covers all62 offsets/four distinct heads/33-row tile tail
against the original band-outer projection arithmetic, exact bits. Split inverse
controls cover asymmetric complex stereo masks, odd tails1025/2501/4099, arbitrary
stem order and destination sentinels. Invalid extents/NaN and cancellation after
one channel has changed retain terminal failure and private partial output.
Existing independent identity/scaling arithmetic controls remain. Strict all-target
Clippy and Release examples PASS. After the early public-chunk finite/cancel gate
and single mask-buffer reuse, the10 affected unit controls, strict all-target
Clippy and Release examples PASS again; the7 spectral/5 track results are
source-compatible reuse, not a new22-case final run.

This necessary ownership split grows the plan to31. NEXT30 retains ENTIRE former
nativeREST29; FINAL31 retains ENTIRE former FINAL30. Complete learned execution/
original masks+PCM, batch2/fulltrack/AMP, mandatory native NVIDIA CUDA and real
same-model devicefault -> actual Tree AND Reader END -> fresh CPU, COMPLETE
practical Demucs, all source/frontend/graph/weight/export/runtime/provider/native/
compiler/binary/fullproducer/mapped-image/OSarch roles, CPU/CUDA release bundles/
CRT/licenses/LGPL source+patches/NVIDIA notices/installer/recovery/full footprint,
quality/functions/resources/cancel and all original final gates remain REQUIRED.
Productive Torch/pins/closures/installer are unchanged; installed savings ZERO.
Original fullhost UNKNOWN,32 parent Rust failures/distinct watchdog/registry
UNKNOWN, pressure cause UNKNOWN and overlap/device/listening/distribution gates
remain open with their original evidence limits. No complete learned acceptance,
measured RSS/peak/speed gain or old failure cause is claimed by this step.

### Final actual receipts

The final outer-entry run PASS4/4 in28.90s: affected Rust/Clippy/Release delta,
static/spec/dependency isolation, original Job reconciliation and one joined
executor covering8 spectral arithmetic processes plus6 pre-load learned negatives.
Prior/current Release binaries produce exact feature AND PCM bytes for1025/2501/
4099/485100 frames (3/6/10/1101 steps) on the same asymmetric stereo/complex-mask
inputs. This proves preserved spectral arithmetic, NOT learned prediction.
Full485100 output SHA256 is
`f180af0c2fafba1a05915d0994dfe6860deb055da212267059714ab50893c0d6`;
feature SHA256 is
`d28a7f55356334f279f2c1eea899bf347a6e9e274decba6ec7e65161f44d8523`.
All6 pre-load negatives exit1/stdout empty, fresh target absent/existing0B unchanged.
Original observer started BEFORE the one joined executor:14/14 scope closes,
pending0/inflight0/errors[]. These are arithmetic/pre-load controls, no model NN.

One actual ORIGINAL reservation opened/query/closed without child:740837376B.
Adjacent host available commit3629158400B and physical7957479424B are separate,
non-atomic observations. The original ceiling is below even the new listed
767735112B payload, before unknown process overhead, and below the unchanged
1240781472B conservative vector+packing allowance. COMPLETE admission is NOT
established; NO learned/oracle/CUDA was launched. No inference about older causes,
actual minimum required memory or safe smaller floor/cap/budget follows.

Actual Windows x64 learned Release component1204224B SHA256
`e25cf88edad4e408b33c4a9db7951a15908c9d1edf9d40cef07c1353a4eaa62f`;
spectral component1058816B SHA256
`376e030b58fede1c36d47d2a267d40daa9cbe13275cbd2adebc26c4069ed2d31`.
These are component receipts, not Compiler/FullProducer/MappedImages/nonWindows/
whole-model/quality/distribution proof. External Native/API check PASS with22 known
library warnings and the unchanged pinned einops warning; held App Native is not
updated. Systems PRE/frozen POST and Methods review cover this change; the single
independent FinalActualDiff chain covers actual final source and receipt delta.
Generated build/final/index logs and arithmetic payloads remain reconstructible
cache; this design, tracked source, fixed pins and recipes retain durable facts.

## STEP28 impact: allocation attribution and learned buffer lifetimes

Every fallible spectral, track and learned vector reservation now retains its
owning phase and checked logical requested bytes in the terminal allocation
error. This includes fixed-schema weight metadata and diagnostic PCM/original-
feature vectors. Byte multiplication overflow remains a shape rejection before
allocation. The diagnostic does not identify allocator overhead, a total process
peak, Job/host pressure, physical VRAM exhaustion or the cause of older failures.
FFT planning, SGEMM internal packing and ordinary process/library allocations
remain separate; their failures are not falsely relabelled as these vector errors.

The final synchronous readers of QKV/gates return before attention output
projection; those owned buffers now drop before that projection. Band selection
drops after normalization, band normalization after projection, FF expansion
after contraction, and mask hidden state after contraction. Residual inputs and
all arithmetic/reduction order are preserved. The chunk diagnostic writes its
first private features/masks/PCM, then drops their buffers before the independent
original-feature forward. Any later failure invalidates ALL those private files;
exit0 and complete output remain necessary. No application publication is added.
These are source-backed shorter lifetimes, not measured RSS/peak/runtime gains.

### Full geometry and original admission reconciliation

For the original485100-frame/1101-position geometry, weights remain526818448
float32 payload bytes (526818456 file bytes including the header). Whole-track
fixed vectors occupy55301400B; maximum batch2 result capacity adds31046400B,
even while one independent sample is computed serially. The active Spectrum
owns18056400B. The axial state owns104850432B and all four returned masks own
72225600B. These lifetimes overlap during mask generation; masks cannot be
counted as a replacement for axial state or spectrum. At the widest mask band's
contraction, selection/hidden/projected vectors additionally occupy1691136/
3382272/4544928B. This phase's listed payload sum is817917016B, NOT a complete
process upper bound. Earlier attention/FF and inverse phases, FFT plans, metadata,
allocator retention/fragmentation, I/O buffers, stack/code/mapped images and
process runtime still require allowance. A returned sample also overlaps the
preallocated complete batch capacity while being appended.

The existing deliberately conservative allowances remain unchanged:

| Included source allowance | Bytes |
| --- | ---: |
| Fixed-schema weight file (header conservatively included) |526818456|
| `activation_bytes(1101)` |414792000|
| `Geometry::transient_bytes()` |114721872|
| `TrackPlan::pcm_transient_bytes()` |183367800|
| Conservative sum, with intentional overlap/double counting |1239700128|

The pinned matrixmultiply0.3.10 std-only, nonthreading/default-configuration
SGEMM packs one A and B buffer per synchronous call. Its source formula is
`min(k,256) * (roundup(min(m,64),MR) + roundup(min(n,1024),NR)) * 4`;
our calls have at most32 rows and x64 MR/NR at most8, giving at most1081344B.
The internal allocator is infallible and separate from the new vector diagnostic.
This bound requires the unchanged effective feature tree and absent compile-time
MATMUL tuning overrides; it is not universal compiler/producer/OS attestation.
Vector allowances plus that packing bound total1240781472B. Process/allocator
overhead has NO proven complete hard bound; an invented safety margin would not
turn this source inventory into a measured release maximum.

The actual ORIGINAL `OfflineChildMemoryReservation` was opened ONCE, queried and
closed without launching any child: limit929548288B. The adjacent host sample
reported caller/system available commit4006580224B and physical7889739776B.
They are distinct observations, not one atomic snapshot. Original admission keeps
2GiB commit/512MiB physical floors and halves remaining allowance after existing
Native promises, minimum128MiB, retaining the promise through actual Tree AND
Reader END for any launched process. No cap, floor or Job budget changed.
This fresh ceiling is below the conservative vector/packing approach even before
process overhead: complete admission was NOT established, so NO new NN/oracle/
CUDA process was launched. This does not prove that actual allocations require
the conservatively double-counted sum, nor explain any STEP27 failure.

All19 Rust controls PASS (new deterministic reserve-layout/byte-overflow control
plus the18 existing learned/spectral/track controls); strict all-target Clippy and
Release examples PASS. The new rejection control never asks the OS for a huge
allocation and is not an induced OOM. The existing numerical/cancellation proofs
remain compatible within their original limits. Productive dispatch, dependency
locks, installer/CPU/CUDA closures and held application Native remain unchanged.

Final static/pre-load checks PASS2/2 in6.31s: strict OpenSpec, three changed
Markdown files/10 local targets, unchanged productive sources/locks, default/
optional/app isolation and the effective nonthreading/default SGEMM features.
All six real pre-load CLI negatives exit1 with empty stdout, no fresh output and
unchanged existing0B output. The original observer surrounds one joined executor:
6/6 scopes closed, pending0/inflight0/errors[]. These are pre-load failures with
no model/NN, not learned execution. A frozen Systems review found the separate
spectral diagnostic reader still lacked attribution; its PCM/mask reader now
also checks byte overflow before I/O and reports phase/bytes. The focused final
all-target strict Clippy/Release spectral-example delta PASS1.18s; source-compatible
19 core controls and earlier learned/track example builds are reused.

The actual Windows x64 Release learned component is1197056B, SHA256
`3df161b6f221ace85ba932118c72ec83d6506e68ae3fb9041b74c56ceea59ead`.
This is not complete compiler/full-producer/mapped-image or Linux/macOS evidence.
External Native/API checks retain22 known library warnings and the pinned einops
warning; no held application extension is replaced. Systems PRE/frozen POST,
Methods and one independent actual-diff review chain cover this coherent change.
Reconstruction uses the existing checkout-aware entry, focused stem crate tests,
strict all-target Clippy, Release examples and original admitted pre-load runner.
The disposable `performance-step28-{build,final,delta}` logs retain raw output;
this design/source/pins retain the durable facts. All three STEP27 execution
failures, their UNKNOWN phases/causes and unpassed original tolerances remain
unchanged; this step adds no completed learned prediction or quality acceptance.

This coherent resource-attribution/lifetime split grows the plan to30. NEXT29
retains the ENTIRE former nativeREST28 and FINAL30 the ENTIRE former FINAL29.
Practical complete learned execution/original masks and PCM, mandatory native
NVIDIA CUDA/fresh CPU, complete Demucs and all role/platform/distribution/quality/
functions/resources/cancel/final gates remain mandatory. The next real model
attempt needs a source-backed complete lifetime allowance and genuinely sufficient
fresh ORIGINAL admission; do not repeat a constrained unchanged attempt. Further
CPU source work alone does not justify another failed CUDA retry. Installed
savings remain zero and original fullhost provenance remains UNKNOWN.

## STEP27 impact: independent learned BS CPU network

The opt-in `stem-dsp/learned-trial` candidate implements the complete release
learned network candidate, reusing the delivered spectral frontend/reconstruction
and track orchestration. The delivered effective-state exporter interprets the
ONE SHA-pinned original checkpoint with a bounded inert stdlib ZIP/protocol2
descriptor decoder, without importing Torch, NumPy or ONNX. It checks the same
configuration/checkpoint pins as the original App loader. All1355 tensors/
131704612 floats occupy526818456 exported bytes; this
authenticates trial file bytes, not a complete producer. Shared rotary aliases
use each axis's last block7 alias, reproducing original default copy_ traversal.
The optional original Torch oracle still exports state after strict loading.

The native path preserves62 bands/dim384, eight time-then-frequency blocks with
eight64-wide heads, original L2 norm clamp1e-12, adjacent rotary pairs, normalized
input gates after noncausal SDPA, two residuals/GELU FF1536, final norm and four
independent Tanh/GLU mask heads with hidden768. Existing matrixmultiply0.3.10
SGEMM has no threading feature; a checked slice/stride wrapper isolates unsafe
calls. Attention uses16-query tiles, retaining ALL1101 release positions without
the approximately4.81GB batch2 score tensor of an unfused graph. No precision,
attention extent, chunk or original admission floor is reduced.

All work stays offline with immutable borrowed inputs and owned fixed-schema
weights. Complete original process admission remains mandatory through actual
tree AND reader end. `activation_bytes` is a conservative visible vector-payload
allowance only; weights, spectral/track buffers, SGEMM internal packing/allocator,
process/stack/code and runtime overhead remain separate. It is not RSS/admission
or a measured release peak. Serial independent samples in the existing batch2
predictor preserve geometry but require actual original batch2/wholetrack parity.
No callback, scheduler, source/content/OwnACK/FinalCAS, UI or productive dispatch
path changes. Private diagnostic output is invalid after any later failure.

The original learned-mask and full-PCM comparison remains UNPROVED. Its actual
CPU attempt failed importing Torch, before export/model initialization/NN.
Predeclared diagnostic mask atol/rtol3e-4 and PCM atol3e-5/rtol3e-4 were never
relaxed and have NOT passed. The optional oracle must run as a repository module
(`python -B -m scripts.bs_learned_trial_reference CHECKPOINT CONFIG NEW_DIRECTORY`),
with the authenticated checkout on its import path, complete original admission
and `CUDA_VISIBLE_DEVICES=-1` BEFORE import. STEP16's logged original BS references
remain historical evidence, but their raw input/WAV bytes are absent from the
relevant retained test cache; hashes alone cannot supply a new numeric comparison.
Compatible STEP25/26 math and old CPU/cancel/failed-CUDA evidence is reused.

### STEP27 actual export, controls and failed execution boundaries

The stdlib path verifies exact527385512 checkpoint and4566 configuration bytes/SHA
from retained SAME opened handles BEFORE interpreting metadata. The exact1345
stored, unencrypted ZIP members include278035B protocol2 metadata and1341 CPU
FloatStorage payloads. Unique names, bounded/fixed metadata extents, byteorder
little/version3, direct1355-name tensor schema, float32, exact shape/contiguous
stride/offset0/storage extent, conflicts and default integer module-version1
metadata are checked. Unknown globals/reducers/BUILD/flags/hooks fail closed.
`pickletools` only tokenizes; the interpreter constructs local inert descriptors,
never calls pickle.load, an input-selected constructor/reducer, eval or Torch.
Every admitted/superseded storage reaches exact finite/CRC EOF; output is exclusive
and invalid after later failure. Caller-owned immutable custody remains required;
same-handle hashing is not Windows write denial or complete producer attestation.

Actual exported526818456B SHA256:
`51619069b73bfcbdc444a16a4ebc8cf53ea8d3466fce67b594412a02b854cbdf`.
The export child returned0 in3.7239202s with empty stderr and actual module-closure
check excluding Torch/NumPy/ONNX. This is a weight-file boundary, NOT learned
arithmetic/quality or productive provenance. Reconstruction uses
`python -B scripts/bs_learned_weights.py CHECKPOINT CONFIG NEW_WEIGHTS` and the
explicit `bs_learned WEIGHTS SHA track INTERLEAVED_F32 FRAMES NEW_PRIVATE_DIRECTORY`.
Local actions retain the existing checkout-aware outer entry and original admitted
owner; no new runtime, process manager, working path, installer or productive pin.

| Actual attempt | Result and exact limit |
| --- | --- |
| Original App Torch CPU oracle |5.2892798s, exit1/MemoryError during `_meta_registrations` import, stdoutempty, no original export directory/model/NN. Before availablecommit4006535168; childpeakcommit927858688. Original1/1scopeclosed/pending0/inflight0/errors[]. |
| First native complete-track attempt |1025 physical frames; original full485100 padding/1101 attention positions requested.146.6121437s, exit1/Allocation, stdoutempty, only empty private output. Mainpeakcommit834088960/WS726884352; exact allocation phase/causeUNKNOWN. The combined export+native observer proves2/2scopeclosed/pending0/inflight0/errors[]. |
| Changed native final-RMS attempt |0.8110986s, exit1/Allocation, stdoutempty, exclusive directory remains empty with NO output file. Before availablecommit3012620288; mainpeakcommit433446912/WS431448064. Original1/1scopeclosed/pending0/inflight0/errors[]. Different progress/headroom; NOT a speed/peak-improvement comparison. |

Both native attempts use synthetic interleaved1025-frame source SHA256
`868c43ce52cec3359a74baa50898dd0d92118f9df4f3f01b19291e37d4fdae74`.
Neither completes a prediction or supports any full learned execution/numerical/
quality claim. Allocation/import failures are terminal host/resource boundaries,
not CUDA/device errors or fallback receipts. Actual causes remain UNKNOWN;
no floor/cap/precision/geometry/memorypattern, foreign process, OS or pagefile was
changed. A further model attempt needs meaningful source/condition change and
fresh complete original admission; do not repeat these constrained failures.

The final RMS now computes each entire row's L2 BEFORE mutating the exclusively
owned axial buffer, with exact old f32 divide→scale→gamma order and checks around
rows/end. All synchronous axial readers have ended; residual attention/FF paths
still retain copying norms. This removes a104850432B destination allocation at1101
by source inspection. It does NOT identify the earlier failure phase/cause or
prove actual RSS/peak improvement. A384×4-row bitwise owned-vs-in-place control
includes distinct/zero/subepsilon/signed-gamma rows plus partial cancellation and
finite-overflow rejection; private partial mutation never publishes output.

Checks: final8 learned controls PASS; unchanged5 spectral+5 track controls are
reused (18 distinct successful Rust controls, not one new final18-case run).
Strict all-target Clippy/Release builds and Ruff/format/strict typing pass;
11 Python metadata/gadget/CRC controls PASS. Original numerical harness FAIL
and both native completion attempts FAIL remain separate real receipts. Initial
lock/bootstrap, import/type/complexity/test-collection/format findings were fixed
without suppressions, weakened assertions or renewed model runs for hashes.
The originally chained existing-output negatives did not run after failed NN.
Six later pre-load CLI negatives PASS without model/NN: malformed digest, source
extent overflow, short source, short weights, invalid original-feature mode and
existing directory. Each exits1/stdoutempty, leaves the fresh target absent and
existing0B output unchanged. Original observer BEFORE one joined executor proves
6/6scopeclosed/pending0/inflight0/errors[]. The final harness initially mistook
the second attempt's empty directory for an empty file and failed BEFORE these
cases; its setup was corrected to actual state, without weakening product checks.
Final static/control runs reuse the first two passing checks and complete15
remaining cases (including11 Python controls) in14.28s. Strict OpenSpec and all101
local Markdown targets pass; default/dependency lock delta and original all-role
fullhost UNKNOWN checks remain intact. No earlier failed model assertion changed.

Actual Windows x64 Release `bs_learned-inplace.exe` is1195520B, SHA256
`9ea417e6edeaa2599bb1d85417661a5cfb277b7dec5b6314ccba6cb4d0efc95c`;
the first failed native component SHA was
`383a85ade5cd35aadb7ee81a470a5af0b6ced5e3eead3acd3bff3ea7a25d3dbf`.
These are component identities, not complete compiler/producer/mapped-image or
Linux/macOS attestation. C `performance-step27-*` logs and `step27-learned-v1`
artifacts are reconstructible cache only. Tracked code, pins and these exact
receipts/recipes are durable authority. No successful whole-model, CUDA, listening,
distribution, installed-footprint saving or general performance claim follows.

This coherent learned split grows the plan to29. ALL former nativeREST27 is
STEP28 and ENTIRE former FINAL28 is FINAL29: actual complete supported native
CUDA/real same-model device fault/tree+reader end/freshCPU, batch2/wholetrack/
quality/AMP, complete Demucs, every source/frontend/graph/weights/exporter/runtime/
manifest/provider/native-dependency/compiler/binary/full-producer/mapped-image/
OS/architecture role, full CPU/CUDA distribution/CRT/licenses/LGPLsource+3patches/
NVIDIA notices/installer/recovery/footprint and all functions/resources/cancel
remain required. Productive Torch pins/closures/installer/repair remain unchanged;
actual installed saving is zero. All original32 Rust failures/fullhostUNKNOWN/
validator/schema/Pydantic/all32 identity keys/overlap/pressure/quality/device/
listening/combined2-4-6<=10/architecture/docs/distribution gates remain open.

No unchanged failed CUDA attempt or NN merely for hashes/API is authorized by
this CPU work. Fresh ORIGINAL admission AND meaningful source-motivated changed
conditions remain prerequisites. Every actual earlier CUDA failure retains its
distinct UNKNOWN cause and terminal classification. No whole acceptance claim.

## STEP26 impact: independent BS wholetrack orchestration

The independent `stem-dsp::track` core implements the actual vendored
`audio.read_virtual_chunk`, `_read_batch`, `_chunk_window`, `_OverlapAdd` and
`_infer_batches` arithmetic outside Python. It borrows synchronous source,
predictor and private-sink callbacks; it never allocates a complete track or
creates a second product pipeline. No productive route, dependency/pin, installer,
App/PyO3/ORT/provider, source publication or retirement behavior changes.

Release chunk485100, step242550, batch2, four planar stereo sources and float32
arithmetic remain. A strict `source_frames > 485100` creates border242550;
virtual left/right reflection excludes repeated edges. The final chunk reflects
only when its physical virtual length is strictly greater than242551; otherwise
it zero-pads. Tiny tracks, including one frame, reach a predictor as full release
chunks. Batch order and shape are preserved, with a final single chunk as needed.
Independent per-chunk inclusive linear fades have48510 samples; first/last windows
follow their original virtual positions. A727650-frame ring accumulates sequential
predictions and positive window counters, flushes only after a complete batch,
and crops exactly `[border,border+source_frames]` to consecutive private blocks.

Checked geometry precedes callbacks/payload allocation and rejects source byte or
final-position overflow. Large vectors use fallible reservations. Source short
reads, malformed/nonfinite complete predictions, finite accumulation overflow,
uncovered normalization, sink failure and cancellation are terminal. Checkpoints
bracket callbacks and bound source copy/padding/validation/accumulation/normalizing
loops; callbacks must cooperate during their own work. A later failure invalidates
ALL previously emitted private blocks. Actual return is not artifact publication:
the caller retains immutable source/model/output readers, verifies complete outputs
and preserves original resource promises through tree AND reader end.

The unchanged conservative original PCM allowance is183367800 bytes; it excludes
weights, learned and spectral activations, decoder internals and process RSS/Job
commit. The fixed core buffers and one complete prediction batch total86347800
bytes, inside that PCM allowance. Calling the spectral core or a learned predictor
requires the full
additional simultaneous envelope under original batch/overlap/1GiB and host guards.
This module performs no resource admission on behalf of the App.

The original Torch arithmetic oracle uses the unchanged vendored reader/batching/
window/OLA/PCM16 writer with a deterministic arithmetic predictor varying by chunk,
stem, stereo and local frame. It loads no model/weights and initializes no CUDA.
Before measurement, complete float comparison bounds are fixed at
`atol=1e-7,rtol=1e-6`; bit-identical fades/output are not assumed. Cases1/1025/
242550/242551/242552/485100/485101/485102/485103/1455307 cover tiny sources, strict
border/tail thresholds, odd tails, both batch sizes and multiple ring shifts.
All samples and original PCM16 outputs must be checked, with any differing codes
reported honestly. Controls cover source/prediction/sink failures and cancellation.
Compatible STEP25 FFT/complex-mask evidence and earlier NN/cancel/failedCUDA
receipts are reused; another NN or unchanged CUDA attempt is not justified.

This coherent streaming split grows the plan to28: NEXT27 retains EVERY former
nativeREST26 duty and FINAL28 the ENTIRE former FINAL27. Learned BS axial attention,
masks/weights/export/CUDA AMP, complete Demucs spectral-time/normalization/shifts/
overlap/conv/recurrent/attention/resampling, complete native supportedCUDA and real
tight SAME-model fault -> tree AND reader end -> freshCPU/CPUfallback/CPUonly,
all Source/Frontend/Graph/Weights/Exporter/RuntimeBootstrap/Manifest/Provider/
NativeDependencies/Compiler/Binary/FullProducer/MappedImages/OSarch roles,
WindowsEXE AND actual Linux/macOS, complete CPU/CUDA RELEASE/CRT/licenses/LGPLsource+
3patches/NVIDIA notices/installer/recovery/full footprint, quality/functions/
interactions/resources/cancel remain mandatory BEFORE cutover. Installed saving
is ZERO. Original validator/schema/Pydantic/all32keys/allold-newroles/fullhostUNKNOWN,
exact32 parent Rust failures/duplicate216 different async snapshot/distinct216
watchdogUNCOMPARED/registryUNKNOWN/pressureUNKNOWN/original2GiB-3GiB-512MiB,
CPU/CUDAoverlapOPEN and every old/new quality/device/listening/distribution/
combined2-4-6<=10/regression/architecture/docs gate remain unshortened.

### STEP26 verification and reconstruction

The routed build/control harness passes in37.41s: five new track controls and all
five existing spectral controls, strict all-target Clippy, Release `bs_track`,
Rustfmt, Ruff/format and strict reference typing against actual local source
packages. Initial checks correctly rejected import order, untyped installed-package
lookup and an implicit re-export; the import and source-search invocation were
fixed without suppression. No Rust test/precision/guard was weakened. External
Release Native/API setup passes with the existing22 library warnings and einops
pin. No productive dependency or installer change is involved.

Original Torch2.12.0+cu130 CPU arithmetic and the Release native diagnostic compare
all32995136 finite output samples across ten complete cases within the predeclared
`atol=1e-7,rtol=1e-6`, never relaxed. These are arithmetic predictions, not learned
model separation. Native floats are independently converted using original
float32 clamp/multiply32767/ties-even/negative-fullscale policy and compared with
every code from the original vendored PCM16 writer. They are NOT bit identical:

| Source frames | Float max absolute error | Changed PCM16 codes, drums/bass/other/vocals |
| ---: | ---: | --- |
| 1 | 0 | 0/0/0/0 |
| 1025 | 0 | 0/0/0/0 |
| 242550 | 0 | 0/0/0/0 |
| 242551 | 0 | 0/0/0/0 |
| 242552 | 0 | 0/0/0/0 |
| 485100 | 5.960464477539063e-8 | 4/2/2/6 |
| 485101 | 5.960464477539063e-8 | 4/5/14/16 |
| 485102 | 5.960464477539063e-8 | 4/5/14/16 |
| 485103 | 5.960464477539063e-8 | 4/5/14/16 |
| 1455307 | 5.960464477539063e-8 | 15/15/40/49 |

All250 differing codes differ by exactly one. The longest case has9 full padded
chunks in batches2/2/2/2/1,11642456 float samples, RMS2.312132549817128e-9,
source SHA256 `dd73cbf153b913b859d8a81514cc7cf1a32a68b32dedb6da08eb978c7b877476`,
native output `08893a5cf40d129645eed06fc5830a9eca8804012490bcb68c6fb0bd9e087dd5`,
original output `7e0cf25d2ac5c75845c61694ffffc910fb3f6e555e18d5b5fd3c5fc315018754`.
Neither these small differences nor synthetic successful comparisons certify
whole-model/audio/listening quality. No performance comparison is claimed.

Five CLI negatives reject overflow/zero geometry, truncated source, nonfinite
source and an existing output with exit1 and empty stdout. The first three
create no output; nonfinite input leaves only an empty exclusive diagnostic file;
the protected existing file is byte unchanged. Diagnostic sink files may remain
partial on later failures and are NEVER product-ready artifacts. The core's
short-read/incomplete/nonfinite/overflow prediction, late sink failure and
cancellation cases all return terminal errors, including normalization and after
a real sink callback. Caller private-attempt invalidation remains mandatory.
Original observer-before-one-joined-executor proves16/16 scope closes,
pending/inflight0/errors[] across reference+ten positives+five negatives. The
numeric harness passes in16.08s; supervisor `worker_crashed` on explicit negatives
is not device failure/fallback. No CUDA or learned model is executed.

Reconstruct via outer development entry with the actual native checkout: locked
`cargo test -p flitzis-stem-dsp -- --test-threads=1`, all-target strict Clippy and
Release `--example bs_track`. Under the existing admitted owned command scope,
run `scripts/bs_track_trial_reference.py NEW_CACHE_DIRECTORY`, then
`bs_track CASE.pcm.f32 SOURCE_FRAMES NEW_PRIVATE_OUTPUT.f32` for every recorded
case. Sources are interleaved stereo float32; diagnostic outputs are frame-major
`[physical_frame,stem,stereo]`. Compare every float with the declared bounds and
every PCM16 code as above. The tracked reference defines deterministic source and
ordinal/stem/channel/local arithmetic. Observe before creating the single joined
executor; retain the original resource guard and actual tree/reader close.
Generated `performance-step26-*` logs and `step26-track-v1` fixtures/binary are
reconstructible cache only, not authoritative sources.

Final five static/control checks pass in18.78s: Rustfmt/Ruff/strict typing,
strict OpenSpec/local Markdown targets/whitespace, unchanged productive sources/
locks/pins and isolated dependency trees, actual current receipts, compatible
STEP25 spectral/older CPU-numerical/CPUcancel/failedCUDA settlements and both
original complete-host manifests still UNKNOWN with ALL roles retained. Reused
observations are not new model executions or a new overall green matrix.
Actual Windows11/build10.0.26200/AMD64/64bit uses Rust1.95.0/LLVM22.1.2/Cargo1.95.0.
The Release diagnostic component is169984 bytes/SHA256
`7165c495fa5c031c8ea5fac6ab87c64a7a3a306d341012aa5a9b2ab38657b6bb`.
This file/toolchain metadata is NOT full compiler/binary/producer/mapped-image,
complete bundle or Linux/macOS evidence. Systems PRE/frozenPOST and Methods PRE
are scoped CLEAR. Final index/docs checks pass2/2 in5.46s: all12 LF-only staged
files equal native D source,125 local Markdown targets are valid, and exact five
local BAT bridges/private pointer/CommonExclusion/hidden flags remain protected.
The single independent final actual-diff review is scoped publication CLEAR with
no P1/P2; all review readers ended. This does not establish whole-path acceptance.

## STEP25 impact: independent BS spectral frontend and reconstruction

The independent `flitzis-stem-dsp` Rust crate implements the missing complex
STFT/features/masking/ISTFT portion of the pinned BS-RoFormer path. It uses the
release configuration in `vendor/bs_roformer/src/flitzis_bs_roformer/model.py`
and forward path in `network/bs_roformer.py`, not its different hop512 default:
44.1kHz stereo, periodic Hann2048, FFT2048, hop441, centered reflect padding,
one-sided unnormalized spectrum, frequency-leading stereo/complex features,
four complex masks, inverse window-square overlap normalization and explicit
original PCM length. No file, Python, tensor runtime, engine or device dependency
enters the core; its only dependency is the already locked `rustfft`.

This trial admits one complete chunk of1025..485100 frames before allocation;
shorter inputs cannot satisfy the original reflect1024 requirement. Features
are `[time,frequency,stereo,real/imag]`, masks add the leading four-source axis,
and output is planar `[stem,stereo,original_frame]`. The original release batch2,
overlap2 and1GiB ceiling are not changed. A caller must separately promise the
sum of all simultaneous chunks/batches and retain source/mask/reader custody
through actual return. Geometry reports the simultaneous caller PCM, features,
all four masks, four-source output, inverse accumulation/envelope, bounded FFT
scratch and a conservative1MiB fixed-plan allowance. This is a payload envelope,
not total process commitment or a replacement for original resource admission.
Fixed-size FFT planning has ordinary allocator behavior; large payload vectors
use fallible reservation. No failure/cancel returns partial output.

Original Torch arithmetic, not model inference, will verify asymmetric stereo,
reflected edges, odd/non-hop tails and complex masks. Tolerances fixed before
comparison are spectrum `atol=2e-4,rtol=2e-5` and PCM `atol=2e-5,rtol=2e-5` for
the bounded reference signals/masks. Different FFT/vector-cosine implementations
may differ in final bits; neither bit equality nor whole-model/audio acceptance
is assumed. Both imaginary signs, DC/Nyquist real-inverse behavior, source order,
all original tail samples, finite/shape/overflow and cancellation need checks.

This coherent spectral split grows the plan to27: STEP26 retains EVERY former
nativeSTEP25 duty and FINAL27 retains the ENTIRE former FINAL26. The learned BS
network/weights/export/CUDA AMP, complete track reflection/chunks/rolling overlap,
Demucs complete hybrid path, successful nativeCUDA and actual same-model narrow
device failure -> complete tree AND reader end -> freshCPU, all source/producer/
image/OSarch roles, quality/functions/resources/cancel, CPU-only/CUDA release
bundles/CRT/licenses/LGPLsource+3patches/NVIDIA notices/installer/recovery/full
footprint and WindowsEXE AND Linux/macOS evidence remain mandatory before cutover.
All former final original-validator/schema/Pydantic, exact32Rust-parent failures,
distinct watchdog/registry/resource-pressure UNKNOWN, CPU/CUDA overlap and complete
quality/device/listening/2-4-6/<=10/architecture/docs/distribution gates remain.
Productive paths, dependency closures, pins and installer remain; installed
saving is ZERO. The crate is not linked into the App or its normal packages.

### STEP25 verification and reconstruction evidence

Initial routed checks pass: five native unit controls, strict Clippy for every
new crate target, Release standalone build, Rustfmt and Python Ruff/format. The
external Release Native/API setup passes with the existing22 library warnings;
the productive einops pin remains. The new crate's normal dependency tree has
only existing `rustfft6.4.1` and its numerical dependencies, no PyO3/ORT/Torch/
LibTorch. Workspace/lock changes add only this independent package; they do not
change the App's dependency graph or its Portable installer/runtime closure.

The original CPU Torch2.12.0+cu130 arithmetic oracle and Release CLI pass all five
complete cases below under fresh original admission, without model inference.
Masks vary with stem/time/frequency/channel and include imaginary DC/Nyquist
terms. PCM has asymmetric tones, right-channel Nyquist content and both edge
impulses. Every feature and all four stereo outputs, including every tail sample,
are finite and within the predeclared tolerances; they are not generally bit equal.

| Case | PCM frames | STFT frames | Spectrum max absolute error | PCM max absolute error |
| --- | ---: | ---: | ---: | ---: |
| Minimum reflection | 1025 | 3 | 7.62939453125e-6 | 4.470348358154297e-8 |
| Silence / hop boundary | 1323 | 4 | 0 | 0 |
| FFT boundary | 2048 | 5 | 7.62939453125e-6 | 5.960464477539063e-8 |
| Odd tail | 4099 | 10 | 1.52587890625e-5 | 4.470348358154297e-8 |
| Release chunk | 485100 | 1101 | 1.52587890625e-5 | 7.450580596923828e-8 |

Release-chunk spectrum RMS is5.028630278359165e-7; PCM RMS is
7.420437580691347e-9. Its complete native output has3880800 samples and SHA256
`51cf8a8e454045ddbf858a3f042af84ca6d1206328ffad512fb6a930b62c9464`;
the original output SHA is
`53dc63901e9fcb977cad48a6633e67ec6db78f00e00fdd5c99edee863b1543d8`.
The reported single-chunk payload envelope is114721872 bytes, not total process
memory, a Job receipt or the complete release batch/track promise.

Seven actual CLI negatives reject oversized geometry, truncated PCM/masks,
nonfinite PCM/masks, overflowing finite masks and existing output; stdout stays
empty, new success files remain absent and the existing output hash is unchanged.
The original supervisor labels these explicit exit1 failures `worker_crashed`;
they are terminal diagnostic rejections, not device failures/fallback receipts.
One joined executor was observed from before the first scope through all13/13
actual original scope closes; pending/inflight0 and sampling errors empty. Native
unit controls additionally prove identity/zero/distinct-source scaling, reflected
odd tails, rate/extent limits, finite failures and cancellation before/during
forward and inverse work. No partial core result can escape an error.

Reconstruct through the normal routed development entry: run the new crate's
locked tests and all-target Clippy, then build Release `--example bs_spectral`.
Under the existing admitted offline command owner, run
`scripts/bs_spectral_trial_reference.py NEW_CACHE_DIRECTORY` with the unchanged
main Python/Torch environment (CPU pools1), then for each recorded case run
`bs_spectral PCM_F32 MASK_F32 FRAMES NEW_FEATURES_F32 NEW_PCM_F32`. Compare every
float with the script's declared tolerances and assert exact lengths/finiteness.
The reference explicitly creates CPU tensors and never loads a model. The CLI
keeps exclusive diagnostic outputs; it supplies no application provenance,
atomic product-generation transaction or built-in resource admission/cancellation
interface. Its caller retains the original tree/reader/resource owner. The core
itself checks its cancellation callback at bounded validation/FFT/overlap/output
blocks and final return. The fixed CPU spectral arithmetic is no nativeCUDA,
learned-network, CUDA-AMP, full-quality or whole-source migration acceptance.

Generated `performance-step25-*` logs and `step25-spectral-v1` fixtures/binary stay
reconstructible cache, not source authority. All previous CPU/failedCUDA and
fullhost-UNKNOWN evidence remains separate and source-compatible, with no new NN.

Final six routed static checks pass in42.40s: all five strengthened native controls,
all-target warning-as-error Clippy, Rustfmt, Ruff/format and strict isolated oracle
typing, strict OpenSpec/local Markdown targets/whitespace, exact unchanged
productive sources/locks plus independent dependency tree, original numerical/
CPU-cancel/failedCUDA scope evidence reuse, and both original full-host validators
still UNKNOWN with all roles retained. Initial build/control harness passes22.20s;
the five numerical/seven terminal controls pass in11.14s. These are separate
scoped checks, not a whole-product matrix or latency benchmark.

Actual component platform: Windows11/10.0.26200 AMD64,64bit, Rust1.95.0
(`59807616e1fa2540724bfbac14d7976d7e4a3860`), LLVM22.1.2 and Cargo1.95.0.
Release `bs_spectral.exe` has1051136 bytes/SHA256
`0e5948b9554010ac13043954d09936fef0bf437a3577d78aa245ba33f72d3b33`.
This is an actual Windows component file, not complete Compiler/Binary/FullProducer/
MappedImages attestation or Linux/macOS execution. Systems PRE/frozen POST,
Methods and exactly one independent final actual-diff chain are scoped-publication
CLEAR with no P1/P2 findings; all review readers end before branch release.

## STEP24 impact: opt-in standalone Windows loader and observed images

The new explicit `FLITZIS_TRIAL_ORT_LOAD_POLICY=package-system32` mode requires
the existing verified runtime and declared-package manifests. It is supported
only by dynamic Windows standalone CLIs. Legacy custody-only/runtime requests
retain their loading behavior; static/frontend requests reject loader settings.
All manifest/source/graph/output gates still precede ORT. The audio frontend can
already have loaded libsoxr, which this ORT boundary does not attest.

After complete file admission and the fresh-process latch, enumerate at most4096
current modules, acquiring real references by address. Reject pre-existing
declared basenames or images within the package directory, including matching
bytes at an alternate file identity. Reset the previous `SetDllDirectory` path,
check `SetDefaultDllDirectories(SYSTEM32|USER_DIRS)`, and retain one package
`AddDllDirectory` cookie. This is an irreversible policy of this disposable child,
never application bootstrap or a reversible App setting. Unknown pre-existing
AddDllDirectory cookies are not enumerable; fresh controlled CLI source is the
boundary, not a claim to clear arbitrary injected loader state.

Load the absolute runtime with `LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|SYSTEM32`.
These documented per-call flags constrain this load's directory search; later
explicit loader overrides, already-loaded modules, API sets, SxS and KnownDLLs
remain distinct. No provider/CUDA DLL is eagerly loaded. Verify the returned
module and the actual `OrtGetApiBase` symbol owner, using full untruncated UTF-16
module paths, reopened deny-Write/Delete files, complete SHA and the admitted
full128-bit FileIdInfo+volume. Compare the wrapper's actual API pointer with the
exact API24 pointer from that library, so its cached runtime cannot silently differ.

Snapshots after loading/bootstrap and before success output verify every observed
declared image against its original file pin. An undeclared image inside the
package directory, foreign backing file, query failure or bound/truncation failure
is terminal; no typed CUDA70 or fallback is inferred. Metadata/logits are written
only after the final image check. References and cloned pins of ALL admitted files
are retained until actual standalone process exit once ORT can cache a library;
its global library/environment have no ordinary Rust Drop. Before that point,
module references end before file pins. Session/profile/cancellation/output readers
retain their existing order. Receipts identify observed declared backing files,
not relocated memory contents or a complete dependency closure/full producer.

Source basis: exact libloading0.9.0 Windows `Library::new` delegates to flags0;
ort2.0.0-rc.12 `load_dylib_from_path` caches its first library and `init_from`
discards its effective-load boolean. The API-pointer and environment-commit checks
therefore both matter. ORT1.24.4 obtains the provider directory from its actual
runtime image and later loads providers with `LOAD_WITH_ALTERED_SEARCH_PATH`:
[Windows environment](https://github.com/microsoft/onnxruntime/blob/v1.24.4/onnxruntime/core/platform/windows/env.cc),
[provider bridge](https://github.com/microsoft/onnxruntime/blob/v1.24.4/onnxruntime/core/session/provider_bridge_ort.cc).
The documented search/reset/reference/path semantics are
[LoadLibraryExW](https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-loadlibraryexw),
[SetDefaultDllDirectories](https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-setdefaultdlldirectories),
[AddDllDirectory](https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-adddlldirectory),
[SetDllDirectoryW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setdlldirectoryw),
[GetModuleHandleExW](https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-getmodulehandleexw),
[GetModuleFileNameW](https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-getmodulefilenamew).
Package-directory rejection uses checked Windows ordinal case comparison on
canonical path ancestors, not a case-sensitive textual prefix:
[CompareStringOrdinal](https://learn.microsoft.com/en-us/windows/win32/api/stringapiset/nf-stringapiset-comparestringordinal).
Module enumeration is a bounded observation, with disappearing/query-failed
modules terminal; it cannot certify absence of every transient or data-file mapping:
[EnumProcessModules](https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-enumprocessmodules).
Imported DLL initializers execute before a post-load mismatch can be rejected;
this diagnostic boundary is not a native-code sandbox.

Verification targets real fixture imports, late basename and exact ORT-style
altered-path loads with poisoned CWD/PATH/previous SetDllDirectory, missing
package dependencies, preloaded foreign images, backing-file identity and pins.
Runtime/API-only controls use existing authenticated CPU files and the original
owned runner/observer, without a model session or new NN. Reuse compatible original
CPU numerics/cancellation and all distinct failed CUDA settlements. No unchanged
CUDA attempt, precision/memory-pattern/resource-floor change or installer change.

The plan grows to26 for this independently reviewable loader boundary. STEP25
retains the ENTIRE former nativeSTEP24: full successful nativeCUDA, real tight
SameModel device fault -> complete tree AND reader end -> freshCPU, CPU-only/
fallback, complete source/frontend/chunk/postprocess/model/audio quality/functions,
ALL Source/Frontend/Graph/Weights/Exporter/RuntimeBootstrap/Manifest/Provider/
NativeDependencies/Compiler/Binary/FullProducer/MappedImages/OSarch roles, portable
WindowsEXE AND actual Linux/macOS evidence, independent Rust cores/thinPyO3,
complete CPU/CUDA release/CRT/licenses/LGPL source+three patches/NVIDIA notices/
installer/recovery/full footprint and practical full Demucs/BS parity before cutover.
Mask-network export alone is insufficient; productive closures remain. Installed
saving is zero. STEP26 retains the ENTIRE former FINAL25: original StemExecution
validator/schema/Pydantic dependency and all32keys/allold-newroles/fullhost UNKNOWN,
exact32 parent Rust failures/duplicate216 different async snapshot/distinct216
watchdog UNCOMPARED/registry UNKNOWN, unexplained Human pressure/original2GiB-
3GiB-512MiB guards/CPU-CUDA overlap OPEN and all quality/device/listening/combined
2/4/6<=10 regression/architecture/docs/distribution gates. No omitted roles or
blanket decorator clearance; all four CUDA failures remain distinct/cause UNKNOWN.

### STEP24 verification and reconstruction evidence

All three locked features build both standalone CLIs and each passes the ten
existing audio-example Rust controls. The dynamic/CUDA test binaries additionally
contain one explicitly ignored fixture test, executed separately below. Initial
two build checks pass48.17s; a test-only function-pointer cast warning is corrected
without suppression. Final feature/control builds also pass after the actual
review correction. These overlapping runs are not an additive overall matrix.
External Release Native/API retains22 known warnings and the unchanged einops pin.

Nine real Windows DLL fixture invocations pass: initial imported dependency,
late bare-name load, late absolute `LOAD_WITH_ALTERED_SEARCH_PATH`, missing import
at each of those three boundaries despite poison availability, preloaded foreign
same-basename image, undeclared initial package import, and undeclared late image
loaded through an uppercase package path. Every missing/undeclared case first has
a valid manifest for the files actually declared/present, so it exercises real
loading rather than failing file admission. Matching root bytes copied to a
different file identity fail the identity check; a foreign export owner fails;
live module/file pins deny Write/Delete, with write access restored after complete
non-ORT fixture guard retirement. Both later-load forms return package values,
not poison values. This tests uppercase path spelling, not every filesystem/alias.

Reconstruct four fixture DLLs in isolated cache directories with MSVC
`cl /nologo /LD /O2`, explicit object/DLL/import-library outputs and linked import
libraries. `dependency.dll` exports `int FixtureDependency(void){return 42;}`;
`root.dll` imports it and exports `FixtureRoot()` returning that value.
`latedep.dll` exports `FixtureLateDependency()` returning73; `late.dll` imports it
and exports `FixtureLate()` returning it. All exports/imports use the matching
`__declspec(dllexport/dllimport)`. The poison directory contains byte-identical
copies of root/late but separately compiled dependency values13/17. Keep poison
in actual CWD/PATH and set its path with SetDllDirectory before the tested policy.
Each case copies only its intended files and creates valid existing schema1
runtime/package manifests with actual hashes. Omit missing imports and deliberately
undeclared existing imports from the component list. Run the dynamic test binary
in a separate original owned child with
`runtime::loader::tests::actual_windows_loader_policy_and_image_identity --exact
--ignored --test-threads=1 --nocapture` and `FLITZIS_LOADER_FIXTURE`,
`FLITZIS_LOADER_POISON`, `FLITZIS_LOADER_CASE` selecting the directories/case.
These are test-only inputs, not productive loader controls.

Nine original owned runtime invocations also pass their expected outcomes: two
controlled CPU-runtime/API-only positives (dynamic and CUDA-enabled binaries,
both using the authenticated CPU1.24.4 package), legacy runtime-only and legacy
declared-custody positives, plus invalid/empty policy, missing package, static
and frontend early rejections. Both controlled positives observe exactly the
admitted `onnxruntime.dll` with full128-bit FileId+volume and matching size/SHA;
the shared provider is declared but not observed. No CUDA provider is eagerly
loaded, no model session runs and no inference/quality/provider-activation claim
follows. The five negatives have empty stdout and no ORT phase. Poison ambient
`ORT_DYLIB_PATH`, CWD and PATH do not become authority.

The original observer precedes each single joined executor; both groups finish
9/9 scopes closed, pending/inflight0/errors[]. Fixture preparation and these two
control harnesses pass3 checks in11.80s. Earlier setup failures are retained:
cmd-list quoting rejected the vcvars path, then bare `cl.exe` could not be resolved
from a passed child environment. An owned cache BAT wrapper, case-insensitive
environment-key merging and absolute compiler path correct those harness issues.
The first failed setup also caused two dependent missing-file/invalid-CWD test
failures before actual loader execution; none are inference evidence.

Final six static checks pass11.12s: actual Windows11/build10.0.26200/AMD64/64-bit,
rustc1.95.0/LLVM22.1.2/Cargo1.95.0 and six complete binary hashes; six Rustfmt
targets/strict OpenSpec/88 Markdown targets/whitespace; productive sources/locks/
installer unchanged; four locked normal feature trees without PyO3/Torch/LibTorch
and dynamic CPU without CUDA;18 settled scopes; compatible original CPU numerics,
actual CPU cancellation and failed CUDA settlements reused without NN; both
original fullhost probes remain UNKNOWN at the actual StemExecution validator
with all productive roles retained. Linux/macOS and other Windows architectures
are not executed. Actual CUDA model execution and freshCPU fault fallback remain open.

Measured Windows x86_64 Debug component files (not release/full-producer evidence):

| Feature | Audio CLI bytes / SHA256 | One-chunk CLI bytes / SHA256 |
| --- | --- | --- |
| static CPU | 36563968 / `955a18745abbc90815db0e430e614d0fba4515d9bbc672ec90487ddf7ac8c5a8` | 24502784 / `f351178d8b3f301efe8da6fa5124e4220a623f5be3dd37d43e39c3fc0f5d0579` |
| dynamic CPU | 13615616 / `3b3dd5dca4e1fe9737a96f9e88f2086a6323f28a5b37d374309462015390fdf0` | 1649152 / `c0b9c22707c40195025f6ac171f8b66ef4e03809fbbdbb4c357cd9696ed67276` |
| CUDA | 13642240 / `ff6c2db188e10756e32c6ded8ea57ca06ac4cf7a871de2337d3557825d5bea0d` | 1674240 / `23406cbdcb5de9608e58b75a9f9d3480bf6f35fb3f09ae5a3e61eb6d2465f4fd` |

Systems PRE/frozen POST/targeted recheck and Methods are CLEAR. The same one
independent final-diff chain finds and resolves a real P2: legacy file Admission
must remain owned outside the inference block until ALL resampled/spectrogram/
metadata/stdout output finishes. Controlled global retention alone does not protect
legacy mode. The outer `_admitted` owner restores that lifetime while the fallible
snapshot still precedes logits/success writes. No assertion or resource/quality
guard is weakened. The same independent chain is publication CLEAR with no
remaining P1/P2. Final index/docs checks pass2 checks in2.32s: all11 LF-only staged
files equal the authoritative source; strict OpenSpec,88 local Markdown targets,
hidden flags and the private five bridges/pointer/common exclusion remain valid.
All review readers end. These are scoped results, not full native acceptance.
Generated `performance-step24-*` logs, fixture DLLs and probes stay reconstructible
cache output, not durable authority. Every native25/final26 requirement above remains.

## STEP23 impact: declared ORT package file custody

The optional standalone package boundary now checks every declared ORT/provider/
CUDA/cuDNN component before the first ORT API or logger. It retains each verified
SAME-file handle inside the existing runtime Admission through all model, profile
and output readers. The original4096-byte runtime schema1 and512MiB main-library
limit remain unchanged. Productive bootstrap, dependencies, loader behavior,
frontend/model arithmetic, quality, memory patterns and resource guards do not change.
The audio CLI can load libsoxr before this ORT boundary; this is not admission
before every native load. No extra model inference is justified by file custody.

Set both `FLITZIS_TRIAL_ORT_PACKAGE` (absolute path) and
`FLITZIS_TRIAL_ORT_PACKAGE_SHA256` (exact lowercase digest) alongside the existing
runtime request. The initialization-only `runtime MANIFEST SHA256` command accepts
this pair too. Partial pairs fail; static builds reject the request and frontend
commands reject package configuration without initializing ORT. Absent package
configuration preserves the original runtime-only admission, with no package claim.

The separate UTF-8 JSON manifest is at most16384 bytes and has exactly
`schema_version:1`, `os`, `arch`, and `components`. Each component has exactly
`name`, `role`, `bytes`, and `sha256`. Roles are `ort-runtime`,
`ort-provider-shared`, `ort-provider-cuda`, `cuda` and `cudnn`. The list contains
1..64 files, each1..2GiB, with checked sum at most8GiB; these are admission bounds,
not allocations or measured footprint. This accommodates the existing24-file
CUDA scratch without weakening the original main-runtime limit. Exactly one
`ort-runtime` entry must match the admitted runtime name/bytes/SHA and file identity.
Only portable ASCII basenames up to128 bytes are admitted; case aliases, separators,
ADS, trailing dots/spaces, duplicate/unknown/missing fields, unknown roles, invalid
sizes/digests, foreign OS/architecture and duplicate opened file identities fail
closed. Both manifests and all canonical component parents must be the same
owned package directory. Windows uses the full128-bit file ID and volume from
the actual open handle via [GetFileInformationByHandleEx/FileIdInfo](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getfileinformationbyhandleex)
and the official [FILE_ID_INFO layout](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info),
without a weaker64-bit fallback; Unix uses device/inode. Other platforms fail closed for this boundary.
Windows pins deny Write/Delete; Unix still requires caller-owned immutable custody.

For the saved CPU runtime, construct the component list from STEP21's two exact
RECORD-verified DLL entries, with roles `ort-runtime`/`ort-provider-shared`, and
hash the exact serialized manifest. For the CUDA scratch, use STEP19/20's24
authenticated entries: three ORT roles, NVIDIA CUDA files as `cuda`, and cuDNN
files as `cudnn`. Hash from the actual complete files; never infer identity from
basenames. The receipt enumerates declared paths/roles/sizes/SHA/count/total and
explicitly keeps loader closure, mapped images and full producer **UNKNOWN**.
An incomplete list remains an incomplete declared list; no runtime/provider/CRT/
driver discovery, complete dependency or redistribution claim follows. Package
file custody does not change how the OS resolves DLLs, or authorize productive
Torch removal. Installer saving remains zero; licenses/LGPL source+three patches/
NVIDIA notices, CRT, release/recovery and full footprint remain required.

The plan grows to25 steps for this independently verifiable package admission.
STEP24 retains ALL former nativeSTEP23 requirements: full successful nativeCUDA,
real tight SameModel device failure -> complete tree AND reader end -> freshCPU,
CPU-only/fallback, full source/model/audio quality and functions, every source/
frontend/exporter/graph/weights/bootstrap/manifest/provider/native-dependency/
compiler/binary/mapped-image/OS/architecture/full-producer role, WindowsEXE AND
Linux/macOS, complete CPU/CUDA release/CRT/legal/installer/recovery/footprint,
and practical complete Demucs/BS parity before cutover. STEP25 retains ALL former
FINAL24 requirements, including original StemExecution validator/schema/Pydantic
and all roles/fullhost UNKNOWN, exact32 Rust parent failures/duplicate216 different
async snapshot/distinct216 watchdog UNCOMPARED/registry UNKNOWN, unexplained
Human pressure/original2GiB-3GiB-512MiB guards/CPU-CUDA overlap OPEN and all quality,
device/listening/combined-regression/architecture/documentation/distribution gates.
All four earlier CUDA failures stay distinct and cause UNKNOWN. This package-file
change alone does not justify repeating the same failed CUDA inference conditions.

### STEP23 verification and reconstruction evidence

Three locked feature builds pass for both CLIs, and each feature passes ten Rust
audio-example controls: the seven prior controls plus strict component parsing,
target/count/total and actual package-custody controls. The latter checks all file
pins, post-drop access, changed size/hash/bytes, missing files, directory containment,
oversize manifest and a real hardlink alias. The two build/format harness checks
pass62.87s. External Release Native/API retains22 known warnings and the unchanged
einops pin. Initial setup used a wrong collection path, collecting zero tests;
its failed log is retained and the subsequent fault tests use the actual path.

The actual CPU declared package (two DLLs/16141888 logical bytes) and CUDA declared
package (24 DLLs/3492501392 logical bytes) both pass file verification and runtime
version/API-only initialization. Existing independently authenticated wheel/RECORD
receipts are reused, and every current complete component hash is rechecked.
No model session, reference export, CPU NN or CUDA inference is run. The CUDA
probe attests no provider kernels or device success. A runtime-only legacy probe
also passes with null declared-package evidence, including poisoned ambient
`ORT_DYLIB_PATH`. Thirty early-negative requests fail with no ORT phase or success
output, including static/frontend requests and either missing package environment
half. Original observer starts before the single joined executor; all33 scopes
close with pending/inflight0/errors[]. Preparation, these controls and sixteen
unchanged fault-protocol tests pass18 harness checks in91.45s. This is a scoped
file/API result, not a speedup or full acceptance matrix.

Exact package manifests use Python `json.dumps(value, indent=2).encode()` without
a trailing newline, root-key order `schema_version,os,arch,components`, component
key order `name,role,bytes,sha256`, and the saved component order: CPU runtime then
shared; CUDA entries in case-insensitive basename order. Reconstruct file entries
from STEP21's CPU wheel and STEP19/20's exact CUDA/ORT wheels and RECORD recipes;
do not reconstruct hashes from version labels. Measured package-manifest SHA256:
CPU `7a5e216d9cc35418e8c38afa293ece04e86abc63153553fe56e81aa901cf2319`,
CUDA `d62a363030d5c75db02f357eb2fd9936fc9edc26160f84b83ef19be231103f4b`.
Full compiler/binary/producer and loaded-image closure remain UNKNOWN.

Measured Windows x86_64 Debug component binaries:

| Feature | Audio CLI bytes / SHA256 | One-chunk CLI bytes / SHA256 |
| --- | --- | --- |
| static CPU | 36562432 / `9cda16a2ae95816fd5918535e13c38abf52866199e7800f2134c703a9e8917e5` | 24501760 / `ef2871c18a30c08229230996d7ad8066a1a42f5ca4684f9524f0d0a453a8340c` |
| dynamic CPU | 13488128 / `8ff234d97288f6fb3ed001efa69a71705b14c83881903578c993410064f47b0e` | 1508864 / `796a5a71731f2602e5b7c75e71760d39cc9ba0b033fa1e8f15fa203c7b8da731` |
| CUDA | 13514240 / `8986915c48e33075852927c0caad34a77ddcca7bbb59021b7595cf7f81ec9a56` | 1532416 / `0bb0ad9764f74327eb20dbf00fd47f7d701dbfebc5df8169e0a0971b273ff415` |

Reconstruct with the configured contributor output route and `--locked`, build
both examples for all three existing features and run the audio-example controls.
API-only probes use `runtime MANIFEST SHA256` plus the package environment pair.
Unchanged frontend, graph, model, provider, abort and fault implementations retain
their original bounded STEP19–22 evidence; no NN rerun is justified by new hashes.
Generated logs `performance-step23-{setup,build,controls}.log` and C probes retain
diagnostics but are not durable authority. Every native and final gate above
remains mandatory; productive locks, installer/repair and installed saving stay
unchanged, with saving zero.

Final five static checks pass11.12s: five Rustfmt targets, strict OpenSpec,
89 local Markdown targets/whitespace, unchanged productive sources/locks/installer,
four locked normal feature trees without PyO3/Torch/LibTorch and dynamic CPU
without CUDA. Saved original CPU numerics, actual CPU termination and failed CUDA
settlements are checked without new inference; both original fullhost probes
remain UNKNOWN at the actual StemExecution validator with all productive roles.
Systems PRE/frozen POST, Methods and one independent final actual-diff review
chain are CLEAR. All review readers end; these results preserve the still-open
whole-quality, nativeCUDA/freshCPU, producer, platform and distribution gates.

## STEP22 impact: end unused standalone host-buffer lifetimes

Both standalone CLIs now drop the serialized graph immediately after successful
`BeatModel::load`/`load_on`, before prediction. The complete-source CLI saves the
source frame count and the identical binary64 duration expression before resampling,
then drops the original PCM only after the synchronous full HQ resample returns.
The complete prepared waveform and spectrogram remain owned through exclusive
success output. Source hashing, full tail/geometry bounds, true chunk lengths,
postprocessing, cancellation, provider options and runtime file pins are unchanged.

This uses the exact locked ort2.0.0-rc.12 copying `commit_from_memory` route, which
returns an owned Session. Its distinct `commit_from_memory_directly<'m>` sets both
zero-copy options and returns `InMemorySession<'m>`; that route is not used.
The [wrapper source](https://github.com/pykeio/ort/blob/v2.0.0-rc.12/src/session/builder/impl_commit.rs)
calls `CreateSessionFromArray`. In ORT1.24.4/API24 the
[C API](https://github.com/microsoft/onnxruntime/blob/v1.24.4/onnxruntime/core/session/onnxruntime_c_api.cc#L730)
finishes loading and initialization before returning. The
[session implementation](https://github.com/microsoft/onnxruntime/blob/v1.24.4/onnxruntime/core/session/inference_session.cc#L1003)
parses ONNX bytes into an owned ModelProto and moves it into the Model; its separate
ORT-format path copies by default. No direct-byte/initializer option is enabled.
The wrapper's finalization retains session resources, not a caller-graph borrow.
The static rc.12 download recipe selects1.24.2, whose
[session path](https://github.com/microsoft/onnxruntime/blob/v1.24.2/onnxruntime/core/session/inference_session.cc#L980)
and [C API](https://github.com/microsoft/onnxruntime/blob/v1.24.2/onnxruntime/core/session/onnxruntime_c_api.cc#L730)
have the same ownership semantics. That is recipe/source evidence, not an exact
loaded-static-runtime or full binary-producer attestation. The measured dynamic
manifests remain bound to1.24.4/API24; no runtime version or dependency changes.
`Soxr::resample` owns even its same-rate copy and deletes its native handle before
returning. These source/API facts support the earlier last-use boundaries; they
do not attest the complete runtime producer or mapped dependency closure.

The82,104,250-byte measured graph and original PCM no longer remain live during
prediction. Rust deallocation does not establish return to the OS, a lower process
peak or a CUDA fix. No precision, memory-pattern option, allocator policy or original
admission floor changes. Existing CPU numerical/cancellation evidence is reused;
no CPU NN is run merely for a new binary hash or buffer lifetime.

The plan grows to24 steps to publish this coherent lifetime change independently.
STEP23 retains the ENTIRE former STEP22 native CUDA/FreshCPU/CPU-only/full-quality,
all producer roles, Windows/Linux/macOS, distribution/recovery/footprint and full
Demucs/BS parity requirements before cutover. STEP24 retains the ENTIRE former
FINAL23 combined regression, original validator/schema/Pydantic and all roles,
32 parent failures/distinct216 watchdog/registry UNKNOWN, pressure-cause/overlap,
quality/architecture/docs/distribution gates. Productive Torch closures remain.

### STEP22 verification and unsuccessful changed CUDA attempt

Three locked feature builds (`native-inference-trial`, `native-inference-dynamic`,
`native-inference-cuda`) each build both standalone CLIs and pass the existing
seven audio-example Rust controls. The original twelve selected core tests pass.
These two trial checks pass72.07s, after separate writer environment/Native setup;
external Release Native/API retains22 known warnings and the analysis lib tests
retain two known CQT warnings. No suppression or assertion weakening is added.

Eight complete frontend cases, including the same-rate copies, full1601-frame
track and384001/768000-Hz cases, produce resampled and spectrogram bytes identical
to the saved STEP20 results. Their original numerical/reference limits remain;
no model/reference inference is repeated. The original observer precedes the
single joined executor:8/8 frontend scopes close, pending/inflight0/errors[].
Ten original early-negative commands also pass with10/10 scopes closed and no
pending/inflight/sampling errors. Existing output, byte/prediction/rate/finite
shape, graph/libsoxr digest, explicit CPU-only and diagnostic guards remain.
Together with16 unchanged strict fault-protocol tests, three format/OpenSpec/
dependency/saved-evidence checks and one fresh original-headroom query,22 checks
pass11.88s. Four locked normal dependency trees remain free of PyO3/Torch/LibTorch;
dynamic CPU does not acquire CUDA. All productive pins/installers/worker closures
and frontend/provider/runtime/fault implementations are unchanged.

Exactly one source-changed CUDA attempt follows a fresh original preflight:
available commit4705738752 bytes, prospective child limit1279127552, above the
previous observed initialization peak1254162432. Actual launch available commit
is4708270080; original actual Job budget1280395264. The current CUDA executable
returns3221226505 (`0xC0000409`) after17.2323321s with empty stdout and a generic
`fatal: Memory allocation failure`. Runtime file/API/environment phases and a
pre-invocation1500-frame marker appear, but the only profile is zero bytes.
No actual model-run/kernel/provider or CUDA-success evidence follows from that
marker. No success metadata/logits/resampled/spectrogram output is created.
The original observer closes1/1 scope with pending/inflight0/errors[]. Main-child
lifetime peak commit1289854976 bytes and peak working set1076211712 are bounded
observations, not a measured improvement or proof of the refusal's cause.
Exact cause remains **UNKNOWN**; proximity to the Job limit is not causation.
The strict independent verifier rejects this actual crash; no limit1 attempt,
exit70 or freshCPU fallback runs. No unchanged retry or guard/OS/process change.
Earlier CUDA1/2/3 failures and their original limits remain distinct and unchanged.

Measured Windows x86_64 Debug binaries (component identity, not full producer):

| Feature | Audio CLI bytes / SHA256 | One-chunk CLI bytes / SHA256 |
| --- | --- | --- |
| static CPU | 36561408 / `68b727f89e4ff2266f97acc211b216e1c93f4c2afcdc97cffd74fcd484fa319a` | 24500736 / `3023a7b9880c93a8912c8b9da1cda72f0110b258987d2126c35dc03bb703af4c` |
| dynamic CPU | 13389312 / `bd69cbe34d3389ba07bd6767ff974499f3de5a11d4289769894f468fde078cc9` | 1381376 / `79a72ba869a31515b72dc70cdbf31d595d41da7d3615767c74439d5ce11dc42b` |
| CUDA | 13416448 / `8c387f2ec0c90fc0db8b358d37c39940b7e29a180d246d2b915454bee4bedc9c` | 1404928 / `6f4c57ed7fb4fdaab7797c0c33e141ad6282aab82219fd73e585f9dd6b490048` |

Reconstruct with the contributor's configured entry/output route and `--locked`:
build both examples for each feature, run `--example beat_this_audio` controls,
then the analysis `--lib` `beat_` selection. Use STEP19/20's exact source/graph/
libsoxr recipes and STEP21's measured dynamic manifests; frontend-only commands
need no ORT initialization. Compare whole resampled/spectrogram files bytewise,
retaining exact source frames/hash and physical-duration policy. Any model attempt
must reuse the original owned runner/observer-before-executor, fresh original
resource admission and full tree/reader settlement. Only independently verified
real narrow device failure permits a later fresh SameModel CPU process.
Logs `performance-step22-{build,controls-static,execution}.log` retain these distinct
passing checks and failed CUDA attempt; generated probes are not durable authority.
Final saved-crash/verifier/settlement and documentation/dependency checks pass2
checks in2.59s (`performance-step22-final.log`), without model execution.
Saved STEP19/21 CPU numerics, STEP20 CPU termination and STEP21 runtime controls/
fullhost UNKNOWN remain compatible reused evidence, not a new green full matrix.
All complete producer/compiler/binary/native dependency roles, licensing/LGPL source
and patches/NVIDIA notices/CRT, release bundles/recovery/Linux/macOS, full footprint
and complete Demucs/BS/source/model/audio acceptance remain open. Installed saving
is zero. Continue independent provenance/packaging/native implementation when fresh
resource conditions do not justify a new CUDA attempt; do not repeat unchanged
failed conditions or lower admission/memory-pattern/precision/quality guards.

## STEP21 impact: explicit standalone runtime admission

The next coherent trial change binds dynamic ORT initialization to an explicitly
supplied, digest-checked package manifest before any ORT API or logger setup.
The manifest identifies its exact runtime file size/hash/version and the current
process OS/architecture. A separate dynamic CPU-only feature permits that same
loading boundary without compiling CUDA support. Static CPU trials reject dynamic
configuration rather than silently ignoring it. An initialization-only command
requires no model or audio processing.

This belongs solely to fresh standalone diagnostics. The application, BeatModel
session arithmetic, graph/weights, frontend, provider policy and original process
admission/retirement authority remain unchanged. The implementation must retain
the checked file handles through all CLI work, reject malformed/changed/foreign
manifests before native loading and fail when environment configuration did not
take effect. A checked file is not a mapped-image or complete-producer proof.
Provider/CUDA/cuDNN/CRT/driver dependencies, compiler/binary provenance, licensing,
installation/recovery and actual Linux/macOS evidence remain separate required
roles. No runtime-only probe certifies device execution or model quality.

The plan grows to23 steps to publish this bounded loading improvement separately.
STEP22 retains the ENTIRE former STEP21 native parity/quality/producer/package/
Demucs/BS requirement set before cutover. STEP23 retains the ENTIRE former FINAL22
regression, original validator/schema/Pydantic attestation, all32 parent failures,
distinct216 watchdog/registry UNKNOWN, pressure-cause/overlap and distribution
gates. Existing working Torch closures remain until full parity is established.

### Runtime contract and reconstruction

The shared standalone bootstrap checks the exact bytes of a maximum4096-byte
manifest, rejects duplicate/unknown/missing fields, and verifies a package-local
regular runtime file with exact size/SHA256 (maximum512MiB). The target identifies
the current process, not an independently attested cross-platform binary. Windows
opens both checked files with read sharing only, denying write/delete; the handles
outlive all CLI model/profile/output work. Other systems require an immutable
caller-owned directory. Canonical path containment rejects a library symlink
escaping that directory. A caller-supplied digest is explicit trial authorization,
not productive weight/runtime provenance.

The runtime's own `OrtGetApiBase`, exact version string and API24 are checked,
then explicit `ort::init_from` supplies the same builder used for optional logging.
Ineffective/repeated environment configuration fails. This API is used exactly
once before all other ORT access in a fresh CLI: ort caches its first library and
`init_from` itself does not prove that a prior different library was absent.
No general App/reload or mapped-image-attestation API is introduced. Dependency
search remains the platform loader's responsibility; provider/CRT/driver closure
still requires separate verification. The initialization-only `runtime` command
does not construct a model session or prove CUDA execution.

Example measured CPU manifest (UTF-8 JSON; hash these exact serialized bytes):

```json
{
  "schema_version": 1,
  "os": "windows",
  "arch": "x86_64",
  "library": "onnxruntime.dll",
  "bytes": 16119840,
  "sha256": "87da0279ab54add397fd518f7022efe8b9a89fd968669d0adff9f9f242344b6c",
  "version": "1.24.4"
}
```

Build the analysis examples with `--locked` and `native-inference-trial` (static
CPU), `native-inference-dynamic` (dynamic CPU) or `native-inference-cuda`. All
are default-off. Serde is an existing-version **development-only** direct edge;
normal analysis dependencies and all productive pins/worker/installer inputs are
unchanged. Use the contributor's configured execution entry and generated-output
route. Run `beat_this_audio runtime ABSOLUTE_MANIFEST LOWERCASE_SHA256` without
runtime environment overrides. For normal `infer`, provide
`FLITZIS_TRIAL_ORT_MANIFEST` and `FLITZIS_TRIAL_ORT_MANIFEST_SHA256`; keep its original
eleven arguments. Both audio and one-chunk CLIs use this boundary. Static builds
reject dynamic configuration. Original output/PCM/shape/provider and graph-SHA
refusals precede ORT loading; frontend-only work never initializes ORT.

CPU runtime bytes come from separately downloaded
[onnxruntime1.24.4 cp312 WindowsAMD64 wheel](https://files.pythonhosted.org/packages/8b/f4/6b89e297b93704345f0f3f8c62229bee323ef25682a3f9b4f89a39324950/onnxruntime-1.24.4-cp312-cp312-win_amd64.whl),
12596856 bytes/SHA256
`535b29475ca42b593c45fbb2152fbf1cdf3f287315bf650e6a724a0a1d065cdb`.
Verify the complete archive against that digest and each extracted DLL against
its RECORD. Extract only `onnxruntime.dll` above and
`onnxruntime_providers_shared.dll`,22048 bytes/SHA256
`838048bae6e1d040e41e4d428a831ac9c5bc91da24abced383d42cb24f2b1376`.
These two files total16141888 logical bytes, not a complete installation or CRT
bundle. The CPU and existing GPU runtime both self-report1.24.4/API24/build Git
`2d924974ef`; that report is not complete build/compiler provenance. The GPU
runtime remains15431712 bytes/SHA256
`c63af7de44303e401b283457fdb510caa65756afa1e8c4f43f90195358c4294b`,
from STEP19/20's independently verified GPU wheel and dependency recipe.
No wheel is installed into a productive environment.

### Verification and actual execution limits

Each of the three feature builds passes seven Rust example controls (six runtime
tests plus the existing fault classifier). They cover strict parsing, target/
size/hash/name/API bounds, relative paths, actual same-file extent/hash checks,
Windows write/delete denial and ineffective repeated environment configuration.
Both standalone examples build for every feature. Initial four unused-import
warnings in static examples are fixed with exact cfg imports, without suppression;
final builds/controls pass2 checks in10.60s. Initial builds passed70.50s. These
overlap, not a combined acceptance matrix.

Twenty real supervised CLI controls pass13.08s: two runtime/API probes, including
a poisoned ambient `ORT_DYLIB_PATH`, and eighteen negative admission/configuration
cases. Original observer scope20/20 closes, pending/inflight0/errors[]. The CPU
wheel preparation passes separately. An initial harness used case-sensitive
`SystemRoot` access on a copied environment dictionary and failed before any child
started; corrected diagnostics retain raw stdout/stderr and the failed log.

One necessary new dynamic CPU-only complete-track execution verifies the new
runtime/package boundary against the **saved** STEP19 reference, with no new
reference inference/export. Original source1536101 frames at48kHz becomes1601
spectrogram frames and two true1500 chunks at starts[-6,107]. All beat/downbeat
times equal; max logit difference0.000049591064453125/RMS0.000005484156854968183;
all3934 profiled nodes use CPUExecutionProvider. Original scope1/1 closes with
pending/inflight0/errors[]. Actual Job limit1428799488 bytes; main-child lifetime
peak commit917692416/working set930390016. This is bounded numerical/execution
evidence, not a quality tolerance, speedup, complete dependency closure or overlap
acceptance. The successful EXE SHA was
`5cac307d620fccbf6a0db1ffa81d08cc55c3a0c86f6f842ae9f03a56836c5b5f`.

One changed-bootstrap CUDA attempt follows fresh original admission: available
commit4934782976 bytes, actual Job budget1393649664, above the earlier observed
initialization peak. Its only model_run has ts1058922us/duration7622054us and twelve
provider-profile entries inside it: five CUDA (including the **failed Conv**) and
seven CPU. Provider entries do not prove five successful physical CUDA kernels.
The Conv fails at `cudnnAddTensor` with CUDNN4003,
`CUDNN_STATUS_INTERNAL_ERROR_HOST_ALLOCATION_FAILED`; exit1, no success metadata,
logits/resampled/spectrogram artifacts. Main-child peak commit1392754688/working
set689483776; scope1/1 closes, pending/inflight0/errors[]. The precise allocation
refusal cause remains **UNKNOWN**; proximity to the Job limit is not causal proof.
The independent fault verifier rejects it. No limit1 fault, exit70 or freshCPU
fallback runs. Do not repeat unchanged conditions or lower original guards.
The attempted CUDA EXE SHA was
`bcee8da18ba54c51b9068022d1d1be2a46ecc5854882d3996d648593710489b9`.

Final Windows x86_64 Debug binaries after import/format-only cleanup:

| Feature | Bytes | SHA256 |
| --- | ---: | --- |
| static CPU | 36561408 | `82bf8fea1f95bfc113c0c22e9c37f8c05cc6f8a74b3b31915e462c22b5a438b9` |
| dynamic CPU | 13389312 | `1a11cf78dd8ce2663f96af22f38d33ccdf92832e8ba37e9269a37bb0d43e074b` |
| CUDA | 13415936 | `50d141dc9e772c446f7be5257fd92f14c7a2af99b12bee8219a0abd081deb315` |

Compiler reports rustc1.95.0/59807616e, LLVM22.1.2, Cargo1.95.0; no full compiler/
binary/producer attestation follows. Current source is execution-compatible on
review; NN is not repeated merely for final EXE hashes. Final22 static/control
checks pass23.48s:16 unchanged strict fault protocol cases, four Rustfmt checks,
strict OpenSpec/local Markdown targets, locked normal dependencies across four
feature trees, twelve original Rust core checks, saved real settlement/failure
evidence and two original fullhost UNKNOWN probes with all productive roles.
Default external Release Native/API retains22 known warnings/einops pin; the
analysis test target retains its two unchanged CQT-test warnings. Logs are
`performance-step21-{headroom,build,build-final,controls,controls-fixed,execution,
static}.log`; generated probes/logs are reconstructible, not durable authority.

All graph/weight/exporter/frontend/source/runtime/provider/native-dependency/
compiler/binary/OS/architecture roles still require full attestation before
cutover. CRT/license/LGPL source+patch/NVIDIA notices, release CPU-only/CUDA bundles,
recovery, Linux/macOS execution, complete install footprint and full Demucs/BS
paths remain open; installed saving is **zero**. A source-motivated next step may
release graph/PCM buffers after their final use, after checking ORT's copied-memory
session semantics, without changing math, memory patterns, guards or claiming a
proved CUDA fix. Actual CUDA completion and independently confirmed narrow device
fault followed by fresh SameModel CPU remain mandatory in STEP22. STEP23 retains
every original combined regression/quality/architecture/distribution gate above.

## STEP20 impact: original input envelope and fail-closed native diagnostics

The isolated complete-source trial now admits the original worker's mono PCM
envelope:512MiB, rates1–768000Hz and at most250000 prediction frames. Checked
integer ceiling conversion precedes resampling/STFT allocation. Source bytes,
resampled capacity and prediction count have distinct bounds; an oversized
actual libsoxr tail fails rather than being truncated. The CLI stats and reads
the same opened PCM file, rejects size changes/partial/nonfinite data, bulk-hashes
the actual raw bytes and releases that raw buffer before resampling. No model,
frontend arithmetic, chunking, precision or productive dispatch is replaced.

### Source and control verification

Twelve selected Rust tests and the standalone diagnostic test pass. Fifteen initial
Python protocol tests require the exact dedicated receipt, graph hash, exit70
and independently captured ORT allocator logs; generic host/unknown errors,
Cpu/CudaPinned, different threads, changed rounding and missing/corrupt/truncated
evidence remain terminal. These are control tests, not CUDA execution evidence.
Final review adds a sixteenth regression: exactly65536 stderr bytes can be the
original reader's truncated tail and are rejected even when valid-looking logs
remain at the end. No original reader limit is enlarged.

Final standalone CPU/CUDA builds pass. Ten real early-negative commands pass:
existing output preservation, invalid rate, changed libsoxr/graph SHA, source
over512MiB, prediction overflow, partial/nonfinite PCM, CUDA requested from the
CPU build and invalid explicit arena diagnosis. The two large file cases are
rejected from metadata before payload allocation/native loading. Their success
does not measure processing a maximum-size source.

All six prior complete native resampled/spectrogram pairs are byte-identical to
STEP19. Their original-worker numerical differences
therefore remain unchanged. Two additional original frontend references use the
unmodified separately locked Python3.12.13/Torch2.8.0+cpu worker, with **no model
load/inference**. Neither productive Python/Torch closure is modified:

| Rate / source frames | Original spectrogram frames | HQ resample | Maximum LogMel difference | RMS |
| --- | ---: | --- | ---: | ---: |
| 384001 / 384014 | 51 | bit equal | 0.00012800640251953155 | 0.000010358720334093284 |
| 768000 / 768101 | 51 | bit equal | 0.00008262949995696545 | 0.000009640202272107286 |

New source SHA256 values are respectively
`333aeb620ee81072d130926b213c3c36b3cfac09c8990bc401b0b6c7b44e275d`
and `25ed4c1f1e221c295de11afa64e1071b125a55dea755d0635cc19ba20ddd6897`.
These bounded comparisons establish no general audio/model tolerance. Original
STEP19 full-model references are reused without another numerical NN round.
The original supervisor observes9/9 frontend scopes and10/10 negative scopes
closed, pending/inflight0 and no sampling errors, using one joined executor per
check. CPU setup/frontends/negatives pass3 checks in263.01s, including download;
the separate final-build check passed before these. Counts overlap, not a matrix.

### Actual cancellation and unsuccessful CUDA attempts

One real native CPU1500-frame inference is terminated through the explicit
joined50ms RunOptions timer. Its unique profile has exactly one `model_run`
(timestamp762902us/duration50184us); all164 CPU-provider kernel events fall
inside that failed run, including real MatMul. The error is termination-specific,
and no success metadata, logits, resampled or spectrogram output is produced.
The original supervisor closes both attempted-CUDA and CPU-cancel scopes2/2,
pending/inflight0/errors[]. CPU main-child lifetime peak commit308736000 bytes,
peak working set323379200. This is bounded CPU termination/settlement evidence,
not general memory, playback deadline, CUDA, overlap or whole-model quality proof.

The same check's native CUDA attempt fails during initialization with a generic
BFCArena4194304-byte refusal. Before-launch available commit4528058368 bytes;
main-child peak commit1254162432. No CUDA kernels or eligible device receipt are
proved. Cause remains **UNKNOWN**; proximity to a resource ceiling is not cause.

Source inspection then found that the CUDA-only dispatch lets ORT add its default
CPU host arena, whereas the explicit CPU route already disables that arena. The
candidate now applies explicit CPU `with_arena_allocator(false)` **after required
CUDA**, preserving CPU shape/operator support, both error-on-failure settings,
TF32off and all original thread/resource options. This is a source-motivated
allocator-policy attempt, not a proved memory fix. See the
[wrapper implementation](https://github.com/pykeio/ort/blob/v2.0.0-rc.12/src/ep/cpu.rs)
and [ORT CPU insertion](https://github.com/microsoft/onnxruntime/blob/v1.24.4/onnxruntime/core/session/inference_session.cc#L1945).
Memory-pattern options, graph, weights, precision and resource floors stay intact.

One changed-implementation CUDA attempt is made under fresh original admission:
available commit3851194368, observed actual Job budget853358592 bytes. It exits
`0xC0000005` without diagnostic output/profile/run evidence; main-child peak
commit860307456. Original scope1/1 closes, pending/inflight0/errors[]. Cause is
**UNKNOWN**, with no claimed allocator improvement, CUDA success or fallback.
An earlier script quoting error was rejected at collection before this process;
it is retained separately. Do not repeat these unchanged failed conditions or
lower guards/change unrelated processes to obtain a positive result.

The standalone opt-in `FLITZIS_TRIAL_CUDA_ARENA_DIAGNOSTIC=1` accepts only an
explicit CUDA limit1 request without cancellation. Typed exit70 additionally
requires one actual Cuda/limit1 constructor, the **last same-thread** Cuda Extend,
positive exact256-byte rounding and the matching BFCArena exception. A later
Cpu/CudaPinned/unknown extension invalidates the match. The separate
[verifier](../../../scripts/beat_this_trial_fault.py) independently checks stdout
and captured stderr after original tree **and** readers return. This denotes
controlled device-arena refusal before cudaMalloc, not physical VRAM exhaustion;
[ORT's allocator source](https://github.com/microsoft/onnxruntime/blob/v1.24.4/onnxruntime/core/framework/bfc_arena.cc#L89)
explains that distinction. No actual matching exit70/freshCPU is obtained here;
the mechanism remains an isolated trial, not productive fallback policy.

### Component inventory, reconstruction and remaining program

Measured platform: Windows AMD64/build10.0.26200, rustc1.95.0/59807616e,
LLVM22.1.2 and Cargo1.95.0. Final Debug CPU EXE36518400 bytes/SHA256
`42c5b84713e275dc05b3ddc0937c28ffbadf591bf358d81692ef52cf2f1ad024`;
CUDA EXE13146624/SHA256
`364bb4c3f604a3c88f6e86155e6d7421b717de5f8077ebf4d897afe35fe883b8`.
The cancellation and failed CUDA observations precede final bulk-SHA extraction;
the CPU inference path remains source-compatible on review, not rerun for hashes.
Final binaries have the fresh frontend/early-negative evidence above.

The owned CUDA scratch copies21 CUDA/cuDNN DLLs from the separate BeatThiscu128
wheel and the three pinned ORT DLLs:24 files/3492501392 logical bytes, no Torch
DLLs. Every copied file matches its authenticated wheel's RECORD hash and size.
Torch wheel3461384651 bytes/SHA256
`0ad925202387f4e7314302a1b4f8860fa824357f9b1466d7992bf276370ebcff`
matches the unchanged CUDA worker lock. ORT wheel provenance remains STEP19's
`da5c1e327d8e119a831be2790e69f93cf6daab9145ed0aca7577f412a620f709`.
The separate CPU-worker environment has15391 files/3243748526 logical bytes;
its shared base interpreter is outside that count. These are component file
measurements, **not physical deduplication or full installation totals**.

Original observer polls additionally sample candidate Windows module paths,
including ORT/CUDA/cuDNN/CRT/driver files; five loader-race299 errors occur in
the first check. That PID union is not full identity-attested loaded closure.
The next attempt keys paths by creation identity, but still lacks CUDA execution.
Neither file hashes, RECORD consistency nor compiler version alone attests the
complete binary/producer or mapped images. Release/CRT/license bundles, LGPL
source/patch obligations, NVIDIA notices, complete CPU-only/CUDA installation,
recovery and Linux/macOS build/run remain open. Actual installed saving is **zero**.

Reconstruct only in owned cache through the outer contributor entry and ACTUAL
native checkout. Use unchanged `workers/beat_this/uv.lock` with isolated
`UV_PROJECT_ENVIRONMENT`, `uv sync --locked --no-dev --no-install-project
--python 3.12.13`. Run the existing
reference generator's `generate` with `checkpoint=None` and CASES replaced only
in a disposable helper by the two rate/frame tuples above. Its unchanged
`synthetic_signal` defines every source sample. Use STEP19's retained graph and
fixture recipes; do not overwrite reference evidence. Rebuild the two features
with the commands below. Copy only DLL names beginning
`cud,cublas,cufft,curand,cusolver,cusparse,nv` from the pinned CUDA wheel, plus
the three ORT DLLs; verify archive SHA and RECORD, keep the owned paths immutable.
Execute every neural command through the original admitted supervisor in one
joined executor, observing complete settlement before any next same-model CPU.

Positive AND failed logs are retained as `performance-step20-{headroom,format,
format-final,build,runtime,allocator-build,runtime2,runtime2-actual,controls,
controls-cpu,inventory,inventory-final}.log` in cache. Initial reference logging
lost a non-UTF8 diagnostic before any reference folder existed; underlying cause
is UNKNOWN. The corrected logger preserves raw bytes. The locked CPU worker then
produces the new references without model inference. The first inventory used a
wrong assumed wheel directory; its corrected check passes7.62s. Generated scripts
and raw logs are reconstructible; this source/recipe/history is durable authority.

Final22 static/control checks pass14.63s:16 protocol cases (including the reviewed
saturated-tail fix), Ruff/format on both Python files, four Rust format checks,
strict OpenSpec,85 Markdown targets/whitespace, all productive pins/locks/host
paths unchanged, three independent normal-dependency feature trees, saved actual
same-run cancellation/failure settlement and final component/frontends/negative
evidence. Both fullhost probes retain the original StemExecution validator
UNKNOWN with all productive resource/provider roles present. Default external
Release Native/API remains usable with22 known warnings and unchanged einops.
Systems PRE/POST/affected, Methods and one independent actual-diff review chain
cover isolated publication, not cutover. `performance-step20-{static,static-final,
index}.log` record the final checks; earlier counts overlap and are not additive.

The justified plan is now22 steps. **STEP21 retains ALL native CUDA/real device
fault→freshCPU, CPU-only, full quality/functions, resources/last-reader, every
producer role, OS/architecture, dependency/installer/recovery and practical
Demucs/BS full-path parity gates before any cutover.** Existing closures remain.
**STEP22 retains the ENTIRE former FINAL21**: original fullhost validator/schema/
Pydantic/all roles, exact32 Rust parent failures/watchdog/registry, pressure
UNKNOWN, CPU/CUDA overlap and combined quality/regression/architecture/docs/
distribution. The successful CPU termination above closes none of those broader
gates. Working Demucs/BS whole-track normalization/shifts/overlap, complex
STFT/ISTFT/masks, attention/recurrent and AMP paths still need implemented,
measured complete parity; an isolated mask graph is not their replacement.

## STEP19 impact: complete-source native CPU prototype

The opt-in standalone `beat_this_audio` extends the independent Rust/ORT trial
from mono float32 PCM through libsoxr HQ, centered LogMel, original chunk planning,
aggregation and minimal postprocessing. It does not decode a second source,
truncate audio, change App dispatch or publish an application receipt. Productive
BeatThis CPU/CUDA, Demucs/BS, provider probes, weights, locks, installer and recovery
remain unchanged. Actual installed saving remains **zero**. The shared core has
no Python, PyO3 or LibTorch; native libraries are separate dependencies.

### Original math and dynamic graph

libsoxr0.1.3 uses FLOAT32/HQ/default one-thread runtime, original wrapper block
size `max(1000,48000*in_rate/22050)` and a real final flush. The DLL is explicitly
supplied, bounded and digest checked. Its owned path must remain immutable;
file hashing does not attest an OS-loaded image or the complete producer.

LogMel uses periodic Hann1024, centered reflect512, hop441, magnitude divided by
sqrt1024,128 Slaney-frequency bins30–11000 Hz with **norm=None**, then
`log1p(1000*x)`. No Slaney area normalization. Starts are `arange(-6,T-6,1488)`;
when T>1488 the last start becomes T-1494. T<=1488 therefore really runs T+12
model frames; T1489 has starts[-6,-5]. Six logits are removed at each edge and
earliest chunks win overlap. Chunks are processed sequentially with one retained
output, rather than retaining all prediction chunks. Minimal postprocessing
uses positive equal maxima in +/-3 frames, adjacent deduplication against the
running mean, first nearest-beat ties and unique sorted downbeats. Physical
duration exclusion follows snapping; all complete logits are retained.

The original strict final0 loader and STEP18 tooling are reused. For the dynamic
export only, the one shared original RotaryEmbedding has caching disabled;
cached and uncached1500 logits must be **bit identical** before export. Weights
and productive packages are untouched. Legacy opset18/dynamoFalse export creates
one82,104,250-byte inline graph,138 initializers/5354 standard-domain nodes,
SHA256 `46bfb2c1a1f1e5c1d8786d0d9462f285971ff7fa8a592ac5f4ab609698bd44ca`.
This is the **measured artifact's** byte identity. Its measured exporter source
SHA256 is `d4e5a5ca65a7142cfa224824dcb9ab4f8a2444ce20f8a5cc943f34a146a7730f`,
retained in three equal pre-refactor source snapshots. Final helper extraction,
early existing-output guards and formatting preserve arithmetic on inspection;
the final exporter is not rerun solely for those changes. Tracing source metadata
may change serialized graph bytes, so the final script has not reproduced that
exact graph hash. Complete exporter/graph producer attestation remains open.
Original1500 reference SHA remains STEP18's `79e7039f37f91d9f2351e97a6e2cdeac14172d24fc58f724ad2c5218a505702a`.
Fixed batch1, three-way QKV/two-way heads and feature-dimension tracer warnings
remain recorded. Output batch metadata is inferred/unknown; input batch stays
exactly1, and every actual output must be finite float32[1,actualT]. The first
attempt rejected this metadata **before inference**; handling unknown metadata
retains the exact actual-output guard. No padding or graph-metadata rewrite.

### Bounded actual numerical comparisons

`scripts/beat_this_trial_reference.py` reconstructs all synthetic sources using
the unchanged pinned worker. Two short22050-Hz cases have513/22949 PCM frames;
44.1/48/96-kHz cases have10001/48013/96017 frames; the48000-Hz track has1536101
frames (32 seconds plus101 samples). The tracked generator defines every sample,
including first/last impulses and a500ms decaying70-Hz transient. No private audio
or listening evidence is used. All six complete resampled arrays are bit equal.

| Case | Spectrogram frames | Maximum LogMel difference | RMS difference |
| --- | ---: | ---: | ---: |
| short14 | 2 | 0.000003814697265625 | 0.0000005289518692597156 |
| short65 | 53 | 0.00011985934543190524 | 0.000009198060005695107 |
| 44100 | 12 | 0.00008272804552689195 | 0.000008660001856043758 |
| 48000 | 51 | 0.00010343178291805089 | 0.000009397934508143894 |
| 96000 | 51 | 0.0001018625043798238 | 0.000009754261191765678 |
| complete track | 1601 | 0.0001449843548471108 | 0.000009849352930371131 |

Three full original-model references are generated once and reused. Actual native
CPU runs use14,65 and two1500-frame chunks (track starts[-6,107]), including the
native frontend rather than original spectrogram input:

| Case | Maximum logit difference | RMS | Executed CPU nodes | Beat/downbeat counts |
| --- | ---: | ---: | ---: | ---: |
| short14 | 0.0000064373016357421875 | 0.000004848902878621278 | 1967 | 1/0 |
| short65 | 0.00001049041748046875 | 0.000003957750487866306 | 1967 | 2/1 |
| complete track | 0.00005078315734863281 | 0.000005471285692518955 | 3934 | 65/65 |

All detected times match the original worker exactly in these cases. These are
finite numerical comparisons, **not general model/audio quality tolerances**.
Wall spans2.4131447/2.0741781/8.1958463 seconds include the owned command/output;
there is no paired speedup claim. The CPU harness incorrectly created the original
thread-profile observer while calling commands on the current thread: it recorded
**zero scopes**, so its memory/retirement report supplies no child evidence.
Do not rerun these models merely to improve measurement. The original synchronous
supervisor remains used; separately observed failure settlement follows below.

### Actual resource refusal and remaining native gates

The explicit CUDA candidate builds with TF32 disabled and local1/1 sequential,
nonspinning sessions. Developer-only dynamic ORT loading is opt-in; default App
features do not select CUDA or libsoxr. Actual hardware execution is **unproved**.
The attempted real mid-ORT timer diagnostic instead failed during model load:
`Exception during loading: bad allocation`, before any run marker/kernel. Actual
host available commit was2,612,334,592 bytes; the original half-remaining allowance
after the2GiB floor allowed only about230MiB. Child lifetime private peak was
225,112,064 bytes, sampled concurrent owned private226,291,712/WS223,981,568.
One scope starts/closes, pending/inflight0/errors[],85 main-child samples/90 total
observer polls; the original tree
and readers settle. This is a **terminal host/Job-limit failure**, not cancellation
or a CUDA failure. A subsequent CUDA attempt was refused **before process launch**
by original `OfflineChildMemoryReservation`:134,217,728 requested with140,152,832
host bytes available after reserve. No GPU result or fallback is invented.

The standalone CLI has a joined, explicitly requested RunOptions timer diagnostic;
actual termination-specific error plus executed same-run kernels and true final
tree/reader end are still required. Its once-only marker precedes invocation and
is not proof of mid-ORT execution. Optional allocator logging is confined to the
standalone diagnostic process. A generic BFCArena/host error never authorizes
CUDA fallback. Even `gpu_mem_limit=1` requires independent Cuda-allocator identity
and the exact rounded allocation refusal before any diagnostic exit70 receipt;
it is not physical VRAM exhaustion. The working productive fallback remains.

No resource floor/cap, unrelated process, OS/pagefile setting or quality option
was changed. A concurrent large unrelated process is only a current observation,
not the cause of the earlier Human pressure case. KeyNet's effective EP, fullhost
original-validator UNKNOWN, CPU/CUDA overlap and all historical negatives remain.

### Native dependencies, reproduction and split

The trial Windowsx64 Release libsoxr DLL is228352 bytes, SHA256
`b2e3a0ff4806540250fa45e9522c3a3549663888397fce4708249369b25db37e`.
Reconstruct using vcpkg020c8202737f's `soxr:x64-windows`0.1.3#8 (default noOpenMP),
SourceForge archive SHA512
`f4883ed298d5650399283238aac3dbe78d605b988246bea51fa343d4a8ce5ce97c6e143f6c3f50a3ff81795d9c19e7a07217c586d4020f6ced102aceac46aaa8`,
with its named001_initialize-resampler/002_disable_warning/003_detect_arm patches.
Dynamic LGPL dependency/source/license distribution remains a bundle obligation.
Do not replace it with Rubato or a Python extension.

The separate CUDA scratch comes from official PyPI
`onnxruntime_gpu-1.24.4-cp312-cp312-win_amd64.whl`,207,197,978 bytes, SHA256
`da5c1e327d8e119a831be2790e69f93cf6daab9145ed0aca7577f412a620f709`.
Only native DLLs are extracted; no wheel is installed into productive environments.
ORT DLL15,431,712 bytes/SHA256
`c63af7de44303e401b283457fdb510caa65756afa1e8c4f43f90195358c4294b`,
CUDA EP312,608,800/SHA256
`110929d2552bc775828e7a5c18c5ef9ab90898fc57a57868623fe2fe8c82d494`,
shared22,048/SHA256
`f7ec022e2b5a5126f606db4b46da795cacc6cbae4788ed5188b046bbefc13717`.
Final Windowsx64 Debug CPU EXE36,456,448 bytes/SHA256
`96bc32667cb1e2187c91d9a5b9b001cba2509ea90e39a703d5686918ae9f852e`;
dynamic-runtime CUDA EXE13,085,184/SHA256
`2ea70135c309b9592e672ce9124434f231e3e60f2ec10eee72460a15eb62c4c6`.
These are logical artifact sizes, not full installations or physical deduplication.
Final binaries are built and six real early-negative cases pass: existing output
retains bytes; partial/nonfinite PCM, changed libsoxr/graph SHA and CUDA request
to the CPU-only build fail before neural execution. Neural numbers above precede
the final conditional diagnostic logger/comment cleanup; normal inference
arithmetic is unchanged on review, not newly rerun for those changes.
Extracted TensorRT847,384 bytes is unused. An initial assumed1.24.2 PyPI endpoint
returned404, before download/build; verified1.24.4/API24 scratch then built.
The attempted CUDA child PATH would use the existing separate BeatThis cu128
CUDA/cuDNN DLL directory, not App cu130 or LibTorch inference. It was never
launched, so no loaded closure/provider identity follows. Complete CUDA/CPU-only
closure, CRT, optimized release bundle and Linux/macOS build/run remain unmeasured.
The trial currently bounds PCM256MiB/rates<=384kHz/resampled256MiB; these explicit
refusals are trial restrictions, not full productive512MiB/768kHz parity.

Use the configured outer contributor entry with the actual native checkout:

```text
cargo build --locked --manifest-path rust/Cargo.toml -p flitzis-looper-analysis --example beat_this_audio --features native-inference-trial
beat_this_audio frontend SOXR_ABSOLUTE SOXR_SHA PCM_F32 RATE NEW_JSON
beat_this_audio infer GRAPH GRAPH_SHA SOXR_ABSOLUTE SOXR_SHA PCM_F32 RATE NEW_JSON PROFILE cpu MAX_GPU_BYTES
```

Generate frontend references first with `scripts/beat_this_trial_reference.py
--worker workers/beat_this/worker.py --output NEW_CACHE_DIRECTORY`; add
`--checkpoint ABSOLUTE_FINAL0` in a fresh process for the three model references.
Use `scripts/export_beat_this_trial.py --dynamic` with STEP18's exact1500 frontend
and the same tooling lock. CUDA build adds `native-inference-cuda`, a separate
target directory and explicit `ORT_DYLIB_PATH`. All commands require the original
owned resource supervisor. Keep reconstructed assets/logs only in the project
cache; source/recipes remain durable. Positive and failed logs:
`performance-step19-{build,frontend,model,model-second,cuda-setup,cuda,gpu-attempt}.log`.
The source generator, exact arithmetic and graph recipe allow reconstruction
after cache loss; existing C scripts are not durable authority.

Final five static/control checks pass29.32s: Ruff/format on both tools, five Rust
format checks,11 selected Rust tests (five native-trial plus six compatible
analysis tests), three normal-dependency feature trees without PyO3/Torch/LibTorch,
all six original fixture hashes, exclusive-output refusal before source/model
access, strict OpenSpec,43 Markdown targets/whitespace, unchanged productive pins
and Cargo's sole optional libloading0.9.0 addition. Both complete-host probes keep
the original `StemExecution` unsupported-validator UNKNOWN with every old/new
productive resource/provider role retained. Separate final binary/negative gate
passes11.57s. Runs overlap and are not an additive whole-system matrix. Initial
Ruff complexity/locals and module-order formatting failures are fixed by helper
extraction/formatting without suppression; their failed logs remain. Default
Release Native/API passes with22 known library warnings and the existing einops
pin. Systems PRE/frozen POST/affected, Methods and the single independent final
actual-diff review chain are clear for isolated candidate publication only.

The STEP19 split reached21 steps: it delivers the complete-source CPU prototype;
**STEP20 retains every native CUDA/fresh-CPU/cancel/memory/last-reader/quality/
provenance/OS/distribution/recovery gate before cutover**, including practical
Demucs/BS migration barriers from STEP18. First obtain fresh real guarded
headroom, rather than repeat unchanged admission failures. **STEP21 retains ALL
former FINAL20 gates** (original fullhost validator/Pydantic/all roles, exact32
Rust failures/watchdog/registry, pressure UNKNOWN, overlap and combined regression/
architecture/docs/distribution). Productive Torch removal is not yet justified.

## STEP18 impact: candidate native BeatThis inference

The opt-in `native-inference-trial` analysis feature and developer exporter evaluate
the original pinned BeatThis final0 network as self-contained ONNX, using a
Python-independent Rust/ORT core and standalone executable. The application does
not select this experiment. No productive Torch dependency, weight pin, dispatcher,
installer or recovery path is removed or replaced. Existing source/OwnACK/last-reader,
resource admission, playback and realtime authorities remain unchanged.

The first boundary is exactly one finite float32 `[1,1500,128]` spectrogram and
both `[1,1500]` raw logit outputs, compared with the same original strict-loaded
model. Model bytes are bounded and SHA-verified before in-memory ORT parsing;
session-local threads and actual executed-node profiling are separate from model
quality. The trial caller must use an admitted disposable offline process.
Synchronous chunk cancellation is checked before/after ORT, with process retirement
required for prompt interruption. No Python binding or LibTorch is involved.

This is not complete-source replacement: soxr HQ, centered Hann/Slaney log-mel,
short chunks (padding them to 1500 changes attention), border6/keep_first,
minimal postprocessing and physical duration exclusion still use the original
worker. Productive cutover requires their full parity, actual native NVIDIA CUDA
and real same-model device failure followed by fresh CPU execution, CPU-only
distribution/recovery and cancellation/memory evidence. Windows executable evidence
does not establish a portable bundle or Linux/macOS build/run acceptance.

### STEP18 productive closure inventory and measured footprint

| Closure | Remaining productive work and concrete migration boundary |
| --- | --- |
| App Python 3.14 / Torch 2.12 cu130 | Demucs 4.0.1 owns normalization, complete-track input/output, shifts/overlap, hybrid spectral/time network, complex STFT/ISTFT, convolutions, recurrent/attention operations and resampling. The existing streaming adapter replaces output encoding, not this neural closure. |
| Same App closure, vendored BS-RoFormer | Strict pinned MUSDB18HQ checkpoint (527,385,512 bytes) and config; complex STFT/view-as-real/complex masking/ISTFT, frequency/time attention, RMSNorm, rotary/gating/SDPA, chunk reflection/overlap and CUDA AMP. A real-tensor mask-network export alone would not preserve the full spectral frontend/reconstruction and current quality. |
| Separate BeatThis Python 3.12 / Torch 2.8 CPU or cu128 | Exact final0, soxr HQ, torchaudio Hann/Slaney log-mel, Conv/BN/GELU, RMSNorm/Linear/rotary/SDPA, variable chunks, border/aggregation, minimal postprocessor and execution/fallback protocol. STEP18 actually exports and executes the fixed network below; the rest remains required. |
| Provider probe | App Torch capability/runtime/allocator identity is also used by the original guarded device probe. Removing inference imports alone cannot preserve those receipts. |
| Already native / other runtimes | KeyNet uses independent Rust CQT plus ORT; PCM conversion is independent Rust. The metrical-prior worker uses its separate JS runtime. None constitutes removal of a productive Torch closure. |

Observed logical file bytes on Windows x64 (including files such as existing
bytecode; not allocated disk size or deduplicated physical ownership):

| Actual directory/artifact | Files | Bytes |
| --- | ---: | ---: |
| App `site-packages/torch` | 11,768 | 2,878,212,219 |
| App `site-packages/torchaudio` | 81 | 8,451,029 |
| Existing BeatThis **CUDA** `site-packages/torch` | 12,303 | 7,402,993,596 |
| Exported inline ONNX graph | 1 | 82,573,488 |
| Final standalone Debug Windows EXE | 1 | 24,413,184 |
| Original final0 checkpoint | 1 | 81,058,141 |

The existing measured worker is actually `2.8.0+cu128`, even though export and
reference execute on CPU using the original CPU loader. CPU-only worker size,
complete dependency/DLL/CRT closure, hardlink deduplication, optimized release
bundle size and Linux/macOS packages are **unmeasured**. These are not complete
installation totals. No installed runtime was removed; actual installed saving
is zero. Export tooling is separately hash-locked for Windows CPython 3.12 and
installed in disposable scratch, never into the productive worker environment.
Production App/CPU/CUDA locks, original model pins, installer and recovery remain
unchanged. The installer does not download or select the candidate graph/EXE.

### STEP18 actual export and independent native execution

One deterministic mono float32 source at 22,050 Hz, 661,059 frames, is generated
from binary64 `t=arange(1499*441)/22050` and
`(.2*sin(2*pi*220*t)+.1*sin(2*pi*431*t)).astype('<f4')`.
The original complete-source frontend yields exactly 1500 by 128 float32 values.
This is a roughly 30-second synthetic fixture, not private audio or listening
acceptance. Source SHA256 is
`6e6a1c4ce61a3e8f57823940c18c0e39fff4541a2771cedc1781649472a6cd23`;
spectrogram SHA256 is
`63c68d1c0d2df56331bf958297d1700ca356fba8f4809902b6a79ff1a80cac38`.

The original `load_local_model` hashes and loads the same retained final0 handle,
strictly reconstructing hyperparameters/state_dict. Pinned checkpoint SHA256 is
`8c328b45f59d8dd3dff219253ff6a8d6482be57d0133a29140e2febbf8eb8331`.
Cold legacy `torch.onnx.export(dynamo=False, opset_version=18)` and ONNX 1.19.1
full checker produce one inline graph, 138 initializers / 2280 nodes, standard
ONNX domain only. No quantization, altered weights, model rewrite, warm-up,
LibTorch or external weight files. Export warnings about fixed shape, tensor
iteration and rotary cache branches are retained; this graph is not dynamic.
Actual graph SHA256 is
`22ac0c62301709a53834453a608aeaf85a12b8ecb4b3bfbf26aafee42c21b7fa`.

The same original model/input supplies both reference streams, SHA256
`79e7039f37f91d9f2351e97a6e2cdeac14172d24fc58f724ad2c5218a505702a`.
One standalone Rust/ORT execution on Windows x86_64 produces 3000 finite logits:

| Stream | Maximum absolute difference | RMS difference | Relative L2 |
| --- | ---: | ---: | ---: |
| Beat | 0.0000025033950805664062 | 0.0000007906240276194365 | 0.000000504598233199382 |
| Downbeat | 0.000003337860107421875 | 0.000000960786692802246 | 0.0000004817917689484372 |

These are measured numerical differences for one exact chunk, not bit equality
or an audio-quality acceptance tolerance. Native output SHA256 is
`f75602be01ba9035690d0769c647ee746a4d2262d1658ff81c1142af46006273`.
ORT's actual executed-node profile assigns all 992 optimized executed nodes to
`CPUExecutionProvider`. The candidate explicitly replaces rc12's automatic device
policy and configures CPU, sequential execution, local intra/inter-op threads1
and disabled spinning. This evidence does not retroactively attest KeyNet's EP.

The owned command, including startup/load/hash/run/output/profile and real
tree/pipe-reader retirement, takes 2.8970175 seconds. No paired Torch inference
wall was measured, so this is not a speedup. Original supervisor observation:
88 samples, child lifetime peak commit 628,264,960 bytes / peak WS 641,540,096;
sampled concurrent owned private 629,424,128 / WS 642,453,504 bytes; one scope
starts/closes, pending/inflight0, errors[]. The existing observer classifies only
known productive modules as inference trees (candidate counter0), so the actual
EXE command and ORT profile establish this candidate's execution. Ordinary Job
admission/half-budget and actual final-reader settlement are reused unchanged.
No leak, hard-paging, memory plateau, playback deadline or pressure-cause claim.

### STEP18 verification, reproduction and remaining split

Build the opt-in candidate with the normal configured contributor environment:

```text
cargo build --locked --manifest-path rust/Cargo.toml -p flitzis-looper-analysis --example beat_this_native --features native-inference-trial
beat_this_native GRAPH GRAPH_SHA256 SPECTROGRAM_F32 NEW_OUTPUT_F32 PROFILE_PREFIX
```

First install `scripts/native-inference-requirements.lock` with `--require-hashes`
into separate export-tool scratch, using the existing BeatThis interpreter and
a temporary `PYTHONPATH`. In a **fresh process**, run
`scripts/export_beat_this_trial.py --worker workers/beat_this/worker.py
--checkpoint ABSOLUTE_FINAL0 --spectrogram SPECTROGRAM_F32 --output NEW_DIRECTORY`.
Use the original frontend/fixture above; do not silently pad a short piece to
1500. Both output streams are contiguous little-endian float32, beat first.
Developer trial assets are not product provenance or a portable installation.

Two Rust negatives pass: changed/empty graph rejected before runtime parsing and
cancel-before-load. Five actual EXE negative cases pass without neural execution:
wrong graph SHA, truncated spectrogram, nonfinite input, existing output (original
bytes retained) and invalid ONNX with a matching digest. Each original owned
scope closes once, pending/inflight0 and no sampling errors. The positive run's
core is byte-identical to final `beat_inference.rs`, SHA256
`0fcc47e18277f457cdce3b834d77044968c61e8f6d1ce61cadc0ddce8bfc6729`.
Final CLI adds only early invalid-input/output refusal; its normal inference
path is unchanged. The successful Debug EXE SHA256 was
`5e28b3081392657a63e7f9c23f1587d8a80f82090eef9ce292a04b8bbd97133e`.
The measured exporter SHA256 was
`55a427a45b7d4d924e78011a723d0a754d2d1ae405a9dfd5d09852b65e1436db`;
later bounded-read/local-name/stdout cleanup changes no export/model arithmetic.
No duplicate NN run is used solely to improve these numbers.

The first harness incorrectly configured Torch inter-op threads twice in one
process and failed **before model load**. The frontend bytes were retained and
the real export then succeeded in a fresh process. Initial two Ruff issues were
fixed without suppression. Positive and failed logs remain separately retained.
This is scoped native inference progress, not a green whole-model/system matrix.
Final four static/runtime checks pass (16.66 seconds), including both repeated
Rust negatives, Ruff/format, three Rust format checks, strict OpenSpec, 42 local
Markdown targets/whitespace, unchanged productive pins and both complete-host
UNKNOWN probes. All old/new provider/resource roles remain present; the original
unsupported `StemExecution` validator still fails closed. Default Release Native/API
verification passes with the same 22 known library warnings and retained einops
pin. Systems PRE/frozen POST, Methods PRE and one independent actual-diff review
chain are clear for this candidate only. Existing overlapping checks are not added
into a claimed whole-system success total.

The migration therefore splits coherently: **STEP19** must attempt the complete
native BeatThis frontend/variable-length/chunk/aggregate/postprocessor/provider
path and practical remaining Torch removal, with actual native CUDA and true
same-model device-failure-to-fresh-CPU proof, resources/cancel/reader/provenance/
distribution before any cutover. Concrete Demucs/BS barriers above retain their
working paths unless equivalent replacements are actually proved. **STEP20**
retains ALL former final STEP19 gates: complete-host original-validator/schema/
Pydantic and every old/new role attestation, exact 32 parent Rust failures,
distinct216 watchdog, registry UNKNOWN, pressure cause UNKNOWN, CPU/CUDA overlap,
combined 2/4/6 and <=10 stress, all quality/functions/interactions and architecture/
documentation/distribution acceptance. No remaining work is omitted by renumbering.

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

## STEP41 impact: preserve diagnostic and protocol separation

The owned command reader recognizes exact protocol prefixes before handing its
bounded output to the separator adapter. The adapter separately compacts diagnostic
text. Previously that compaction removed leading whitespace from an indented
marker and could manufacture a valid protocol line. A dedicated CUDA-failure exit
could consequently trigger an unjustified CPU retry; execution, encoder and
BS-input metadata had the same normalization gap.

Only diagnostic text whose compacted form begins with a reserved prefix receives
a stable `Diagnostic: ` label, within the existing 1000-character diagnostic bound.
Original exact receipt extraction, duplicate/oversized rejection, genuine same-model
CPU fallback and reader/tree settlement retain their existing paths. Ordinary
diagnostics retain their existing formatting. No source, cache, provider-learning,
resource admission, inference geometry, numerical quality or realtime policy changes.

Regressions cover spaces, tabs, nonbreaking spaces, mixed exact and padded lines,
all four receipt roles and the existing diagnostic length bound. Tiny actual owned
children exercise both adapters' exit-70 decisions; exact markers still permit the
existing fallback and duplicate or padded markers do not. These are protocol and
adapter checks, with no actual model, CUDA-device or listening acceptance claim.
Both complete host manifests remain UNKNOWN at the unsupported Pydantic validator;
this correction does not certify generated schemas or compiled validator/serializer
connections. STEP42 and FINAL43 retain the entire former native and final remainder.

## STEP42 impact: bind actual generated host constructors

The host code policy previously checked AST-declared methods and dataclass
field/configuration metadata without checking the generated `__init__` that runs.
Replacing that function, its defaults or its frozen `object` closure could retain
a known component identity while changing source, request or limit construction.
In particular, a replacement `WorkerLimits.__init__` could omit the checked
`__post_init__` and accept negative limits.

For the supported direct-object, plain-field dataclasses, the policy now builds a
stdlib `make_dataclass` reference from held field order/defaults, boolean options
and source-declared hook presence. Reference generation uses trusted module globals,
inert object annotations and an inert hook; it does not execute host source, a host
constructor or a hook. Actual function/descriptor binding, structural code,
qualified name, generated filename/module, positional/keyword defaults and exact
closure bindings must match. Exact builtin defaults are checked recursively; bool
is distinct from int, and equal builtin subclasses cannot substitute runtime
behavior. Factories, custom field metadata, altered init/keyword policy and
inheritance remain unsupported. Source-defined constructors keep the ordinary
method proof; `init=False` without one requires exact `object.__init__`.

This closes a constructor-specific false-known component gap, including actual
`_Source`, `WorkerLimits` and `StemGenerationRequest`. It does not certify generated
equality/hash/frozen setters, annotations, metaclasses, `__new__` or external runtime
policy. Both complete original host manifests remain UNKNOWN at the unsupported
Pydantic execution validator; its actual compiled validator/serializer and complete
dependency lineage remain open. All eight existing component roles remain required.
No admission, source/content/OwnACK, last-reader, realtime, quality, model, dependency
or installation behavior changes. Productive Python/Torch paths remain active.

The old predicate reproduced18 failed regressions and7 passing controls in4.72s,
including the actual WorkerLimits bounds bypass and three actual constructor
components. The initial focused implementation passed75 tests in54.33s. Frozen
systems review then found an additional equal builtin-subclass default gap; the
constructor-only canonicalizer and direct/nested positional/keyword regressions
address it. All81 product regressions then passed, retaining the eight-role component positive
and both complete original host UNKNOWN tests. Formatting, one static constructor
access typing error and six test/type lint findings were corrected; subsequent
31 constructor regressions plus strict Mypy/spec checks passed. One later lint
finding in the test fake was corrected by retaining the captured canonical setter
as its explicit delegate; the actual negative construction/UNKNOWN/restoration
assertions remain unchanged. Final affected formatting/lint/index and independent
review follow this frozen source. These are scoped controls, not a whole-system,
model, device, hearing, performance or installed-footprint acceptance claim.

This justified split grows the plan to44. NativeREST43 retains ENTIRE former
nativeREST42 and FINAL44 retains ENTIRE former FINAL43, including every original
process/numerical/CUDA/Demucs/platform/distribution/negative/UNKNOWN gate. No new
neural execution or unchanged failed CUDA retry is authorized by this policy fix.

## STEP43 impact: preserve exact builtin types in host value evidence

Ordinary source-declared functions and methods previously compared default
evidence using Python equality. An unchanged `classify(value=1)` returning
`value is True` therefore stayed known after its live default became `True`,
although its result changed. The same gap affected nested collections, keyword
defaults, source constructors, constants and retained callable defaults. The old
predicate reproduced all35 initial regressions in7.44s with held source and
function bodies unchanged.

The shared value encoder now tags exact string, boolean and integer types,
retains float hexadecimal values and recursively measures exact builtin bytes,
slice and collection values. Subclasses cannot run equality, iteration or hex
hooks during this encoding. Positional/keyword default containers and keyword
names are checked before truthiness, length, key comparison or lookup. Guarded
callable defaults still use their own source/declaration/body/default proof;
supported mutable builtin defaults and the separate state-independent runtime
carrier policy remain available. Existing generated-constructor controls share
the stricter primitive encoding without broadening their field policy.

This is a type/value evidence correction, not a complete behavioral equivalence
claim: dictionary insertion order, alias topology, other generated methods,
metaclasses and external compiled execution retain their separate limits. Both
complete original host manifests remain UNKNOWN at the original Pydantic
validator; actual compiled validator/serializer and dependency lineage remain
open. All eight component roles remain required. No model, inference, admission,
source/content/OwnACK, last-reader, realtime, dependency or installer path changes.
Productive Python/Torch paths stay active; no new neural or CUDA run is justified.

This justified split grows the plan to45. NativeREST44 retains ENTIRE former
nativeREST43; FINAL45 retains ENTIRE former FINAL44, including all original
process/numerical/CUDA/Demucs/platform/distribution and negative/UNKNOWN gates.

The initial integrated run passed113 product regressions and3 static checks
in75.99s. Independent final review then found that type-set membership could
invoke a custom metaclass's hash/equality and let a builtin subclass impersonate
an exact builtin. All4 narrow function/generated-constructor regressions reproduced
that false-known result in1.02s. Both value encoders now select builtin types by
identity comparisons only, before any subclass or metaclass comparison hook.
Final affected regressions and checks follow this correction; the initial passing
run alone does not certify the corrected final source.

The corrected frozen source passed81 affected product regressions and3 static
checks in26.98s: all41 new cases,31 constructor controls and9 native conversion
controls, including all eight positive roles and both original fullhost UNKNOWNs.
The42 unchanged ordinary/origin cases from the initial run remain compatible.
This is a123-product scoped passing union, not one123-test run or a complete
system matrix. Ruff/format, strict Mypy and strict OpenSpec passed; final staged
byte/whitespace/protected-bridge checks and the same independent review chain
complete the publication boundary. The isolated Release/API retained22 known
Rust warnings and the unchanged einops0.6.1 warning. No neural, device, hearing,
whole-quality, performance or installed-footprint acceptance is claimed.

## STEP44 impact: preserve actual dictionary insertion order

Both ordinary value evidence and the supported generated-constructor encoder
sorted dictionary entries. Equal dictionaries with different insertion orders
therefore shared evidence, although `next(iter(value))` returned a different key
under unchanged held source and function code. The original predicate reproduced
all20 new negative cases in3.77s; both equal-order positive controls passed.

Both encoders now retain the entry sequence from exact builtin dictionaries,
recursively through supported nested values. Set/frozenset sorting, exact builtin
type gates, source-backed callable checks and state-independent mutable runtime
carriers keep their existing rules. Ordinary positional/keyword defaults, source
methods/constructors, constants, retained callables and tuple-wrapped dictionary
defaults in generated constructors cover the affected paths. Dictionary evidence
remains an entry list, so surrounding JSON metadata key sorting preserves order.

This tightens value evidence without certifying alias topology, other generated
methods, metaclasses, `__new__` or external compiled plans. Both original complete
host manifests remain UNKNOWN; actual Pydantic compiled validator/serializer and
dependency lineage are still open. No model/admission/queue/cancellation/reader/
source/content/OwnACK/FinalCAS/clock/realtime/dependency/installer path changes.
Productive Python/Torch remains active; no neural, CUDA or device run is justified.

This justified split grows the plan to46. NativeREST45 retains ENTIRE former
nativeREST44; FINAL46 retains ENTIRE former FINAL45, including all original
process/numerical/CUDA/Demucs/platform/distribution and negative/UNKNOWN gates.

The frozen integrated run passed145 product regressions and3 static checks
in85.86s, including all22 new order cases, the existing exact-type/constructor/
origin/source checks, all eight positive component roles and both original
complete-host UNKNOWNs. Ruff format/lint, strict Mypy and strict OpenSpec passed;
one initial test-only Yoda-condition lint finding was corrected without a
suppression. Final staged byte/protected-bridge checks and one independent actual
diff review complete publication. The isolated Release/API retained22 known Rust
warnings and the unchanged einops0.6.1 warning. This is focused evidence, with no
neural, device, hearing, full-quality, performance or installed-footprint claim.


## STEP45 impact: preserve bounded mutable owner sharing

The unchanged predicate reproduced four precise failures in2.54s: positional,
keyword-only, cross-area and nested list defaults changed identity behavior
under identical held source bytes and function code, while value evidence stayed
known. Systems and Methods PRE approved this bounded correction.

One graph compares mutable list/dict/set owners across each function's positional
and keyword defaults, across supported generated-constructor defaults, and within
each measured constant. Exact tuple/list/dict/slice paths record repeated owners
as deterministic path pairs; raw object IDs remain temporary traversal keys.
Cycles, more than4096 visited nodes or path depth above64 remain UNKNOWN before
these roots reach their recursive value encoders. Existing exact type gates,
dictionary entry order, canonical sets, source-backed callable defaults, retained
aliases/contextmanagers and state-independent runtime carriers keep their checks.
Fresh owners preserving the declared sharing graph retain the same evidence.

This is mutable-owner topology within those roots, not arbitrary identity.
Immutable scalar/tuple identity, cross-function/cross-constant/global-default
relationships and other generated methods/metaclasses/new retain separate limits.
Both original complete host manifests remain UNKNOWN at the Pydantic compiled
validator/serializer and dependency-lineage boundary. Productive Python/Torch
continues. No queue/admission/model math/source/OwnACK/reader/clock/realtime,
dependency or installer path changes; no NN/CUDA/App/device/listening/latency/
footprint acceptance follows. NativeREST46 retains ENTIRE former nativeREST45;
FINAL47 retains ENTIRE former FINAL46, including all negative/UNKNOWN gates.

Focused regression covers merge and split of lists/dicts/sets, nested containers,
slices, source methods/constructors, generated defaults, retained callables,
equivalent replacement owners and graph refusal. All existing processing/type/
order/origin/constructor/native-conversion cases, eight positive roles and both
original complete-host UNKNOWN controls remain required. Static checks cover
changed format/Ruff, strict Mypy, strict OpenSpec and exact staged/protected bytes.
One initial real C901/E501 lint pair was corrected by extracting children traversal
and wrapping the test line, without suppression. Frozen Systems POST and one
independent final actual-diff review precede checked English publication.

Frozen Systems POST found that dictionary child collection was eager before the
node guard. Child traversal now uses an exact-builtin lazy iterator, so refusal
does not first materialize the complete dictionary; a large-dictionary UNKNOWN
control joins the bounded graph cases. This correction precedes integrated tests.

The same final review chain also identified generated field/reference defaults
reaching recursive encoding before graph preflight. Field defaults now validate
topology before value encoding; the shared reference field set is preflighted
before constructor-default encoding. A generated-field refusal control makes
recursive encoders fail if any unbounded graph reaches them.

The graph walk counts repeated subtree occurrences as well as unique owners,
bounding later recursive value expansion for acyclic shared DAGs. A depth12 DAG
control requires refusal before recursive generated-field encoding. Exact default
container arity also refuses a guaranteed oversized graph before keyword sorting.

The integrated run passed all187 product cases (42 new plus145 existing), strict
Mypy and strict OpenSpec in102.34s; Ruff found one unused test-local assignment.
Removing that unused binding preserves the executed load and every assertion.
Final verification reruns the five affected generated-field refusal cases plus
all four static/index checks; other product results remain source-compatible.
The isolated Release/API setup retained22 known Rust warnings and unchanged
einops0.6.1; no neural/device/whole-quality or installed-footprint claim follows.

## STEP46 impact: retain Beat custody before output setup

Two precise failures reproduced on unchanged source in0.70s. A real isolated
Beat child already existed when output-thread construction refused or stdout was
absent; each exception escaped with retirement unset, the child still alive and
its temporary request retained. The private red probe explicitly retired only its
own fixtures afterward and proved the common FIFO could be reacquired.

The Beat path now creates its existing process owner immediately after child
creation. Stdout validation, reader construction/attachment, child activation,
thread start and supervision run inside its guarded setup. The finish path runs
in finally with a default supervision-failure reason, including when an unexpected
allocation exception continues to propagate. This reuses the existing owned
command and asynchronous retirement components rather than adding a new owner,
queue or cleanup deadline. CPU and CUDA Beat adapters share the corrected path.

Frozen verification covers constructor RuntimeError/OSError/MemoryError, missing
stdout, an unexpected monitor error after reader start, and delayed temporary
cleanup under both returned failure and propagated exception. The latter retains
the original Beat slots and common Native FIFO borrow until actual tree/reader/
temporary retirement, then proves a fresh successful request. Original source
PCM remains unchanged. Existing thread-start, command/tree/readers, cancellation,
heavy FIFO and stem-command regressions remain relevant. Systems PRE and Methods
PRE approved this focused correction; frozen Systems POST and one independent
actual-diff review precede publication.

No numerical kernels, model/provider choices, memory floors, source/content/
OwnACK/FinalCAS, playback eligibility, timing, callback, dependencies or installer
change. The fresh isolated Release/API setup retained22 known Rust warnings and
the einops0.6.1 warning; matching evidence can be reused after the Python-only
correction. No neural, CUDA, App/device/listening, latency, footprint or whole-
quality acceptance follows. This practical split grows the plan to48:
nativeREST47 retains ENTIRE former nativeREST46 and FINAL48 retains ENTIRE former
FINAL47, including all original negative/UNKNOWN gates and both complete-host
Pydantic compiled-plan/dependency boundaries.

The frozen integrated run passed95 functional checks and3 static checks in27.61s;
strict Mypy found three test references to an imported, non-reexported ticket
type, including one existing annotation. The test now imports the same Native
type directly, preserving its class identity and assertions. Initial Ruff findings
were corrected by extracting the guarded stdout/reader helper and test callbacks,
assigning exception text and using an explicit raw regex; no suppression was added.
Final verification reruns the affected Beat module, all four static checks and
exact staged bytes. Other successful command/heavy/stem/probe results remain
compatible with this test-import and documentation-only correction.

## STEP47 impact: reject queued work after worker-start refusal

Three precise regressions failed on unchanged product source in0.45s with the
real CPython3.14.2 ThreadPoolExecutor. Submission queues its WorkItem before
adjusting the worker count; Thread.start can raise RuntimeError after enqueue.
The old stem reservation returned its permit and reported rejection while its
queued callable remained retained and could later run. The Pair case actually
called prepare_stem_pair after ordinary caller cleanup had already released its
source and retirement owners. Controller generation cleanup shares this pool seam.

The existing reservation now stores the callable and decides acceptance under
its existing lock. Executor work retains only that reservation. A RuntimeError
clears the callable and returns capacity once before unlocking and rethrowing;
a worker takes the same lock and either skips rejection or takes the accepted
callable. Target execution remains outside the lock, with ordinary final release.
There is no second executor, admission queue, registry, Event or lifecycle owner.
An inert wrapper can remain in the executor until drain after rejection; repeated
startup failures do not acquire a new raw-wrapper queue bound. The fix is bounded
to RuntimeError submission failure, not arbitrary Python allocation/exception closure.

Systems PRE and Methods PRE approved the actual caller/stdlib interaction and
the existing-carrier handshake. Verification uses real enqueue/start refusal with
zero workers and one busy worker, a forced existing-worker dequeue before refusal,
weakref callable retirement before drain, exact Pair source/retirement/heavy-work
cleanup with a fresh Native FIFO successor, no late Native reads/results, all34
permits, and accepted target closure outside the lock until actual return. Existing
reservation, shutdown, target-failure and admission regressions remain relevant.
Frozen Systems POST and one independent final actual-diff review precede publication.

The new isolated writer required its normal environment/Release/API setup; further
checks reuse its matching Native. No Rust, model, CUDA, numerical, resource-floor,
playback, OwnACK, source-publication, clock, RT, dependency or installer change is
needed. Unchanged neural/device/latency/footprint matrices add no evidence for this
Python rejection seam and are not repeated. All prior negative/UNKNOWN gates stay
open. This coherent split grows the plan to49: nativeREST48 retains ENTIRE former
nativeREST47 and FINAL49 retains ENTIRE former FINAL48, without omissions.

The integrated run passed48 functional cases and3 static checks in11.16s; the
remaining Ruff check found formatting in the two changed test files. Parenthesized
context-manager layout corrects this without changing test bodies or assertions.
Final verification repeats the five affected lifecycle cases and all five static/
staged checks, reusing the other43 source-compatible functional successes. The
isolated Release/API retains22 known Rust warnings and unchanged einops0.6.1.

## STEP48 impact: settle failed analysis admission before work begins

Controlled RuntimeError injection at the documented Native ticket-construction
and acquisition boundaries failed both direct regressions on unchanged product
source in0.22s. All four CPU/CUDA bridge cases then timed out in8.31s: run_process
had cleared its retirement event before admission, but no child or reader existed
to set it. Each RED fixture explicitly released only its own event and joined its
thread afterward. This reproduces the Python failure-handling gap, not an observed
OS allocation failure. The normal productive Native path already borrows its active
whole-job owner; constructor queue-allocation refusal mainly concerns independent
callers, while the Native acquisition API also exposes gate-poison refusal.

Only RuntimeErrors from Native ticket construction and try_acquire become a
distinct admission error before the scope yields. Existing ticket cleanup still
withdraws the failed request. run_process settles only that invocation as a terminal
supervision failure, allowing either protocol bridge to return. Callback and
payload exceptions preserve their original identity; no broad handler grants
retirement after live work. The diagnostic AnalysisJob reuses its existing failed
PCM retirement/finish path before bridge, temporary storage or either model branch
starts. A Native retirement refusal remains nonterminal; no cancellation is invented.

Systems PRE and Methods PRE approved this bounded distinction. Frozen verification
covers both error boundaries and protocol bridges, actual Native FIFO successors,
original Beat slots, callback/payload exception identity, diagnostic failure and
refused retirement, plus compatible delayed tree/reader/temporary cleanup controls.
Frozen Systems POST and one independent actual-diff review precede publication.
The isolated Release/API setup retains22 known Rust warnings and einops0.6.1;
matching Native can be reused for this Python-only correction.

No numerical, model/provider, memory-floor, playback, source/content/OwnACK,
last-reader, clock, RT, dependency or installer behavior changes. No model/device
execution, CUDA fallback, latency, footprint or whole-quality acceptance follows.
This coherent split grows the plan to50: nativeREST49 retains ENTIRE former
nativeREST48 and FINAL50 retains ENTIRE former FINAL49, including every previous
negative/UNKNOWN gate and both complete-host compiled-plan/dependency boundaries.

The frozen integrated run passed all130 functional cases and four static checks
in20.21s: strict Mypy on both changed product modules, strict OpenSpec, exact
authoritative/staged source and protected-file checks, and exact eight-file index.
Ruff format/lint found one103-character test-fixture write call; splitting only
that call preserves its generated bytes and every assertion. Final verification
repeats the four affected protocol-bridge cases and all six static checks. The
other126 functional successes remain source-compatible; this is a successful-case
union, not a claim that the initial136-case run was entirely green.

## STEP49 impact: settle rejected waiting stem admission

Five controlled Native constructor/acquisition refusal cases failed in1.25s on
unchanged product code. Both the real shared-pool admission poll and the legacy
continuation let RuntimeError escape, leaving the resolved original waiting
without a scheduled backend or terminal delivery. The shared current/deferred
case reached the same defect. Injection exercises the documented Native API
error boundary, not an observed OS allocation failure or general OOM closure.

A common waiting-inference helper now removes the definitively failed attempt
and dispatches the existing settled terminal event. Existing source/content/
intent and deferred Pair delivery decide which subscriber can receive it.
Deferred source, producer and Native ticket custody survives until ordinary
unfenced delivery and job disposal. There is no eager broad cancellation or
finish_read for a backend that never began. Normal contention still defers.
Exact registry and waiting membership are revalidated before acquisition because
terminal delivery can reenter the legacy loop with an older outer snapshot.

Systems PRE and Methods PRE selected this existing event route instead of broad
submission-failure cleanup. Focused checks cover four constructor/acquisition
by shared-pool/legacy cases, mixed stale/deferred/current recipients, real Native
FIFO retirement, all34 unused worker permits, and a distinct legacy successor
scheduled once. Compatible cancellation, playback, Pair, sharing, pressure and
six-lane admission controls remain relevant. A genuine held identity delivery is
not claimed by the mixed terminal test. Frozen Systems POST and one independent
final actual-diff review precede publication.

The isolated environment/Release/API setup is required once for this writer;
matching Native is reused afterward. No Rust, model, device, numerical, resource
floor, playback policy, source publication, OwnACK, clock, RT, dependency or
installer change is needed. All original failed/UNKNOWN and quality/distribution
gates remain open. This coherent split grows the plan to51: nativeREST50 retains
ENTIRE former nativeREST49 and FINAL51 retains ENTIRE former FINAL50, without
omissions. Productive Python/Torch paths remain active.

The integrated run passed88 functional cases and four static checks in21.27s.
Two older fanout cases failed because their setup directly called the unchanged
low-level scheduler without Native heavy admission. That helper bypasses both
changed continuation methods; its omission is source-backed, not a separately
executed parent-baseline result. Setup now uses the common waiting-admission
helper, preserving every artifact, source,216-row, capacity and custody assertion.
Ruff found an import layout, a long test body and a combined assertion; a test
helper preserves those assertions. The OpenSpec requirement puts SHALL on its
first content line for the validator. Final verification repeats the eight
affected functional cases and six static checks; the other82 functional successes
remain source-compatible. This is a successful-case union, not one green96 run.

## STEP50 impact: yield resource-ineligible stem FIFO positions

The normal controller poll could register a Native heavy ticket under clear
headroom, then stop polling it when the existing resource guard became pressured
or unavailable. That inactive position still preceded otherwise admitted analysis
work after the active owner retired. Six corrected tests reproduced this actual
FIFO starvation on unchanged product source in1.51s; two active-borrow controls
passed. An earlier eight-failure fixture run is not this behavior proof: it
constructed successors before the stem ticket and incorrectly expected a settled
backend to retain its already finished source read. Corrected construction order
and settled-state assertions preserve the real FIFO and custody contracts.

Resource denial now uses existing `withdraw_waiting()` on waiting inference,
requested preparation and successful/reuse delivery carriers. It releases only
inactive FIFO positions, without changing request registries, source/interests,
producer files, failure/cancel state or active ticket borrows. Failed deferred
terminal deliveries retain their separate custody. Recovery uses the same owners
and unchanged thresholds, joining behind existing waiters; no second queue,
scheduler, resource estimate, Native policy or realtime work is introduced.

Focused checks cover both denial states and all three carrier types, original
metadata/custody, inclusive recovery hysteresis, real Native successor order and
an active completed-generation borrow through ordinary shutdown. Existing shared
pool handoff, pending/admission/retirement, playback, stale, cancellation and
failed deferred delivery controls remain relevant. Matching Native is reused
after the new isolated checkout's initial environment/Release/API setup. No
numerical, model, CUDA, memory-floor, clock, OwnACK, dependency or installer change
is made; all original fullhost/native/quality/distribution gates remain open.

The frozen integrated run passed87 affected functional cases, including all nine
new controls, and five static/source checks in24.52s (92PASS, one index check
deselected until staging). Ruff format/lint on both Python files, strict production
Mypy, strict OpenSpec and exact authoritative/staged source plus protected private
bindings passed. Systems PRE/frozen POST and Methods PRE/failure assessment were
clear. These are controlled policy/FIFO results, not observed OS allocation
pressure, numerical/provider parity, installed savings or whole-system acceptance.

## STEP51 impact: retire cancelled queued preparation owners

Two regressions failed on unchanged production source in10.27s. Strict terminal
delivery observed cancellation but kept its operation, source captures, cold
reservation and heavy ticket queued until an unrelated heavy predecessor or two
occupied workers finished. Cleanup then allowed the already-cancelled callable
to enter. The productive Pair preparation caller supplies existing source and
shutdown cancellation; it needs actual captured-owner retirement before return.

The strict waiter now retains only the unique existing reservation retirement
receipt as queue identity. After its first terminal error it removes exactly its
own still-queued entry under the existing mutex, then synchronously drops that
entry outside the mutex. Existing operation/reservation/heavy Drop order remains
intact. A worker claim wins under the same mutex; an absent entry leaves actual
result/disconnection waiting unchanged. No new queue, cancellation registry,
worker, source predicate under lock, memory policy or realtime work is introduced.

Systems and Methods PRE chose this bounded existing-owner route. A real Native
heavy blocker and held capture destructor test prove retirement begins before
predecessor end, the queue mutex remains available, and caller/FIFO successor
cannot escape unfinished capture destruction. Two occupied workers and an
independent queued peer cover precise removal. Existing active cancellation,
poison, shutdown, same-owner continuation and Pair source/OwnACK/read controls
remain relevant. Frozen Systems POST and one independent final actual-diff
review precede publication. Matching Native is reused after required builds.

The separate asynchronous start_analysis queue has no strict receiving waiter
and retains its existing cancellation limit. Optional diagnostic Python bridge
startup robustness is also separate. This step does not claim either corrected,
or close native process/fullhost/model/CUDA/quality/platform/distribution gates.
Productive Python/Torch, all floors/caps, source publication, clock and RT remain.
The substantive split grows the plan to53: nativeREST52 retains ENTIRE former
nativeREST51 and FINAL53 retains ENTIRE former FINAL52 without omissions.

The first integrated Native selection passed21 and failed3 in2.96s. Two component
fixtures ran their nested heavy Pair operation inside an ordinary cold owner;
their outer setup now uses the existing heavy terminal-waiting entry without
changing any read, PCM, cancellation, registration or source assertions. The
third failure exposed a real public-path mismatch: PreparedStemsWork::prepare
also used ordinary admission before calling the heavy Pair kernel. Its public
cold/legacy borrow test remains unchanged. That production outer call now enters
one heavy terminal-waiting owner for both branches; nested Pair work reuses it
inline and legacy transient source/component work shares the same FIFO custody.
The early rejection guard and those callers matched parent source exactly;
this is source-backed parent equivalence, not a separately executed baseline.
Systems and Methods separately approved this coherent correction and the
unchanged public receiver, render, OwnACK and actual-reader assertions.

The final frozen Native selection passed26 in4.35s, including the unchanged
public cold and legacy variants, both component cases and retained-result source/
Pair controls. Python passed59 affected functional cases and three static checks
in6.90s. Format, strict OpenSpec, exact staged-source/protected bindings and the
final exact-index check form the publication checks. Only this result paragraph
and the completed task state change afterward; all Rust and Python inputs retain
their checked bytes. These separate runs are not one green combined matrix and
do not close the original native/CUDA/fullhost/quality/distribution gates.

## STEP52 impact: settle analysis bridge construction failures

The optional diagnostic supervisor constructed its cancellation Thread before
entering existing failure settlement. Controlled constructor RuntimeErrors escaped
the real supervisor without attempting Native PCM retirement or publishing a
terminal result. The independent Native heavy borrow then remained intentionally
held by that unfinished job, blocking a real FIFO successor. Both regressions
failed on unchanged production in0.35s; a refined observation run failed both in
0.33s and recorded the escaped error, absent retirement attempt and blocked actual
successor before fixture cleanup. These injections are not observed OS thread or
allocation failures and do not establish universal OOM handling.

Bridge construction now occurs inside the existing try, after admission-error
precedence. The same exception set, branch waits, Native PCM retirement, temporary
cleanup and Native finish remain authoritative. An optional bridge reference and
the existing started flag permit joins only for a bridge that actually started.
No new thread, queue, scheduler, cancellation registry, retry, broad exception
catch or fabricated completion is introduced. Refused Native retirement still
retains the same heavy borrow and unfinished diagnostic state.

Systems and Methods PRE selected this bounded correction before changing the
separate productive Native queue claim path. Focused checks cover constructor
failure with successful and refused retirement, original admission-error
precedence, cancellation before heavy admission, real Native FIFO progress and
unchanged delayed Beat/Key/PCM and finish-race controls. Tests observe the real
heavy successor before fixture-only leaked-borrow cleanup. Original Native reader
evidence remains compatible because no Rust, source admission, model, resource
policy, dependency, installer, clock, OwnACK or realtime behavior changes.

The asynchronous productive start_analysis queued-cancellation gap remains
source-identified OPEN and is not claimed reproduced or fixed. Native process
closure, both original fullhost UNKNOWN gates, compiled Pydantic lineage, actual
CUDA/same-model fresh CPU, numerical quality, platforms and distribution remain
open. This substantive supervision split grows the plan to54: nativeREST53
retains ENTIRE former nativeREST52 and FINAL54 retains ENTIRE former FINAL53.

The frozen integrated check passed56 functional cases and four static checks in
11.20s (60PASS, one exact-index check deferred until staging). The actual Jobs
and Heavywork suites include all three new constructor/cancellation controls,
both strengthened admission-refusal cases and real Native FIFO/last-borrow
controls. Ruff lint/format, strict production Mypy, strict OpenSpec and exact
authoritative/staged source plus protected private bindings passed. Systems PRE
and frozen POST were clear; one independent final actual-diff review precedes
publication. No device fixture, model inference or new Rust test was run. Initial
isolated runtime setup built unchanged Release Rust once in61s with22 known
warnings; matching Native/API was reused afterward. These are controlled Python
supervision and actual Native admission results, not actual Native PCM failure,
OS pressure, universal allocation failure, model/CUDA or whole-quality acceptance.

## STEP53 impact: retire cancelled queued asynchronous analysis

The productive Native start_analysis_with path had no receiving waiter to retire
an invalidated queued request. Its shutdown and original source/request checks
ran only after heavy claim. Four real LaunchFixture regressions failed against
unchanged production in2.11s: request, Native source revocation and shutdown kept
one queued admission and three task-token references behind a live heavy owner;
the occupied-worker variant retained two active ordinary jobs plus that queued
analysis. Each case recorded PCM/task/payload state before releasing all fixture
owners. Source revocation here exercises the real Native primitive, not UI unload.

The same queue now optionally retains a cancellation predicate for this async
heavy path. Existing workers inspect it between eligibility waits; the existing
loader control poll performs the same bounded inspection detached from Python.
Predicates capture only the original NativeAnalysisOwner and shutdown flag, not
another PCM reader or task guard. They use original source-cell/lifetime and
request epoch authority; pending Pair mirrors and vacated numeric slots are not
cancellation. No per-request thread, registry, queue extraction or scheduler is
introduced. Both occupied workers require the serviced control pump for prompt
queued retirement; no general latency or unserviced-pump guarantee is implied.

An inspection borrows each observed job's SAME cold reservation and SAME heavy
ticket while its predicate runs outside the queue mutex. The original remains
visible to worker claims and existing strict terminal waiters. Cancellation
removes only its exact queued receipt; a raced worker claim stays terminal-owned.
Callable captures and predicate owners end before admission release; observation
owners also end before their reservation and heavy borrows. This prevents a
snapshot from retaining hidden owners beyond the complete original admission.
Focused controls hold actual predicate/destructor and claimed-payload boundaries,
exercise lane reentry and ordinary progress, and retain existing Pair, source,
task-token and active terminal cancellation assertions.

Systems and Methods PRE approved this stable-queue design after rejecting queue
extraction's ordering and strict-waiter receipt races. Frozen review and affected
checks precede publication. This substantive practical split grows the plan to55:
nativeREST54 retains ENTIRE former nativeREST53 and FINAL55 retains ENTIRE former
FINAL54. Productive Python/Torch, complete native process and fullhost UNKNOWN,
compiled Pydantic lineage, original numerical/CUDA/fresh-CPU, platform, quality,
distribution and all earlier negative/UNKNOWN gates remain unchanged.

The frozen source passed49 distinct Native cases in separate selections:
18 ColdJobs,14 analysis-owner,3 source-custody,2 prepared-content and12 public
background-borrow/stream-shutdown/restore controls. One initially empty borrow
selector was corrected to its actual nested module and is not counted as proof.
The first integrated compile found four existing direct-helper fixtures missing
the new optional argument; adding None preserved their original assertions.
Final Release/API checks passed with20 library warnings and34 Looper test warnings;
the compatible Native was reused afterward. Python Jobs and Heavywork passed56
functional cases plus3 static checks in2.72s: Rustfmt on the four substantive
Rust files, strict OpenSpec and exact LF stage/source plus protected bindings.
The reviewed two-line mod.rs insertion leaves unrelated inherited formatting
intact. The exact final-index check remains mandatory before publication.
These separate successful selections are not a green whole-system matrix.
Only task completion and this result paragraph change after functional checks;
all five Rust inputs retain their tested bytes. No model/device/CUDA, physical
pressure, latency, footprint or whole-quality acceptance is inferred.

## STEP54 impact: retire cancelled queued asynchronous loads

Actual ColdLoad admission retained initial-source PCM, loading state and its cold
reservation after cancellation while waiting behind unrelated heavy work or two
occupied ordinary workers. Six regressions failed against unchanged production
in3.19s: request, epoch, actual Native placement replacement, resident cancellation,
shutdown and ordinary shutdown. Every failure recorded the live blocker boundary
before cleanup: PCM3/2 owners, loading8, adoption0, unsettled cancellation and no
terminal error; the queued counts were0/1/1 or2/1/3. The never-opened source path
ensures this is pre-preparation cancellation, not a decoder/model failure.

The same bounded queue now carries lightweight unarmed authority predicates for
ordinary and analysis-bearing loads. It retains original base request/epoch,
proposal, definitive placement, resident and shutdown fences. Pending placement
alone stays live. The predicate first takes the original request mutex to preserve
the admission commit fence, even when shutdown is already visible. It clones no
PCM, prepared material or artifact pin. Exact receipt removal and all existing
claimed-work/strict-waiter visibility and admission borrowing remain unchanged.

An optional queued-retirement envelope becomes executable only after queue
acceptance. It owns the unclaimed original callable, waits for and releases that
same request commit lock, destroys original captured owners and LoadingGuard, then
uses existing cancellation cleanup and terminal Error/Progress delivery. Waiting
before destruction also closes accepted queue shutdown racing metadata commit.
Rejected submissions drop the unused envelope without invoking it under caller
locks. Taking the original callable for actual worker execution disarms queued
cleanup. All envelope and predicate captures end before the same original cold
reservation and heavy capability. Terminal delivery is not a resource-END receipt.

Systems and Methods PRE approved this focused approach. Verification covers the
real PyO3 control pump/filter, replacement loading-token preservation, rejection
under a held commit lock, accepted close/commit ordering, claimed payloads and a
valid queued Pending source through its real own ACK, plus affected original cold
and load controls. Source/policy/OwnACK, active cancellation, original numerical,
resource, realtime and quality authority stay intact. This substantive split grows
the plan to56: nativeREST55 and FINAL56 retain their ENTIRE former54/55 duties and
all inherited negative/UNKNOWN gates. Other asynchronous classes remain separate;
productive Torch and all process/fullhost/numerical/CUDA/platform/distribution
acceptance limits remain unchanged. Frozen verification and review precede publication.

Focused verification passed47 distinct Native cases:8 queued-load cases,21
ColdJobs cases,14 analysis-owner cases,3 source-custody cases and1 actual shutdown.
The eight load cases retain their tested production/test bytes; only the existing
shutdown fixture then changed its obsolete no-error expectation to exactly one
queued Error and no Started. All earlier source, voice, guard and artifact checks
remain intact. The final shutdown and common controls passed;26 selected Python
loader regressions passed in1.86s. Final Release/API verification passed with20
library warnings;35 Looper test warnings and the einops0.6.1 warning remain.

An unintentionally broad nested cold_load selection ran309 cases:287 passed,
9 failed and13 remained ignored in1022.87s. It is not a green matrix and is not
repeated. Besides the updated shutdown expectation, eight failures were checked
individually with unchanged assertions against original production at dcea080f.
An earlier comparison was invalidated when its shared Cargo target reused the
current Rust runner. The corrected comparison freshly compiled the retained
original source into an unused exclusive target, verified its2835-test inventory,
old child Python source and an actual old Native mapped-image witness. This
parent stage contains the six initial, unselected RED tests; its production and
selected original fixtures match the retained commit.

Six failed leaves/assertions occur in both versions: a missing
configure_automatic_bpm_range hook on the ResumeAudio test double, the manual
analysis supersession TaskError expectation, three public Generate/Piano waits
and the discard queue's first-files retirement wait. Their differing asynchronous
diagnostic snapshots remain explicit; matching assertions do not establish
matching causes. Both216 registry cases pass in fresh processes while their
broad-run registry failures remain unexplained. No timing or nondeterminism claim
is drawn from the invalidated comparisons. An unchanged collector snapshot-owner
lifetime is a source-only investigation candidate; no collector fix or controlled
reproduction is included here.

These additional negative observations remain open alongside every inherited
quality and final gate. They do not replace the original32 parent failures,
distinct watchdog, registry or other UNKNOWN records. Frozen Systems and Methods
reviews cleared the scoped load-retirement change. Final format, strict OpenSpec,
source/stage/index and protected-binding checks and the single independent final
actual-diff review precede its publication. No whole-model, CUDA, device, listening,
latency, footprint or whole-system acceptance follows from these selections.

## STEP55 impact: end asset observation pins before unlocking

Two deterministic original/stem regressions failed against unchanged collection
logic in0.02s. The first actual sweep snapshots a live application pin and pauses
immediately after its actual asset-gate unlock. After the application releases its
real pin, a second sweep still sees the first observer's strong PathOwner: one
pending retirement, zero deletions and the file still present before test teardown.
The test-only thread-local gate owns synchronization only; its Weak probe never
creates another owner. Both collectors end before captured facts are asserted.

The existing collector now destroys its temporary owners Vec before unlocking the
same admission/deletion gate. PathOwner contains paths, immutable file identity
and an atomic saved-assignment flag; it has no custom destructor, resource close,
callback or service reentry. The early no-retirement return already drops locals
before the mutex guard. Reader destruction, saved claims, identity checks, eight
I/O attempts, fairness, queue bounds and realtime behavior remain unchanged.

Production has one shared background collector. Overlap is reachable with its
existing manual test sweeps; this controlled defect does not establish ordinary
production starvation or explain the historical discard-saturation timeout.
The two new cases also retain real surviving-owner controls. Bounded existing
asset/PCM-reader/identity/sharing and Python-to-Native lifetime checks cover the
affected behavior. Systems PRE and Methods approved this focused experiment;
frozen Systems review and the one final actual-diff review precede publication.

This substantive lifetime split grows the plan to57: nativeREST56 retains ENTIRE
former nativeREST55, and FINAL57 retains ENTIRE former FINAL56. All six qualified
failed leaves, two broad-only registry failures and every inherited negative or
UNKNOWN gate remain open. Native process allowance, fullhost/Pydantic compiled
lineage, numerical/CUDA/fresh-CPU, quality, platform and distribution acceptance
remain unchanged. Productive Python/Torch remains active; no footprint saving,
model, device, listening or whole-system acceptance follows from this correction.

Frozen verification passed38 Native asset cases in9.69s, including both new
original/stem boundaries and their real surviving-owner controls. The existing
isolated Windows C3 resource profile remains ignored; it is not a passed plateau
or a closed historical216 gate. Nine Python-to-Native lifetime cases and four
static checks passed together in5.81s. Static checks cover both Rust files,
strict OpenSpec, exact LF source/stage bytes and the five protected local bridges,
private discovery pointer, common exclusion and hidden flags. Final Release/API
verification passed with20 existing library and35 test warnings; the einops0.6.1
warning remains. Only this result and task completion change after functionals;
both Rust inputs retain their tested bytes. Final index checks and the single
independent actual-staged-diff review remain required before publication.

## STEP56 impact: drain accepted queued ownership during poisoned shutdown

Two new regressions fail against unchanged production after controlled poisoning
of the actual cold queue mutex. Both existing workers are held in claimed ordinary
jobs before an asynchronous job is accepted. Actual submission borrows a local
active heavy capability; its caller then ends, leaving the queued job as its sole
active owner. This is not a queued job behind an independent heavy predecessor.

After the original close call, both cases still observe two claimed jobs, one
queued job, three admissions and stopped=false. The original run fails both cases
in5.01s. Weak capture observations never promote an owner. All observations precede
explicit fixture repair and actual worker joins, so the failing tests leave no
queued captures, heavy tickets or threads behind. This is a controlled internal
failure boundary, not evidence of an ordinary production panic, OOM or the cause
of any historical discard, watchdog or registry failure.

Permanent close now recovers a poisoned guard solely to mark the lane stopped and
take its actual queue. Accepted callables and terminal retirement callbacks are
destroyed outside the queue mutex through the unchanged AdmittedJob ordering.
This breaks the queued reservation/Shared ownership cycle even when no strict
receiving waiter exists. The existing strict poison-waiter cleanup remains intact.
New admission remains refused, poison is not cleared, and actual claimed kernels
keep their readers and reservations until terminal return. Worker scheduling,
cancellation predicates, source/request/OwnACK, queue bounds and realtime behavior
are unchanged.

The callback regression additionally holds terminal cleanup after actual callable
capture END. Its original cold reservation and heavy capability must remain held
until that callback ends; poisoned try_lock distinguishes an acquired guard from
WouldBlock to check lock freedom. Independent claimed reader owners remain alive
through close and end only when their workers are released and joined. Systems
and Methods PRE clear this scoped correction. Frozen Systems review, focused
ColdJobs and affected load controls, strict spec/format/source checks and the one
independent final actual-diff review precede publication.

This substantive failure-retirement split grows the plan to58. NativeREST57 retains
ENTIRE former nativeREST56 and FINAL58 retains ENTIRE former FINAL57. Productive
Python/Torch, installed saving ZERO, every inherited negative/UNKNOWN and all
native process, fullhost/Pydantic, numerical/CUDA/fresh-CPU, quality, platform and
distribution gates remain unchanged.

Frozen verification passes all23 selected ColdJobs cases in0.52s, after inspecting
their actual compiled inventory, and eight exact queued-load controls in0.50s.
These are31 distinct successful cases in two selections, not one green whole-system
matrix. The load controls retain original request, epoch, placement, resident and
shutdown cancellation, replacement loading tokens and a valid source's own ACK.
The actual current Release/API build passes in43.92s with20 existing library
warnings;35 Looper test warnings and the pinned einops0.6.1 warning remain. Only
this result paragraph and task completion change after functionals; both Rust
inputs keep their tested bytes. Final format, strict spec, source/stage/index and
protected-binding checks and the one independent final staged-diff review remain
required before publication. No model, CUDA, physical-pressure, device, listening,
latency, footprint or whole-quality acceptance follows from this correction.
