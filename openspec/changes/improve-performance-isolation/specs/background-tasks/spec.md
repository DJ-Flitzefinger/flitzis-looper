## ADDED Requirements

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

Windows child commit ceilings SHALL deduct shared Native prospective promises,
retain existing memory floors, be installed before suspended activation and remain
owned through actual tree and pipe-reader retirement. A ceiling SHALL NOT claim a
model-memory estimate, physical working-set bound or whole-system guarantee.

#### Scenario: Generation competes with playback or analysis
- **GIVEN** an original source-owned generation remains current
- **WHEN** playback is active before inference admission
- **THEN** inference waits visibly without stopping any PAD or changing its original request
- **AND** saved-set lookup and component preparation retain independent admission
- **WHEN** playback becomes active during an admitted generation
- **THEN** that attempt reports a distinct playback-headroom failure and retains custody until actual cleanup
- **AND** a waiting precise analysis retains its original quality and cancellation authority

#### Scenario: A child has less available commit than physical memory
- **GIVEN** caller/system commit and physical headroom are measured separately
- **WHEN** the shared Native ledger grants a child ceiling after existing promises and original floors
- **THEN** the Windows job enforces that ceiling before child activation
- **AND** cancellation, allocation failure and launcher exit do not release its promise before actual tree/reader end
- **AND** unsupported platform enforcement and unmeasured device deadlines remain explicit

#### Scenario: A queued stem becomes ineligible for memory admission
- **GIVEN** a current inference, requested pair preparation or held reuse delivery owns an inactive common FIFO position
- **WHEN** the existing stem resource guard reports pressure or unavailable monitoring
- **THEN** the controller SHALL withdraw only that inactive position so an otherwise admitted background owner can progress
- **AND** the original request, interests, source custody and held delivery remain pending without failure or cancellation
- **AND** recovery rejoins the same owner behind existing FIFO waiters under the unchanged memory thresholds
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

### Requirement: Separator Diagnostics Cannot Become Protocol Receipts
The host SHALL preserve the distinction between diagnostic text and exact child
protocol lines while compacting or truncating command output. Whitespace
normalization SHALL NOT manufacture encoder, BS input, completed-execution or
CUDA-failure evidence. Genuine independent receipts SHALL remain available, and
duplicate or oversized receipts SHALL remain invalid.

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
