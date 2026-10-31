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
