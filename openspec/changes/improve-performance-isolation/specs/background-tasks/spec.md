## ADDED Requirements

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

#### Scenario: Components load while another PAD is played
- **GIVEN** one PAD is preparing components under a controlled slow reader
- **WHEN** another loaded PAD receives trigger or STOP
- **THEN** those controls can reach their ordinary admission path independently of the slow read
- **AND** same-PAD ordering, cancellation, immutable sharing and last-reader retirement remain correct

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
