## ADDED Requirements

### Requirement: Windows Child Ownership Precedes Process Creation
Beat and shared offline Windows commands SHALL allocate and retain a native
process owner and their private pipe objects before creating a child. Successful
`CreateProcessW` output SHALL write directly into that owner's retained state,
without requiring a newly allocated Python process-result or handle wrapper.
Python constructor failure SHALL positively retire the exact suspended root before
releasing its original child reservation. Existing complete-tree and independent
reader-retirement obligations SHALL remain unchanged after activation.

The child SHALL remain suspended until the existing observer, Job assignment and
resume path admits it. Only explicit standard-handle duplicates SHALL be inherited;
parent writer copies SHALL close before output supervision begins. Argument
quoting, UTF-16 paths, separate stdout/stderr, environment spelling/deduplication
and timeout behavior SHALL retain the supported Windows semantics.

#### Scenario: Python publication fails after an actual suspended child starts
- **GIVEN** the native owner and pipe objects already exist and an actual suspended Windows child has been created
- **WHEN** a controlled Python error occurs before the constructor publishes its result
- **THEN** cleanup terminates the held root and observes its actual kernel-handle END before closing its original child reservation
- **AND** the child has performed no worker effects and a fresh reservation can be admitted afterward
- **AND** this test does not claim arbitrary continuous-OOM closure or an observed defect in an unproved CPython binary build

#### Scenario: Selective Windows launch preserves the public worker protocol
- **GIVEN** a command contains quoted and non-ASCII arguments, a private working directory and a supported Windows environment
- **WHEN** the existing owned-command supervisor activates its prepared child
- **THEN** stdin is DEVNULL, stdout/stderr remain independent and their EOF follows closure of the actual owned writers
- **AND** ordinary failure, cancellation, descendants, timeout and fresh retries retain the existing Job/tree/reader contracts

### Requirement: Recovery-Held Missing Originals Do Not Consume Restore Attempts
The Loader control poll SHALL attempt at most eight deferred original-source
restores per invocation. Current observer fences, stale path/content checks and
recovery-held missing-original checks SHALL precede the attempt budget. Skipped
owners SHALL NOT prevent independent later originals from progressing. Each
actual scheduling attempt, including a refused attempt, SHALL consume the budget.
The fixed-PAD snapshot order, queue-full stop, resident-pressure rotation and
original project/content/session/reader ownership SHALL remain unchanged.

#### Scenario: Recognized Bank recovery retains eight missing originals
- **GIVEN** a genuine interrupted Bank journal retains eight missing original assignments and seventeen valid originals exist outside its participant banks
- **WHEN** startup admits the first eight valid originals and subsequent Loader polls run
- **THEN** successive polls attempt eight and one remaining independent originals while retaining all missing assignments and their content, controls and deferred owners
- **AND** returning original files can resume through those same deferred owners without inventing a source ACK or settling the Bank journal
- **AND** failed Native admission attempts still consume the eight-attempt bound

### Requirement: Arrangement Fences Do Not Consume Disk Settlement Attempts
The stem control poll SHALL attempt at most eight retained disk-selection
settlements per invocation. It SHALL check each original owner's current
arrangement fence before attempting settlement; a fenced owner SHALL NOT consume
that attempt budget or prevent independent later owners from progressing. The
stable snapshot order, durable selection, source/content checks and existing
prepared-pair, lease and retirement authority SHALL remain unchanged. A fenced
owner SHALL remain retained until the original arrangement releases its fence.

#### Scenario: A Bank Copy fences eight retained source settlements
- **GIVEN** eight retained source-PAD disk settlements are fenced by an admitted Bank Copy and later independent settlements remain eligible
- **WHEN** the stem control poll runs before that Bank arrangement settles
- **THEN** it attempts up to eight independent settlements in their original order without consuming or releasing the fenced owners
- **AND** later polls preserve the same bound and allow the original owners to settle after their fence is released
- **AND** existing unsettled-target preflight refusal and Native acknowledgement obligations remain unchanged

### Requirement: Returned Offline Children Survive Supervisor Allocation Failure
The caller SHALL retain raw custody of a genuine returned Beat or shared-command
child when allocating its supervisor raises `MemoryError`. Synchronous custody
SHALL continue through payload and actual tree retirement, output-pipe closure
and resource cleanup. No reader has started at this boundary. Cleanup SHALL NOT
allocate a replacement supervisor or treat Job closure as tree-end proof.
The original exception SHALL propagate unchanged only after retirement. Beat
temporary storage, the real Native ticket borrow and both global permits SHALL
remain held through delayed cleanup. This bounded guarantee SHALL NOT certify
unknown partial effects inside child construction or continuous allocation failure.

#### Scenario: Supervisor allocation fails after a suspended child returns
- **GIVEN** the public Beat or shared-command path owns a genuine returned child
- **WHEN** allocating its supervisor raises a memory allocation error
- **THEN** the original child ends before its tree and output resources close
- **AND** delayed Beat temporary cleanup retains admission and the actual ticket borrow
- **AND** retirement precedes unchanged exception propagation and fresh successful execution

### Requirement: Prelaunch Beat Allocation Failure Retains Cleanup Ownership
Before child construction, a Beat request's allocation failure SHALL release its
unused global permits or retain them through actual temporary cleanup. An
acquired Native ticket borrow SHALL remain held until that cleanup ends. The
original allocation exception SHALL propagate unchanged. Failure to allocate the
cleanup thread SHALL use synchronous cleanup with the same ownership guards.
Worker arguments and the child-only environment SHALL be prepared within this
prechild cleanup boundary, before acquiring a child-memory reservation or
constructing a process. Unknown partial effects inside process construction
SHALL remain outside that no-child guarantee.

#### Scenario: Request preparation fails before child construction
- **GIVEN** Beat admission has acquired both global permits
- **WHEN** temporary construction returns no owner or request preparation raises a memory allocation error
- **THEN** no child is launched and the original exception propagates
- **AND** retirement and fresh admission require actual temporary and ticket-borrow cleanup
- **AND** a delayed cleanup retains both permits and the borrow until it completes, including when the cleanup thread cannot be allocated

#### Scenario: Child environment preparation fails before launch
- **GIVEN** request preparation retains both Beat permits, temporary storage and a real Native ticket borrow
- **WHEN** preparing the child-only environment raises a memory allocation error
- **THEN** neither child-memory reservation nor process construction begins
- **AND** actual delayed temporary and borrow cleanup precedes retirement and fresh admission
- **AND** the exact exception and immutable source bytes remain unchanged and fresh Beat execution succeeds

### Requirement: Shared Offline Workers Require Process Tree Ownership Before Launch
The shared Beat/stem worker launcher SHALL refuse execution on platforms without
an owning process-tree adapter before allocating a child or tree owner. Launcher
exit and closed output pipes SHALL NOT substitute for descendant retirement.
Refusal SHALL use existing startup-failure cleanup to settle unused admission,
temporary storage and ticket borrows. Windows and Linux ownership routes SHALL
retain their existing behavior. This interim refusal SHALL NOT establish or
remove the outstanding requirement for full macOS support and actual validation.

#### Scenario: The host has no owning offline process adapter
- **GIVEN** an offline command or Beat request on an unsupported host such as macOS
- **WHEN** its shared launcher attempts to start the worker
- **THEN** it refuses before creating a child, native memory budget or tree owner
- **AND** the unused request's temporary storage, ticket borrows and admission settle so a later request is not stranded

### Requirement: Linux Offline Workers Own Descendants Through Reader Retirement
Linux offline supervision SHALL bind any observer to the gated original payload
before execution. A private subreaper SHALL retain descendants independently of
payload exit, process groups and sessions, reap its complete private subtree and
acknowledge ECHILD before successful broker exit can certify tree retirement.
Final acknowledgement SHALL drain racing teardown before closing the proof channel.
Payload PID and returncode SHALL remain distinct from broker PID and exit.
Existing admission, temporary storage and source custody SHALL remain held until
both actual tree retirement and independent output-reader settlement. Lost or
invalid ownership proof SHALL remain nonterminal and SHALL NOT admit a successor.
The application SHALL NOT become a global subreaper or reap unrelated children.

#### Scenario: A successful payload leaves an escaped or silent descendant
- **GIVEN** a Linux worker has inherited-output or silent descendants, including a changed session or double fork
- **WHEN** its original payload exits, fails, times out or is cancelled
- **THEN** its private broker terminates and reaps the complete owned subtree while unrelated processes remain alive
- **AND** payload status, genuine tree end, broker exit and both output-reader ends remain independent requirements

#### Scenario: Observer admission fails before Linux payload activation
- **GIVEN** the broker retains a gated payload and its stable original identity
- **WHEN** observer admission fails or cancellation is observed before activation
- **THEN** no payload code executes and private teardown still precedes resource release
- **AND** an externally killed gated payload remains unreaped until its identity can no longer be bound to a new observer

#### Scenario: Linux ownership proof is lost
- **GIVEN** an admitted invocation still owns its original source and resources
- **WHEN** the private broker disappears or its protocol cannot prove complete tree retirement
- **THEN** root exit or output EOF cannot certify retirement or release the original admission
- **AND** closing private control requests cleanup from a still-owning broker without inventing a tree-end receipt
- **AND** this bounded Linux ownership contract does not establish whole Linux application admission or macOS support

### Requirement: Queued Material Migration Settles After Captured Owners End
Accepted unclaimed material preparation and stem-copy jobs SHALL observe their
original cancellation and shutdown flags in the existing cold lane. Queued
retirement and permanent lane closure SHALL destroy the actual kernel's captured
material, PCM and path pins before publishing a terminal preparation error.
Retirement SHALL run outside the queue and result mutexes, retain the original
reservation through cleanup, and SHALL NOT overwrite an existing result.
Claimed kernels SHALL retain their original cancellation, rollback and result
paths. Successful stem-copy results SHALL retain their `created` and
`cache_reference` evidence for the controller's existing exact-owned cleanup.

#### Scenario: Cancelled migration waits behind occupied workers
- **GIVEN** accepted material or stem-copy work and an unrelated queued operation
- **WHEN** the serviced control pump observes that migration's original cancellation
- **THEN** its still-queued captures end and its preparation becomes failed before either worker finishes
- **AND** the independent operation keeps its original queue position and execution
- **AND** the controller's existing terminal wait continues to precede rollback and hold/writer release

#### Scenario: Permanent close drains accepted migration work
- **GIVEN** an accepted migration kernel has not been claimed by a cold worker
- **WHEN** permanent close removes that queued callable
- **THEN** actual capture destruction precedes its terminal preparation error
- **AND** a completed successful stem generation retains its cleanup receipt after cancellation

### Requirement: Obsolete Queued Waveform Views Retire Before Worker Availability
Asynchronous waveform projection SHALL register its original request cancellation,
shutdown and source-incarnation authority with the existing cold lane. Existing
off-realtime cancellation observations SHALL retire only still-queued obsolete
views and their captured readers before returning their reservations. A queued
observation SHALL NOT clone the source PCM reader. Claimed work SHALL retain its
existing kernel cancellation, actual reader lifetime and exact result-publication
fences. Current views, unrelated queued work and valid content movement SHALL remain
eligible. Genuine queue exhaustion before cancellation is pumped SHALL retain its
existing visible failure and explicit-retry behavior.

#### Scenario: Zoom changes while both cold workers are occupied
- **GIVEN** an independent queued operation and multiple superseded waveform views
- **WHEN** the existing control pump observes their original cancellation flags
- **THEN** obsolete unclaimed views return their reservations before either worker finishes
- **AND** the latest view and independent operation retain their original ownership
- **AND** later waveform output retains its original absolute coordinates and PCM projection

#### Scenario: A queued view loses its original source
- **GIVEN** an accepted waveform view still holds its original PCM reader
- **WHEN** actual source replacement invalidates its captured source authority
- **THEN** the existing pump retires that unclaimed reader without waiting for a worker
- **AND** a normal Pair move preserving that original authority does not cancel its view

### Requirement: Native Inference Preserves the Whole Source Pipeline
A native replacement SHALL retain the original complete source, resampling,
frontend, true model chunk lengths, aggregation and postprocessing before
productive cutover. Symbolic graph metadata SHALL NOT substitute for actual
varied-length execution. Unknown output batch metadata MAY be accepted only
when the input batch is exactly one and every actual finite output has shape
`[1, actual_time]`. Productive runtime removal SHALL require complete quality,
CUDA/fresh-CPU fallback, CPU-only, cancellation, resource, provenance and
distribution parity. Host-memory refusal SHALL remain terminal.

Complete-source admission SHALL preserve the original512MiB mono float32 byte
bound, rates1–768000Hz and conservative250000-prediction ceiling. The native
frontend SHALL validate checked complete geometry before payload allocation,
resampling or STFT, without truncating source or tail to satisfy the bound.

#### Scenario: A short BeatThis piece uses global attention
- **GIVEN** a centered original spectrogram has at most1488 frames
- **WHEN** the native trial forms its original six-frame edges
- **THEN** model attention runs on exactly `T+12` frames without padding to1500
- **AND** border6/keep_first, minimal peak rules and physical-duration exclusion remain original
- **AND** a successful bounded synthetic comparison does not certify whole audio quality

#### Scenario: A standalone trial finishes reading temporary host buffers
- **GIVEN** the original full source has been admitted and a copying model session is used
- **WHEN** synchronous full resampling or model loading succeeds
- **THEN** original PCM and serialized graph buffers may end after their respective last reader
- **AND** original source frames and physical duration, full resample tail, prepared waveform, spectrogram, profile and exclusive success output remain intact
- **AND** direct/zero-copy graph loading requires its own graph lifetime and is not substituted
- **AND** Rust deallocation alone proves no OS memory release, process peak improvement or CUDA success

#### Scenario: A device trial cannot be admitted
- **GIVEN** the unchanged host ledger or owned child limit refuses the attempt
- **WHEN** native CUDA or cancellation verification is attempted
- **THEN** no CUDA success, device-failure-to-CPU or mid-ORT-cancellation claim is made
- **AND** existing productive workers and original resource floors remain available

#### Scenario: A selected Native BeatThis NVIDIA worker completes or qualifies for CPU fallback
- **GIVEN** the App admitted a complete source and selected the pinned Native CUDA package
- **WHEN** the guarded child completes the full dynamic final0 inference
- **THEN** a CUDA result requires observed CUDA compute in that request, the actual device and verified runtime images
- **AND** with automatic policy only, a qualified device error may start one fresh guarded CPU child after the GPU Job, tree and both readers end
- **AND** that child uses the same staged PCM, pinned graph and App request with fresh host admission
- **AND** mandatory CUDA, explicit CPU, cancellation, host pressure, malformed receipts and unknown failures preserve their own outcomes

#### Scenario: A standalone diagnostic reports a controlled CUDA arena refusal
- **GIVEN** an explicitly requested isolated CUDA limit1 diagnostic without cancellation
- **WHEN** actual ORT logs independently bind a unique Cuda/limit1 arena, its last same-thread extension and exactly matching positive rounded BFCArena refusal
- **THEN** a dedicated receipt and exit70 may identify controlled device-arena refusal, without claiming physical VRAM exhaustion
- **AND** Cpu/CudaPinned, unknown/missing/ambiguous diagnostics, generic host failures and crashes remain terminal
- **AND** any next same-model CPU process waits for actual original tree and reader settlement
- **AND** control fixtures alone do not establish actual CUDA execution or fallback

#### Scenario: A native inference is interrupted after kernel execution
- **GIVEN** a uniquely profiled original owned invocation with an explicit diagnostic RunOptions timer
- **WHEN** a termination-specific error is accompanied by actual kernel events inside that same failed model run
- **THEN** bounded mid-ORT termination may be reported after complete tree and reader end without success artifacts
- **AND** a pre-invocation marker, model-load failure or unrelated profile cannot establish this result

### Requirement: Heavy Background Work Shares Resource Admission
Existing offline owners SHALL coordinate heavy stem generation and analysis child
execution without a second scheduler, job queue or audio clock. Already-admitted
analysis SHALL wait cancellably for heavy capacity; resource contention SHALL NOT
become numerical failure or select a lower-precision BPM path. Effective child
numerical pools SHALL be bounded before model work, with child environment applied
before imports and no mutation of App-global numerical settings.

Windows stem command ownership SHALL assign the child to its kill-on-close Job
while suspended and retain the Job through actual tree and pipe-reader retirement.
The stem guard SHALL use actual Windows low-memory status and exhausted observed
headroom for admission and active stop. Stem command Jobs SHALL run without a RAM
or commit ceiling. Alignment SHALL account for the concrete block and outstanding
Native promises against measured capacity.

#### Scenario: Generation competes with playback or analysis
- **GIVEN** an original source-owned generation remains current
- **WHEN** a current requested subscriber's playback is active before inference admission
- **THEN** inference waits visibly without stopping any PAD or changing its original request
- **AND** saved-set lookup and component preparation retain independent admission
- **WHEN** a current requested subscriber's playback becomes active during an admitted generation
- **THEN** that attempt reports a distinct playback-headroom failure and retains custody until actual cleanup
- **AND** a waiting precise analysis retains its original quality and cancellation authority

#### Scenario: Windows signals low memory during a stem command
- **GIVEN** caller/system commit and physical headroom are measured separately
- **WHEN** the existing stem guard observes low memory or exhausted headroom
- **THEN** the owned command requests cancellation and retires its Job, tree and both readers
- **AND** the original attempt reports resource pressure after retirement
- **AND** unsupported platform monitoring and unmeasured device deadlines remain explicit

#### Scenario: A queued stem becomes ineligible for memory admission
- **GIVEN** a current inference, requested pair preparation or held reuse delivery owns an inactive common FIFO position
- **WHEN** the existing stem resource guard reports pressure or unavailable monitoring
- **THEN** the controller SHALL withdraw only that inactive position so an otherwise admitted background owner can progress
- **AND** the original request, interests, source custody and held delivery remain pending without failure or cancellation
- **AND** recovery rejoins the same owner behind existing FIFO waiters when monitoring reports clear
- **AND** active tickets and independent borrows remain owned until ordinary retirement, while failed deferred terminal deliveries retain their separate custody

### Requirement: Cancelled Strict Cold Preparation Retires Its Unclaimed Owners
Strict terminal-waiting cold preparation SHALL retire its exact unclaimed queued
operation after observing cancellation, without waiting for another heavy owner
or occupied cold worker. Captured owners SHALL be destroyed outside the queue
mutex before its reservation and heavy ticket are released and before the caller
returns. Independent queued operations SHALL remain intact. A worker-claimed
operation SHALL retain the existing actual-terminal wait and reader retirement.

#### Scenario: A cancelled Pair preparation waits behind an unrelated heavy job
- **GIVEN** the original strict Pair preparation is queued behind a live heavy owner
- **WHEN** its existing source or shutdown predicate reports cancellation
- **THEN** only that still-queued original is removed without starting its payload
- **AND** captured readers, cold admission and its heavy ticket retire in that order
- **AND** caller completion and a later FIFO owner remain blocked while capture destruction is unfinished
- **AND** already running work and other queued originals keep their existing ownership

#### Scenario: Public prepared-stem publication nests Pair preparation
- **GIVEN** the public prepared-stem operation captures its original Pair or legacy source
- **WHEN** its preparation enters the cold lane
- **THEN** one common heavy owner and strict terminal wait cover its transient work
- **AND** a nested Pair kernel reuses that same admitted owner without a second queue position
- **AND** public engine borrowing, original source currency and own-ACK publication remain unchanged

### Requirement: Failed Precise BPM Has a Usable Approximate Fallback
When precise automatic BPM estimation fails, the system SHALL attempt a simpler
bounded source-derived scalar estimate outside realtime. A validated usable
saved neural period SHALL be reused without repeating inference. Otherwise a
bounded interval or source-energy estimate MAY provide an approximate BPM.
Unusable, corrupt, unavailable, cancelled or stale source evidence SHALL NOT
invent a BPM. Existing precise checks SHALL retain their actual failed outcome.

The selected scalar SHALL carry a separate approximate status, source/model and
actual execution provenance, original precise refusal and captured request range.
It SHALL remain within that range without clamping or rounding into acceptance.
Valid same-source precise values and manual/Tap/half-double choices SHALL retain
precedence. Existing source/content/current-request, OwnACK and retirement guards
SHALL remain authoritative.

Approximate evidence SHALL display **Approximate BPM** and an explicit warning
that the value may be inaccurate, including after save, restore and copy. It
SHALL NOT certify beat/downbeat/phase, synthesize accepted timing or establish a
second clock. Explicit performer grid intent SHALL retain its separate authority.

#### Scenario: A usable period fails the precise phase check
- **GIVEN** current source-bound neural evidence retains a usable period
- **AND** precise estimation and independent corroboration have failed
- **WHEN** the approximate fallback completes for the current request
- **THEN** its scalar BPM is usable with a visible inaccuracy warning
- **AND** the original failed precision diagnostics remain unchanged
- **AND** diagnostic raw observations do not become an accepted playback phase

#### Scenario: No neural candidate is available
- **GIVEN** complete verified source PCM is admitted and the request is current
- **WHEN** bounded source-energy rhythm evidence supports a coarse period
- **THEN** the fallback returns an explicitly approximate scalar without claiming neural execution
- **AND** silence, corruption, cancellation or insufficient rhythm returns no invented BPM

### Requirement: Slow Stem Work Does Not Own Unrelated Live Controls
Stem disk preparation and numeric conversion SHALL run outside realtime and
SHALL NOT retain shared control-publication locks across slow file reads or
conversion. Native numeric work SHALL release Python execution while running.
Short admission/publication sections SHALL revalidate current source, content,
placement, intent, memory reservations and operation-specific acknowledgement.

#### Scenario: A bounded numeric stem block is converted
- **GIVEN** four retained immutable PCM16 source spans and complete prospective raw/payload/copy/scratch promises
- **WHEN** the Python-independent native core converts a bounded block outside interpreter execution
- **THEN** global binary64 interpolation, compensated mono averaging, ties-even PCM16 endpoints and ordered instrumental clipping match the independent byte oracle
- **AND** unrelated mutable public controls, their own trigger/audio and STOP can progress
- **AND** cancellation during native work is observed before any subsequent output write, while worker/source custody lasts through actual return
- **AND** exclusive RIFF/RF64 writers, complete output verification and private generation commit remain authoritative
- **AND** the actual compiled numeric/bridge implementation participates in the processing identity; unsupported binding evidence remains UNKNOWN

#### Scenario: Components load while another PAD is played
- **GIVEN** one PAD is preparing components under a controlled slow reader
- **WHEN** another loaded PAD receives trigger or STOP
- **THEN** those controls can reach their ordinary admission path independently of the slow read
- **AND** same-PAD ordering, cancellation, immutable sharing and last-reader retirement remain correct

#### Scenario: A committed component request waits in native admission
- **GIVEN** a public committed residency request has captured its source-bound admission state
- **WHEN** its native admission phase waits outside Python
- **THEN** it retains no Python Engine borrow or interpreter execution
- **AND** source, placement, launch, selection and reservations are revalidated before mutation
- **AND** an older captured request cannot supersede a newer admitted intent
- **AND** a failed newer admission does not revoke an earlier viable request or current proposal

#### Scenario: A cancelled committed request remains behind occupied workers
- **GIVEN** both cold workers are occupied and an unclaimed committed residency request holds its actual callback capture in the queue
- **WHEN** its own receipt is cancelled and the ordinary loader control pump runs
- **THEN** the original queued callable and its captured readers retire off realtime before its cold admission becomes available
- **AND** replacement admission does not wait for either occupied worker to finish
- **AND** independent queued work, current selection authority and original acknowledgements remain intact
- **AND** an uncancelled lane close retains the existing job guard settlement, while claimed readers keep their original lifetime

#### Scenario: Intent changes during a committed component read
- **GIVEN** a worker holds immutable source and file pins with a scoped component reservation
- **WHEN** cancellation, source/content replacement or a newer intent supersedes it during the read
- **THEN** its obsolete result cannot publish or borrow another PAD's acknowledgement
- **AND** actual readers and pending promises retire only after their real use ends

#### Scenario: A source window moves while a real read is held
- **GIVEN** the original content has promised its complete source/component bytes, owner witnesses, reader locators and source history before I/O
- **WHEN** its physical placement moves through a pending control mirror during the read
- **THEN** its immutable history promise follows the moved custody row
- **AND** Copy retains independent target history, while a retired row or equal-PCM replacement cannot inherit the original promise
- **AND** final publication still validates the original content lifetime, source/window, proposal and intent

#### Scenario: Historical views end during source preparation
- **GIVEN** a worker explicitly pins historical PCM for its preparation snapshot
- **WHEN** the original resident views retire while that worker still holds the PCM
- **THEN** those actual bytes remain charged once to its own resident scope until its accounting pin ends
- **AND** a foreign scope's PCM reference alone does not revive an ended local owner
- **AND** complete legacy conversion promises its separate transient host envelope before any source read

#### Scenario: Registration or binding preflight waits outside Python
- **GIVEN** a public registration, residency request or current-binding lookup has captured owned shared state
- **WHEN** native reconciliation or registration preflight waits
- **THEN** it retains no Python Engine borrow or interpreter execution
- **AND** registration revalidates current source/window, placement, discard and the original lane lifecycle after its final lock waits
- **AND** only successful registration advances its order watermark, so failed newer work leaves an earlier viable registration intact

### Requirement: Background Throughput Preserves Playback Headroom
Offline jobs SHALL use a coordinated bounded resource policy for concurrent
child/native CPU work. CPU-only systems SHALL retain a usable same-model path;
CUDA execution/fallback SHALL be reported truthfully. If supported concurrent
generation cannot meet the declared playback deadline checks, generation SHALL
be visibly deferred or gated without silently stopping active PADs.

#### Scenario: CUDA is unavailable or fails
- **GIVEN** a generation request permits automatic device selection
- **WHEN** CUDA is absent or its attempt has a device/runtime failure
- **THEN** the same supported model can execute through the CPU fallback
- **AND** its effective resource settings and actual device/fallback outcome remain observable
- **AND** cancellation or malformed/corrupt input retains its terminal outcome rather than being hidden by fallback

### Requirement: Native Conversion Preserves Artifact Semantics
Optimized conversion SHALL preserve bounded streaming geometry, established
binary64/PCM16 rounding and clipping order, RIFF/RF64 handling, cancellation and
exclusive private output custody. Changed processing implementations SHALL have
coherent provenance. Complete immutable artifact verification and descriptor-last
eligibility SHALL remain prerequisites to selection and independent live adoption.

#### Scenario: Conversion is cancelled or fails
- **GIVEN** detached conversion is writing a private generation
- **WHEN** cancellation, malformed input or an allocation/write failure occurs
- **THEN** no incomplete artifact is advertised as ready or adopted by audio
- **AND** only proven owned private output is eligible for failure cleanup

### Requirement: Native Inference Replacement Preserves Supported Behavior
Removing a productive PyTorch runtime SHALL require evidence of at least current
model quality, complete-source processing semantics, supported functions and
their interactions. Replacement SHALL preserve verified weight provenance, CPU
execution, supported CUDA/fallback behavior, bounded resources, cancellation and
distribution/recovery. A language binding to the same tensor runtime SHALL NOT
be described as removing that dependency.
Shared Rust logic SHALL remain portable across Windows, Linux and macOS, with
OS-specific process/device/resource behavior isolated behind platform adapters.
CPU operation SHALL remain a baseline; CUDA SHALL NOT be a required dependency
on systems without a supported NVIDIA device. Platform build/run claims SHALL
identify the OS/architecture actually checked.
GPU selection SHALL use supported provider capabilities rather than require
NVIDIA hardware. A practical AMD-capable replacement SHALL preserve model quality,
functions/interactions, bounded resources and CPU fallback. Actual AMD execution
SHALL remain distinct from mocked provider selection or an installed runtime.

#### Scenario: A model replacement cannot establish parity
- **GIVEN** a native inference replacement is being evaluated
- **WHEN** an operator, quality, device or distribution requirement remains unproved
- **THEN** the working productive path remains available and the barrier is reported
- **AND** no successful-load or synthetic-only result is presented as full no-regression acceptance

#### Scenario: An isolated BS spectral core reconstructs a complete chunk
- **GIVEN** a complete admitted 44.1kHz stereo chunk and four finite complex masks
- **WHEN** the independent offline Rust trial processes the pinned model's spectral path
- **THEN** periodic Hann2048, centered reflection, hop441, frequency-leading stereo features, complex mask multiplication and inverse window-envelope normalization preserve the original complete PCM length
- **AND** shape, finite, allocation and cancellation failures return no partial result; immutable source/mask custody lasts through actual return
- **AND** original Torch arithmetic comparisons include nonidentity complex masks and edge/tail lengths without neural inference
- **AND** learned network, CUDA AMP, whole-track chunk overlap, quality, producer and distribution parity remain required before productive runtime removal

#### Scenario: An isolated native trial selects a dynamic runtime

- **GIVEN** a fresh standalone trial requests dynamic ORT initialization
- **WHEN** it admits a caller-owned package manifest
- **THEN** exact manifest and runtime bytes, size, version and process OS/architecture are checked before ORT environment or model initialization
- **AND** missing, malformed, changed, foreign or ineffective configuration fails terminally without a device-fallback receipt
- **AND** CPU-only dynamic initialization does not require CUDA support, while a static build rejects dynamic configuration
- **AND** file/runtime initialization evidence does not certify mapped images, dependent providers, model execution, complete producer or portable distribution

#### Scenario: An isolated BS track core streams complete arithmetic
- **GIVEN** an admitted immutable stereo source and a synchronous complete four-source predictor
- **WHEN** the independent offline track core processes the release chunk485100, batch2 and overlap2
- **THEN** original virtual reflection, strict tail-padding thresholds, independent linear fades, float32 rolling accumulation and physical crop preserve every source frame in order
- **AND** tiny sources are padded to full model chunks, source and sink spans stay bounded, and source/model/output readers retain custody through actual completion
- **AND** short reads, invalid predictions, nonfinite/overflow values, sink failure or cancellation invalidate the entire private attempt, including earlier written blocks
- **AND** arithmetic-only predictions do not attest learned BS attention/weights/export/CUDA AMP, complete Demucs, quality, provenance, distribution or productive Torch removal

#### Scenario: A prospective prepared BS arena recipe uses production allocation rules
- **GIVEN** the original complete prepared diagnostic and track allocation schedule, actual target metadata Layouts and pinned nonthreaded packing behavior
- **WHEN** sparse header storage executes the SAME complete production first-fit allocation/free algorithm without learned payloads
- **THEN** the derived concrete extent is explicitly replayed for both16-aligned base residues0/16mod32, including headers, absolute alignment, tiny tails and fragmentation
- **AND** exact reachable free-list/live-state restoration is checked at packing, band, transformer, sample and complete B1/B2 batch boundaries before repetition is inferred
- **AND** all-four masks, optional original-feature forwarding, full485100/1101 geometry and persistent whole-track owners remain in the recipe
- **AND** arena-only success does not authorize model execution without complete native/process closure and sufficient fresh ORIGINAL admission, or certify learned/CUDA/quality/producer/distribution parity

#### Scenario: An isolated native BS predictor evaluates learned release weights
- **GIVEN** a complete admitted CPU attempt retains the original pinned configuration/checkpoint and a strict authenticated fixed-schema trial export
- **WHEN** the independent learned core evaluates all62 bands, eight time/frequency blocks and four mask heads
- **THEN** original L2-clamp normalization, adjacent-pair rotary, full noncausal scaled attention, normalized-input sigmoid gates, residual GELU feedforward and Tanh/GLU mask semantics are preserved
- **AND** bounded query tiles avoid a complete quadratic score tensor without shortening the release1101 time positions or frequency extent
- **AND** the existing spectral and track cores supply reconstruction and synchronous release-batch processing; weight/source/output readers and complete resource promises last through actual completion
- **AND** bad weights/digest/tail, shape, finite, allocation or cancellation errors invalidate the whole private attempt
- **AND** fallible native vector allocation failures report the owning phase and checked logical requested bytes without claiming a process peak or host/device failure cause
- **AND** synchronous readers finish before their private input buffers are released; a diagnostic second forward does not retain completed first-forward payloads, and any later failure invalidates all private output
- **AND** complete chunk/batch reconstruction may project independent stem masks serially into private caller-owned PCM, with one reusable inverse context and unchanged per-result arithmetic/geometry
- **AND** axial state remains live through the inverse; the complete batch destination, spectrum, single mask and inverse scratch remain included in simultaneous admission
- **AND** any later stem, arithmetic or cancellation failure invalidates the ENTIRE private destination and all prior output
- **AND** another real model attempt requires fresh original complete admission; conservative vector estimates do not attest total process/allocator memory
- **AND** the isolated diagnostic may own subsequent Rust allocations, including FFT planning and SGEMM packing, in fixed reusable backing without an exhaustion fallback to System; pre-activation pointers retain their original allocator ownership
- **AND** that backing survives every pointer/reader through process exit, and its capacity/counters do not prove full-model schedule sufficiency or remaining stack/image/CRT/loader/native process allocations
- **AND** on Windows that arena uses a dedicated private read/write reserve-and-commit mapping with checked rounding to the actual page size; logical pool capacity excludes page padding, and other platforms retain their original System backing with native extent unknown
- **AND** a failed backing acquisition publishes no allocator state, and the mapping survives all production readers through process exit; its known page extent does not bound other native costs or authorize model execution and is never added twice inside PrivateUsage
- **AND** Windows pre-activation Rust allocations own individual checked page-rounded mappings with explicitly aligned payloads and original-base headers; their final readers precede native release, including release after activation, while other platforms retain System ownership
- **AND** bootstrap realloc preserves old storage on refusal and migrates into the unchanged arena after activation without fallback; bootstrap mapping counters include their headers/slack/padding but do not bound native API/runtime/process costs, and failed release is terminal HOST failure without false settlement or unwinding
- **AND** the standalone learned CLI prepares exclusive output handles, paths and fixed I/O buffers before allocator activation, and retains initialized standard streams through terminal reporting
- **AND** prepared output buffers reach their sinks only through explicit writes/flushes; disposal after return, failure or unwind discards pending bytes without a destructor write retry, while successful completion still explicitly flushes then syncs the file
- **AND** ordinary trial return consumes all prepared source, weight, optional original-feature and output file owners after their last readers, before positive trial status; successful work attempts each output's flush, sync and single checked close even after another owner fails, while failed work abandons pending output without flush or sync
- **AND** first work/completion failure remains authoritative, all remaining owners still receive their sole close attempt, and failed close is unresolved OS ownership rather than proven settlement; checked native close is Windows-only and other platforms cannot report positive completion through implicit Drop
- **AND** terminal borrowed status/error writes and flushes are checked once and their locks released without closing runtime-owned standard handles; diagnostic-report failure preserves the original error and nonzero exit without recursive reporting
- **AND** CLI-owned sealed I/O failures preserve a static owning phase and numeric OS code without platform error-message formatting; original core errors and allocation phase/bytes remain intact
- **AND** any preparation, later I/O, arithmetic, allocation or cancellation failure invalidates the entire fresh private directory, including empty or partially flushed outputs, while an existing directory remains unchanged
- **AND** retained I/O removes concrete late preparation paths but does not prove native I/O/CRT/TLS/loader/exit closure, full allocator sufficiency or complete process admission
- **AND** the learned CLI prepares both directional FFT plans/windows before allocator activation, consumes them afterward to allocate FFT buffers/scratch and bounded inverse storage in the shared arena, and reuses the SAME exclusive workspace across all-four diagnostics and serial complete track samples
- **AND** each reconstruction rebuilds its exact current geometry envelope and clears accumulation per channel; a maximum-envelope prefix, stale partial state or a previous successful stem cannot certify a later attempt
- **AND** retained selected FFT plans/windows remain included in the preNN process allowance; consumed-plan working vectors belong to the shared arena with a separately proven complete ordered prefix, never counted twice inside PrivateUsage
- **AND** spectral/control reuse and an arena-only proof do not establish native process bounds, original admission, learned parity or a changed CUDA condition
- **AND** learned rotary, softmax/sigmoid and mask transcendental kernels use the pinned source-owned Rust math without late CRT transcendental calls, while preserving f32 expressions and original full-model numerical acceptance tolerances
- **AND** kernel/operator comparisons, including large finite arguments and exceptional classes, do not close retained FFT/stack/images/native I/O/TLS/Loader/exit/panic process backing or authorize a new learned/CUDA attempt
- **AND** bounded CPU arithmetic comparisons do not establish batch2/wholetrack, CUDA AMP, complete Demucs, quality, full producer, distribution or productive Torch-removal acceptance

#### Scenario: A standalone trial declares ORT package components
- **GIVEN** an explicit runtime request and a separate SHA-authenticated package manifest
- **WHEN** the standalone trial checks the declared package before ORT initialization
- **THEN** every declared component is size/digest/role/target checked from retained same-file handles in the same canonical package directory
- **AND** duplicate names or opened file identities, path escapes, unknown or malformed fields and a mismatched runtime entry fail terminally
- **AND** all file pins remain through the last session, profile and output reader
- **AND** absent optional package configuration retains only runtime-file evidence; checked declared files do not establish loader resolution, complete dependencies, mapped images, full producer or distribution

#### Scenario: An explicit standalone Windows trial controls native loading
- **GIVEN** a fresh dynamic standalone trial explicitly requests its verified package loader policy
- **WHEN** it loads the runtime and observes declared native images
- **THEN** the initial load restricts directory search to its package and System32, checks the actual returned module and API export owner, and verifies observed backing-file identity against retained pins
- **AND** preloaded package images, foreign or undeclared package images and incomplete module queries fail terminally before success output
- **AND** loader policy and all admitted file pins remain process-local through actual runtime readers and process exit, without eagerly loading CUDA for CPU probes
- **AND** legacy requests retain their behavior, unsupported platforms reject this policy, and observed declared backing files do not certify all loader overrides, relocated image bytes, full dependency closure, producer, CUDA execution or quality

### Requirement: Heavy Stem And Analysis Jobs Execute Sequentially
PAD stem generation, Generate all Stems and complete BPM/Key analysis SHALL share
one sequential heavy-work admission while retaining their existing bounded queues.
The next independent heavy job SHALL NOT start PCM/model preparation until the
previous job's transient owners, actual child trees and readers have retired.
Ordinary playback preparation SHALL remain eligible while heavy work waits.
Same-job branches MAY overlap only under the same retained admission and SHALL
both end before that admission is released. Cancellation and UI completion alone
SHALL NOT certify resource release. Persistent playback sources and published
results SHALL retain their valid ownership; no OS RAM trimming is required.

#### Scenario: PAD and batch stems compete with complete analysis
- **GIVEN** one Stem or BPM/Key job still owns transient model/PCM storage or a child/reader
- **WHEN** more PAD requests, Generate all Stems or Analyze Audio are requested
- **THEN** their existing bounded queues retain requests without starting another independent heavy job
- **AND** a cancelled queued request releases only its own admission and cannot strand later work
- **AND** the successor starts only after actual prior owner retirement, including error and shutdown paths
- **AND** ordinary Cold playback preparation may progress while the heavy job remains occupied

#### Scenario: Available memory differs between machines
- **GIVEN** a supported machine with16-32GiB RAM or a smaller supported configuration
- **WHEN** queued heavy work becomes eligible
- **THEN** admission uses actual available memory/commit and outstanding reservations with the existing safety floors
- **AND** a fixed8GiB machine assumption does not impose an additional global ceiling
- **AND** original geometry, numerical quality and failure/retirement guards remain unchanged

#### Scenario: Beat output setup fails after its child exists
- **GIVEN** an admitted Beat worker has created its child process
- **WHEN** stdout validation or output-reader construction fails, or an unexpected setup or supervision exception propagates
- **THEN** the existing process owner still terminates and reaps the child and settles its readers
- **AND** temporary storage, Beat slots and the same heavy-work permission remain owned until actual retirement, including delayed cleanup
- **AND** another heavy job cannot start early, while a fresh Beat request can run after the failed attempt's resources retire

#### Scenario: Native heavy admission fails before Beat work begins
- **GIVEN** a Beat invocation has not begun payload work or created a child
- **WHEN** Native heavy-ticket construction or acquisition reports an admission failure
- **THEN** the invocation reports terminal supervision failure and settles its own retirement event
- **AND** both CPU and CUDA protocol bridges can return without waiting for nonexistent readers
- **AND** a diagnostic analysis job retires its captured Native PCM and reports failure without starting either branch or fabricating cancellation
- **AND** caller callback and admitted-payload exceptions keep their original identity, while actual live tree and reader ownership remains protected

#### Scenario: Diagnostic cancellation bridge construction fails
- **GIVEN** diagnostic analysis owns captured Native source admission and has not started either analysis branch
- **WHEN** cancellation-thread construction raises an error covered by existing supervision failure handling
- **THEN** the job follows its ordinary Native PCM retirement and terminal failure settlement without exporting PCM, starting inference or fabricating cancellation
- **AND** an existing admission failure remains authoritative without constructing the optional bridge
- **AND** prior cancellation remains cancellation, and a real Native retirement refusal remains nonterminal with any admitted heavy borrow retained
- **AND** another heavy owner can acquire only after the original job's actual required retirement completes

#### Scenario: Other PADs play while stems are generated for a stopped PAD
- **GIVEN** the requested stem PAD is stopped and resource admission permits background work
- **WHEN** other PADs play or the performer operates their START/STOP controls
- **THEN** that unrelated playback does not defer or abort the stem calculation
- **AND** current requested subscribers, original source/cancellation, actual memory pressure and publication guards remain authoritative
- **AND** no PAD is stopped automatically to make background work eligible

#### Scenario: A shared job retains an obsolete playing subscriber
- **GIVEN** a shared physical stem job retains a stopped current subscriber and another subscriber whose source or content has changed
- **WHEN** the obsolete subscriber's PAD plays while its old readers are still retained
- **THEN** that unrelated current playback does not defer or abort the surviving subscriber's calculation
- **AND** currentness is checked against the existing complete source, content, placement and intent authority without releasing the obsolete reader's custody
- **AND** an active interested subscriber in an unsettled Pair mirror continues to fence work until that mirror settles
- **AND** heavy admission, memory pressure, cancellation and final publication checks retain their existing authority

#### Scenario: Explicit stock BS model-call diagnostics
- **GIVEN** an ordinary admitted stock BS child has the explicit model-call diagnostic enabled
- **WHEN** its first actual model call returns successfully
- **THEN** it may emit one fixed scalar record of its PID, actual device and monotonic/process-CPU boundaries around that call
- **AND** output emission occurs outside the measured call and no additional worker, model route or CUDA synchronization is introduced
- **AND** a failed call emits no successful-call record, and execution/provenance/admission still require their original independent guards
- **AND** CUDA host return is not presented as completed kernel timing or device-deadline evidence

### Requirement: Automatic Acceleration Selection Includes Integrated GPUs
Hardware-aware offline execution SHALL consider supported NVIDIA/AMD discrete
GPUs, Intel/AMD integrated GPUs and CPU paths using actual provider/model
capabilities and transfer-inclusive cost/resource evidence. Selection/calibration
SHALL remain outside realtime and preserve playback headroom. Actual execution
and fallback SHALL be observable; missing acceleration SHALL retain CPU operation.
NVIDIA CUDA SHALL remain a mandatory supported acceleration path, including
native inference replacements, with same-model CPU fallback on systems without
usable CUDA. Cost/benefit discretion SHALL apply to additional non-CUDA providers
and SHALL NOT justify dropping CUDA support. CUDA installation SHALL NOT be
mandatory on CPU-only systems.

#### Scenario: An integrated GPU is present but unsuitable for a workload
- **GIVEN** a system has a supported integrated GPU and a working CPU path
- **WHEN** model support, available memory or measured total performance favors CPU
- **THEN** automatic selection may choose CPU without treating GPU presence as a speed guarantee
- **AND** GPU initialization/execution failure retains the same model's guarded CPU fallback

#### Scenario: Passive calibration selects an existing separator route
- **GIVEN** completed verified observations share the exact source, shape, model/configuration, complete producer and runtime/driver/device identity
- **WHEN** complete transfer-inclusive CPU cost is meaningfully lower, or measured CUDA allocation exceeds current free device headroom
- **THEN** automatic selection may choose CPU for that workload without starting additional calibration inference
- **AND** expired, unknown, failed, cancelled, partial or fallback observations cannot establish a performance preference
- **AND** explicit CPU intent, shared host admission and playback gates retain authority

#### Scenario: A CUDA attempt fails outside its device operation
- **GIVEN** the original child encounters a source, checkpoint, decode, PCM or encoder failure
- **WHEN** that error has no dedicated device-operation failure receipt and exit status
- **THEN** the attempt fails without repeating the model on CPU
- **AND** only a recognized placement, transfer or inference CUDA fault can request the same-model CPU fallback

#### Scenario: Completed execution qualifies passive calibration
- **GIVEN** an ordinary completed separator result has passed final source, producer, resource and actual-reader settlement guards
- **WHEN** its observation is considered for the bounded 32-key, 600-second calibration cache
- **THEN** its backend, model and effective one-thread pools SHALL match the captured plan
- **AND** CUDA learning SHALL require known allocator peak bytes and the child's actual runtime/driver/device identity matching the probed key, using the exact executed ordinal
- **AND** missing, foreign or mixed-device observations remain valid optional diagnostics without teaching a preference or repeating successful inference
- **AND** old V1 receipts remain readable, CPU execution queries no CUDA metadata, and original observation age is never renewed by promotion

### Requirement: Separator Diagnostics Cannot Become Protocol Receipts
The host SHALL preserve the distinction between diagnostic text and exact child
protocol lines while compacting or truncating command output. Whitespace
normalization SHALL NOT manufacture encoder, BS input, completed-execution or
CUDA-failure evidence. Genuine independent receipts SHALL remain available, and
duplicate or oversized receipts SHALL remain invalid.
Receipt framing SHALL use the raw child's LF boundaries, retaining the existing
trailing-CR and end-of-stream tolerance. Embedded CR, VT, FF, record separators,
NEL and Unicode line separators SHALL remain diagnostic text after decoding.

#### Scenario: An embedded diagnostic separator precedes a marker
- **GIVEN** an owned child embeds a protocol marker after a non-LF diagnostic separator
- **WHEN** the host captures, decodes and compacts that output
- **THEN** the marker remains diagnostic for all four receipt roles
- **AND** one separate genuine LF-framed receipt remains valid without becoming a duplicate

#### Scenario: An indented marker accompanies the CUDA-failure exit code
- **GIVEN** an owned separator prints only an indented CUDA-failure marker as diagnostic text
- **WHEN** it exits with the dedicated CUDA-failure exit code
- **THEN** the compacted diagnostic remains distinct from an exact receipt
- **AND** neither separator retries on CPU on the strength of that diagnostic

#### Scenario: An indented execution diagnostic accompanies a genuine receipt
- **GIVEN** a diagnostic contains an indented completed-execution marker
- **AND** the child emits one separate exact completed-execution receipt
- **WHEN** the host normalizes command output
- **THEN** only the separate exact receipt is available to the execution parser
- **AND** the diagnostic remains bounded and visibly distinct from protocol evidence

### Requirement: Offline Process Producer Sources Retain Their Actual Roles
Guarded separator producer inventories SHALL retain the shared Linux supervisor
source as a loaded parent role and its directly executed isolated broker source
as a separate child role. Existing actual tree, reader and publication custody
SHALL retain both files without importing the broker into the parent or relaxing
loaded-class evidence. Incomplete full-host evidence SHALL remain UNKNOWN.

#### Scenario: A shared process class references the Linux supervisor
- **GIVEN** the shared process helper imports the Linux supervisor on the current host
- **WHEN** the host component compares its loaded declarations and bindings with held source
- **THEN** its supervisor declaration is available in the guarded parent inventory, including on Windows
- **AND** changed supervisor source or a substituted imported class invalidates that component
- **AND** equal-byte relocation preserves component identity without certifying the complete inference host

#### Scenario: Either separator retains the isolated broker source
- **GIVEN** either separator captures its complete producer source inventory
- **WHEN** its existing source guard remains live through processing and publication or cancellation
- **THEN** the exact sibling broker file is held against replacement as a child source role
- **AND** the broker is excluded from parent loaded-code inspection and is never imported for evidence
- **AND** changes after actual guard retirement alter the next source witness while package roots and full-host UNKNOWN remain authoritative

### Requirement: Host Constructor Evidence Includes Generated Execution
A known guarded dataclass component SHALL bind its actual supported constructor
execution to held source declarations, including generated bodies, defaults,
keyword defaults, captured bindings and source-declared post-init calls.
Metadata or signatures alone SHALL NOT establish this constructor evidence.
Unsupported constructor generation SHALL remain UNKNOWN, and constructor evidence
SHALL NOT establish complete host, validator or other generated-method evidence.

#### Scenario: A generated limits constructor skips source validation
- **GIVEN** held source declares a dataclass with a checked post-init bounds method
- **WHEN** the loaded constructor omits that call while retaining its field metadata
- **THEN** the guarded component is UNKNOWN
- **AND** checking constructor evidence does not invoke the host constructor or hook

#### Scenario: Generated defaults or frozen captures change
- **GIVEN** a component previously matched supported source-backed generation
- **WHEN** positional or keyword defaults change their value or exact builtin type, or a frozen constructor captures a different object binding
- **THEN** the component is UNKNOWN even when its code or field metadata is unchanged
- **AND** existing complete-host UNKNOWN gates and actual reader-retirement authority remain intact

### Requirement: Host Default Value Evidence Preserves Exact Builtin Types
Guarded source-declared functions and methods SHALL compare supported default and
constant values recursively without equating distinct builtin types. Unsupported
builtin subclasses, positional-default containers, keyword-default containers or
keyword names SHALL remain UNKNOWN before their custom comparison or encoding
hooks run. Source-backed callable defaults and existing mutable runtime-carrier
policies SHALL retain their separate evidence rules.

#### Scenario: An equal-valued default changes runtime behavior
- **GIVEN** held source declares an integer default and its loaded function matches
- **WHEN** the actual positional or keyword default becomes boolean True, including inside a supported nested collection
- **THEN** the component is UNKNOWN even when Python equality considers the values equal
- **AND** source-declared constructors and retained callable defaults apply the same type-preserving value policy
- **AND** neither complete-host identity nor model, device or quality acceptance is inferred

### Requirement: Host Dictionary Value Evidence Preserves Insertion Order
Supported dictionary values SHALL retain their actual entry order recursively
when compared with held source, including ordinary defaults, measured constants
and supported generated-constructor defaults. Equal key/value pairs alone SHALL
NOT establish that ordered dictionary behavior matches the source. Source-backed
callable defaults SHALL apply the same rule to their retained inner defaults.
Set and frozenset evidence SHALL retain its existing unordered policy.

#### Scenario: Equal dictionaries produce different iteration results
- **GIVEN** held source and loaded function code agree and a dictionary default returns its first key
- **WHEN** the actual default is replaced by an equal dictionary with reversed insertion order
- **THEN** the component is UNKNOWN even though its source and function code remain unchanged
- **AND** an equal replacement retaining the original entry order preserves known identity
- **AND** alias topology and complete-host, model, device and quality evidence retain their separate limits

### Requirement: Host Evidence Preserves Bounded Mutable Owner Sharing
Guarded host evidence SHALL compare the sharing topology of exact mutable list,
dict and set owners across one function's positional and keyword defaults, across
supported generated-constructor defaults, and within each measured constant.
Exact tuple/list/dict/slice paths SHALL use deterministic evidence without
persisting object IDs or invoking subclass hooks. Cycles and excessive graph
depth or size SHALL remain UNKNOWN. Existing source-backed callable, exact-type,
dictionary-order, canonical-set and runtime-carrier checks SHALL remain intact.
Immutable identity and relationships across separate constants, functions or
global/default roots retain separate limits; this is not complete object identity.

#### Scenario: Equal defaults change their shared owner
- **GIVEN** held source and loaded function code agree and two equal mutable defaults have distinct owners
- **WHEN** actual defaults share one owner, including across positional and keyword areas
- **THEN** the component is UNKNOWN even though memberwise values remain equal
- **AND** splitting a declared shared owner is also UNKNOWN
- **AND** replacing defaults with fresh owners preserving the declared topology retains identity
- **AND** both original complete-host UNKNOWN gates and all model, device and quality gates remain open

### Requirement: Rejected Stem Submission Cannot Restart Work
The existing stem reservation SHALL decide executor acceptance before any worker
can invoke the submitted callable. A RuntimeError during worker startup, including
after executor enqueue, SHALL release the rejected callable and its reserved
capacity exactly once before reporting failure. A later drained wrapper SHALL
NOT invoke that callable or access caller-cleaned source or preparation owners.
Accepted work SHALL execute outside the reservation lock and retain its capacity
until actual return. The existing two-worker/32-queued admission remains intact;
it does not bound the executor's raw count of inert wrappers after rejection.

#### Scenario: Worker startup fails after Pair preparation is enqueued
- **GIVEN** Pair preparation captures its source, retirement and heavy-work owners under an existing worker reservation
- **WHEN** the real executor enqueues the wrapper and worker startup raises RuntimeError
- **THEN** ordinary submission failure closes those captured owners and returns the reserved capacity once
- **AND** a later executor drain performs no Native preparation or result delivery for the rejected request
- **AND** an existing worker that dequeues before submission fails cannot enter the rejected callable

### Requirement: Failed Waiting Stem Admission Uses Fenced Terminal Delivery
The control poll SHALL settle Native heavy ticket RuntimeErrors before inference
through existing terminal job event delivery. Constructor or acquisition refusal
terminates only that waiting attempt, without launching its backend or
fabricating cancellation. Current subscriber, source, content and intent checks
and deferred Pair delivery SHALL remain authoritative. Failed tickets and
captured owners SHALL retire through ordinary job disposal after all required
deliveries. Legacy delivery reentry SHALL NOT submit a surviving owner twice.

#### Scenario: Native admission refuses an otherwise eligible waiting job
- **GIVEN** identity resolution completed and the original job is waiting for heavy admission
- **WHEN** Native ticket construction or acquisition raises RuntimeError
- **THEN** the original current subscribers receive a terminal failure through ordinary delivery
- **AND** the failed waiting attempt launches no inference and leaves unused worker capacity available

#### Scenario: A shared subscriber is deferred during admission refusal
- **GIVEN** a waiting job has current and Pair-deferred subscribers
- **WHEN** Native heavy admission refuses the attempt
- **THEN** available current subscribers receive the failure without changing deferred state
- **AND** deferred custody remains until unfenced delivery and ordinary disposal
- **AND** obsolete subscribers cannot apply the failure to replacement content
- **AND** later waiting work retains exact registry and admission ownership across delivery reentry

### Requirement: Queued Asynchronous Analysis Retires Invalidated Captures
Queued asynchronous Native analysis SHALL observe its original source/request
and shutdown cancellation through existing workers and the serviced control pump,
without waiting for unrelated heavy work. Predicate execution and captured-owner
destruction SHALL occur outside the queue mutex and Python attachment. Every
observation SHALL borrow the same complete cold admission and heavy capability
until its predicate owners end. Queue visibility, ordinary cold progress and
exact receipt removal SHALL remain intact; an already claimed payload SHALL
retain its original admission until actual terminal retirement.

#### Scenario: Queued analysis becomes invalid behind a live heavy predecessor
- **GIVEN** an actual analysis request captures its PCM reader and task token while unrelated heavy work remains active
- **WHEN** its original request, source or shutdown authority invalidates the queued request
- **THEN** existing worker polling retires the unclaimed callable and its captured owners before the predecessor ends
- **AND** the serviced control pump provides the same retirement when both cold workers are occupied
- **AND** the cancelled callable never starts and unrelated ordinary cold jobs remain eligible

#### Scenario: A cancellation snapshot races an actual worker claim
- **GIVEN** a queued predicate observation borrows the original reservation and heavy ticket
- **WHEN** a worker claims the original before that observation attempts exact queued removal
- **THEN** the observation cannot remove or certify the running payload as terminal
- **AND** neither payload nor predicate captures can outlive the complete original admission
- **AND** ordinary peers and existing strict terminal waiters retain queue visibility

### Requirement: Queued Asynchronous Loads Retire Through Original Terminal Delivery
Queued ordinary and analysis-bearing loads SHALL observe their original unarmed
request, epoch, proposal, placement, resident cancellation and shutdown authority
through existing workers and the serviced control pump. Pending placement alone
SHALL NOT cancel a valid load. An accepted unclaimed load SHALL await its original
admission commit before destroying captured source/material/pin and loading owners,
then settle cancellation through existing cleanup and filtered terminal delivery.
Predicates and terminal callbacks SHALL retain the same complete admission until
their owners end and SHALL run outside the queue mutex. Terminal events alone
SHALL NOT certify complete resource release. Claimed work SHALL retain its actual
terminal reader obligations; rejection SHALL NOT run accepted-queue cleanup.

#### Scenario: An invalidated load waits behind unrelated work
- **GIVEN** an actual queued load retains an old source reader and its exact loading token
- **WHEN** original request, epoch, placement, resident or shutdown authority cancels it before worker claim
- **THEN** existing polling retires its captured owners without starting preparation or waiting for unrelated heavy work
- **AND** a serviced control pump provides retirement when both ordinary cold workers remain occupied
- **AND** existing cancellation cleanup and request-filtered error delivery settle that attempt once, without removing a replacement loading token

#### Scenario: Queue shutdown races the original admission commit
- **GIVEN** an accepted load is queued while its caller still owns the request commit lock
- **WHEN** shutdown removes that unclaimed load
- **THEN** queued retirement awaits and releases that lock before destroying its loading guard and captured owners
- **AND** a rejected submission never invokes this accepted-job retirement callback under caller-owned locks
- **AND** a raced actual worker claim disarms queued-only terminal cleanup

#### Scenario: A queued source remains current during pending placement
- **GIVEN** a valid unarmed load is queued while the content-placement mirror is pending
- **WHEN** cancellation polling runs without another definitive invalidation
- **THEN** the original job and loading admission remain alive
- **AND** after the mirror settles the source can prepare, publish and reach its original own ACK
- **AND** ordinary start-policy changes do not become source cancellation

#### Scenario: A cold queue is poisoned before permanent shutdown
- **GIVEN** accepted unclaimed jobs retain their callable, terminal cleanup and original admission owners
- **WHEN** permanent shutdown closes a cold lane whose queue mutex is poisoned
- **THEN** shutdown recovers the guard only to close admission and take the queued jobs
- **AND** captured callables and accepted terminal cleanup end outside the queue mutex before their reservation and heavy capability
- **AND** new admission remains refused and the mutex remains poisoned
- **AND** claimed payloads retain their actual readers and admission until ordinary terminal retirement

### Requirement: Asset Sweep Observation Ends Under Its Admission Gate
Temporary strong path-owner observations made by project asset collection SHALL
end before releasing the shared owner-admission/deletion gate. An overlapping
sweep SHALL NOT promote the previous sweep's observation after actual application
ownership ends. Existing live assignment, source-job and Native PCM reader owners,
immutable file identity, unknown-file preservation and bounded fair retirement
SHALL remain authoritative. Observation END alone SHALL NOT certify reader END.

#### Scenario: A second sweep follows actual application pin release
- **GIVEN** a sweep observes a real application pin protecting a queued original or immutable stem generation
- **WHEN** that sweep unlocks and the application releases its last actual pin before another sweep
- **THEN** the previous observation cannot keep the exact retired path alive
- **AND** an independent surviving application pin still preserves its bytes until its own release
- **AND** unknown, replaced or actually read files retain their existing protection

### Requirement: Whole Resident Window Cancellation Retires Unclaimed Captures
A queued resident-window preparation SHALL retire its complete callable when its
original whole publication is rejected. Retirement SHALL run outside the cold
queue mutex through the existing ordinary lane and serviced loader pump. Its old/bank readers
SHALL end before its ordinary reservation becomes available. Lane shutdown SHALL
retain the original job guard settlement. Shared member cancellation that leaves
an independent owner or another live member SHALL NOT reject the whole physical
preparation. Claimed work SHALL retain its existing source, proposal, placement,
publication and actual reader-lifetime checks.

#### Scenario: A cancelled window waits behind occupied workers
- **GIVEN** both cold workers are occupied and an unclaimed window preparation retains its actual old/bank readers in a full queue
- **WHEN** its whole ticket is cancelled and the ordinary loader control pump runs
- **THEN** those queued captures retire before replacement admission without waiting for either occupied worker
- **AND** current source/window playback, selection authority and independent queued jobs remain intact
- **AND** this additional-reader retirement does not certify the shared backing's last reader or OS memory release

#### Scenario: One member cancels a shared window preparation
- **GIVEN** a queued physical window preparation has another live member or an independent owner
- **WHEN** one member cancels its own ticket
- **THEN** that member loses its own launch authority while the shared preparation remains eligible
- **AND** the surviving owner still requires its original independent acknowledgement

#### Scenario: A claimed cancelled source remains saved for a fresh restore
- **GIVEN** a real source reader is held inside its claimed window preparation and its exact original/cache assignments remain saved
- **WHEN** the window is cancelled
- **THEN** its reservation remains held until the actual reader returns, after which the cancelled publication leaves no queued message or pending reservation
- **AND** ending the old engine, PCM readers and actual workers preserves those same saved file identities and bytes
- **AND** a fresh warm preparation of those files requires its own current ordinary source acknowledgement and exact full-source PCM
- **AND** restoring the earlier finite view separately requires its original range, a current ordinary window acknowledgement and exact finite PCM
- **AND** only explicit own assignment end after restored readers and workers end authorizes selective retirement

### Requirement: Definitive Launch Revocation Retires Queued Committed Captures
A queued committed-residency capture SHALL retire through its existing ordinary
cold lane and serviced loader pump when its original content lifetime has a
definitively different launch revision. Its actual captured reader SHALL end
before its reservation becomes available, without waiting for occupied workers
or consumption of an already admitted STOP callback. Temporary placement
unavailability or another PAD's STOP SHALL NOT substitute for that definitive
revision change. Original status settlement, source/selection authority, worker
claim and actual claimed-reader lifetime checks SHALL remain intact.

#### Scenario: Public STOP follows a real queued voice capture
- **GIVEN** both workers are occupied and a full queue includes a committed operation with its actual callback capture
- **WHEN** public STOP revokes that content's original launch revision and the loader pump runs before the STOP callback
- **THEN** the extra queued reader and reservation retire before replacement admission
- **AND** original selection, source and current playback remain intact until the genuine STOP callback is consumed
- **AND** this additional-reader retirement does not certify the backing's last reader or OS memory release

#### Scenario: Another PAD stops while the capture remains eligible
- **GIVEN** a queued committed operation retains its original content and launch revision
- **WHEN** public STOP affects a different PAD or placement is temporarily pending without changing that revision
- **THEN** the original operation remains eligible and retains its actual capture
- **AND** after preparation its own original acknowledgement remains necessary

### Requirement: Superseded Resident Window Proposals Retire Queued Readers
A queued resident-window preparation SHALL retire through its existing ordinary
cancellable lane when its original proposal definitively loses authority after
a successfully admitted newer preparation. Its actual old/bank readers SHALL end
before its reservation becomes available, outside the queue mutex and without
waiting for occupied workers. Refusal of a newer request SHALL NOT invalidate a
still-current original proposal. The surviving preparation SHALL retain its own
publication and acknowledgement; source, placement, shared-member, independent
owner and claimed-reader guards SHALL remain intact.

#### Scenario: A newer public loop request supersedes queued preparation
- **GIVEN** two successfully admitted public loop requests for the same content retain their actual readers behind occupied workers in a full queue
- **WHEN** the newer proposal makes the original proposal non-current and the loader control pump runs
- **THEN** only the obsolete preparation's queued readers and reservation retire before replacement admission
- **AND** original source, selection and window remain intact until the surviving request's actual callback and own acknowledgement
- **AND** independent queued jobs remain eligible and this additional-reader retirement does not certify backing-last-reader or OS memory release

#### Scenario: A full queue refuses a newer public loop request
- **GIVEN** an original queued loop preparation remains current and its ordinary queue is full
- **WHEN** admission of a different newer loop request is refused
- **THEN** the original proposal, readers and reservation remain eligible
- **AND** it subsequently requires its own original callback acknowledgement for its exact requested window

### Requirement: FullMix Intent Revocation Retires Queued Window Captures
A queued resident-window preparation SHALL observe a definitive change to its
original prepublication storage-intent epoch through the existing ordinary
cancellable lane. Successful FullMix control admission SHALL retire that obsolete
callable and its actual source/component captures before returning its reservation,
without waiting for occupied workers or the FullMix callback. The original
Pending mirror SHALL retain its distinct settlement and reader obligations.
Control-queue refusal SHALL NOT revoke current window work. Original source,
selection, proposal, shared-owner, placement, worker-claim and claimed-reader
guards SHALL remain authoritative.

#### Scenario: FullMix replaces an unclaimed window after genuine AllStems adoption
- **GIVEN** actual AllStems adoption has completed and a full cold queue retains the subsequent window preparation behind occupied workers
- **WHEN** FullMix is successfully admitted and the ordinary loader control pump runs before its callback
- **THEN** only the obsolete window's actual source/component captures and reservation retire through existing off-lock queued cleanup
- **AND** the separately invalidated original Pending mirror settles through its existing path
- **AND** callback demand, original source and selected disk authority remain intact until the genuine FullMix callback
- **AND** independent queued work and a fresh window with its own acknowledgement remain valid

#### Scenario: A full control queue refuses FullMix
- **GIVEN** an original queued window remains eligible after genuine AllStems adoption
- **WHEN** the actual control queue refuses FullMix before changing its intent epoch
- **THEN** the original window and Pending readers, reservation and intent remain intact
- **AND** that original window still requires its own actual callback acknowledgement

### Requirement: FullMix Revocation Retires Unarmed Committed Captures
Queued committed-residency preparation SHALL observe a definitive change to its
original storage intent while its proposal remains unarmed. Successful direct
FullMix admission SHALL retire the obsolete callable's actual source and component
captures through the existing ordinary lane before returning its reservation,
without waiting for occupied workers or the FullMix callback. An armed proposal
SHALL retain its existing publication and claimed-reader obligations. Original
Pending mirrors SHALL settle through their distinct existing path. Refused
FullMix admission SHALL preserve the current committed request and its own ACK.

#### Scenario: Direct FullMix follows a committed Capture from actual AllStems
- **GIVEN** actual AllStems adoption and own acknowledgement have completed and a subsequent committed request retains its real callback Capture behind occupied workers in a full queue
- **WHEN** the public FullMix admission helper succeeds and the loader pump runs before its callback
- **THEN** the obsolete unarmed capture's two source readers, four component readers and queue reservation retire outside the queue mutex
- **AND** the old Pending mirror settles separately while source, disk selection and callback AllStems demand remain intact until the genuine FullMix callback
- **AND** a fresh committed request subsequently requires its own acknowledgement and exact requested component mask

#### Scenario: A full control queue refuses direct FullMix during committed preparation
- **GIVEN** an original committed Capture remains eligible after actual AllStems adoption
- **WHEN** the actual control queue refuses FullMix before advancing storage intent
- **THEN** the original capture, Pending mirror, storage intent and reservation remain intact
- **AND** after workers resume the original request reaches its own acknowledgement for its exact requested mask

### Requirement: Configured Kernel Cache Retains One Source-Owned Publication Owner
The offline Windows memory-query cache SHALL use one lazy source-owned owner with
short locked lookup/publication and configuration outside its lock. It SHALL
publish only fully configured successful wrappers, propagate initialization errors,
and permit retry. Concurrent cold callers SHALL return their own configured wrapper;
clearing SHALL NOT cancel initialization or prevent subsequent in-flight publication.
Guarded evidence SHALL bind actual constructor/type, methods, slots, lock and owner
relationships without encoding live WinDLL or lock state as implementation.

#### Scenario: Clear overlaps a cold initializer
- **GIVEN** a cache initializer has started but has not returned a complete wrapper
- **WHEN** another caller clears the cache and successfully initializes a new wrapper
- **THEN** the original initializer may later return and publish its own wrapper
- **AND** no partially configured wrapper or failed initialization becomes cached
- **AND** observation cadence and monitoring failure deadlines remain unchanged

### Requirement: Resource Clock Evidence Preserves Definition-Time Capture
The exact declared StemResourceGuard constructor clock default SHALL retain its
definition-time capture. Guarded evidence MAY compare it to an independently
captured time.monotonic builtin object. This policy SHALL be
restricted to that source role, constructor, clock parameter and expression. It
SHALL retain definition-time capture and explicit clock injection. Arbitrary builtin
defaults SHALL remain unsupported. Native C/image provenance SHALL remain separate.

#### Scenario: Module clock changes after constructor definition
- **GIVEN** the actual constructor retains its independently matched original default
- **WHEN** the mutable time.monotonic module attribute is replaced afterward
- **THEN** default guard construction retains the originally captured clock
- **AND** explicit injected clocks retain their original behavior
- **AND** replacing the constructor default itself refuses guarded identity
- **AND** component binding proof does not establish complete-host, native-image or model acceptance

### Requirement: Cold Publication Coordinates With Staging Recovery Readers
Cold PCM publication SHALL coordinate its final owned-handle release, owner-marker
deletion and staging rename with the store's actual staging recovery readers.
It SHALL retain immutable sharing, file/directory identity checks, unknown-file
preservation and off-realtime execution. Cancellation predicates, committed
verification and lifecycle publication SHALL execute outside that exclusion.
Independent sharing failures SHALL retain their original error and cleanup custody.

#### Scenario: Recovery observes a live generation during ordinary source restore
- **GIVEN** an ordinary flat source has complete verified cold artifacts
- **AND** recovery holds the actual live generation's directory and owner marker
- **WHEN** its current cold transaction reaches final publication
- **THEN** publication waits for those recovery readers before deleting or renaming
- **AND** complete committed verification and source-own acknowledgement remain required
- **AND** cancellation during that wait refuses adoption and retires only owned artifacts
- **AND** an uncoordinated reader still produces its original sharing error

### Requirement: Explicit Native HTDemucs CPU Uses the Existing Stem Owner

The Windows source-checkout App SHALL capture an explicit Native HTDemucs CPU
selection with its stem request. A Native model child SHALL use the
independently admitted CPU package and release-pinned learned Scan64 graph under
the existing suspended-launch, Job, cancellation and reader ownership. The
worker SHALL verify its dedicated Job independently of memory policy. Source,
output and planner extents SHALL retain exact geometry and checked arithmetic.
The App SHALL verify the completed PCM before shared codec and cache preparation.
Existing source checks and publication SHALL remain with the stem owner.

#### Scenario: The verified learned CPU release is unavailable

- **GIVEN** the request selected Native HTDemucs CPU
- **WHEN** the release record lacks an approved learned graph or the actual worker, graph, Job or runtime fails verification
- **THEN** the request fails and no new stem set is published
- **AND** it grants no Torch producer identity or completed-set reuse
