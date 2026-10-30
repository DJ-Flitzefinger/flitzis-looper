## ADDED Requirements

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
