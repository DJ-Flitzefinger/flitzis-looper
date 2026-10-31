# Development Guide

This guide covers local development, validation, OpenSpec usage, and package
layout for the current project.

For performance controls use the [user guide](user-guide.md); for independent
Windows installation use the [Portable guide](portable-windows.md). Current
[architecture modules](architecture.md) explain implementation ownership.
Historical diagnostics and acceptance packets below retain their original limits
and unfinished gates. They do not activate FULL/pre-Rust/model-stage/Goal programs
or authorize unrelated implementation. Apply the actual current task and its
relevant checks. Public source defaults below are separate from explicitly
configured private developer resource routing.

## Branch And Repository

`main` is the integration branch. Use the task's managed writer checkout for
implementation and protect unknown local changes before switching or integrating.

Before repository edits:

```powershell
git branch --show-current
git status
```

By default the user handles GitHub pushes manually. Follow any explicit
task-specific commit/push authorization; it never grants remote setup, force-push
or credential changes. Workspace-local continuation rights stay in local memory.

## Setup

Run commands from the repository root:

```powershell
uv sync --locked
uv run --no-sync maturin develop --locked
uv run --no-sync python scripts/setup-runtime.py
```

On Windows the lock resolves Torch 2.12.0 and TorchAudio 2.11.0 from the explicit
official CUDA 13.0 index. CUDA wheels still support CPU execution. The local
`vendor/bs_roformer` distribution provides the pinned offline MUSDB18HQ worker;
it installs through `uv sync`, without installing training/model families.
Model assets remain an explicit separate installation. See
[stem setup](stem-generation-setup.md) for identities, driver verification and rollback.
Automatic BPM analysis requires the separately locked BeatThis worker. The setup
command above provisions and verifies it at ignored `models/beat-this/`, with
resumable acquisition/scratch in `.cache/runtime-setup/`; it opens no App or audio
device and performs no inference. Use `--check` for non-acquiring verification and
`--with-stems` to additionally acquire and CPU-load both optional separator models
at `models/torch/hub/checkpoints/`. Explicit managed developer model/cache overrides
remain supported. See [BeatThis setup](beat-this-setup.md).

On Windows, double-click `build-release.bat` to build the Release once, then
double-click the official `start.bat` entrypoint for normal starts. For direct
development source runs:

```powershell
uv run python -m flitzis_looper
```

Use `uv run cargo ...`, not plain `cargo ...`, so the Rust/PyO3 build uses the
project Python environment consistently.

Private B2 reference-first diagnostics run through
`python -m flitzis_looper.analysis.beat_scoring_cli`; see
[scoring workflow](beat-this-scoring-workflow.md) for source aliases, strict
plans, complete native lineage and explicit missing-input reports. They require
actual independent reference receipts for scoring and never start the app or
worker. Synthetic tests verify the contract without declaring musical acceptance.

After a genuine full reference seal is available, `draft --include-corrected-legacy`
adds explicitly supported QM comparators alongside selected Beat This candidates.
It preserves separate candidate/comparator selections and complete native provenance.
Neither a receipt hash nor installed binaries register a producer. Engineering
comparisons are pure explicit APIs with complete-array disagreements and unverified ordinal
fits. See [corrected legacy diagnostics](beat-this-corrected-legacy.md). The ignored
hardware-free native probe uses `FLITZIS_B2_LEGACY_CONFIG` and the existing Windows
Rust test runner. It starts no CPAL stream, app, device or model worker.

### Windows Start Files

Double-click a BAT file in the repository root, or call it from a terminal:

- `start.bat` is the official normal start: it runs the already installed Rust
  Release build without synchronizing dependencies, installing, or compiling.
- `build-release.bat` builds and verifies the Rust Release profile without
  starting the app. It accepts no arguments and keeps the result visible until
  a key is pressed, on both success and failure. No shortcut arguments need to
  be configured.
- `start-dev.bat` builds and starts the app with the Rust Debug profile.
- `start-release.bat` builds and starts the app with the optimized Rust Release
  profile.

All files run from their own repository directory, including when called from
another working directory or a path containing spaces. `start.bat` uses the
existing `.venv\Scripts\python.exe` directly. It checks the loaded native
module's `native_build_profile()` before starting the app. A missing environment,
missing extension/profile getter, import failure, or Debug build stops startup
with instructions to run `start-release.bat --build-only`. It never silently
builds or switches profiles.

All build entrypoints synchronize dependencies with `uv sync --locked`, install
the selected native profile with
`uv run --no-sync maturin develop --locked` (adding `--release` for Release),
and verify the installed profile. `build-release.bat` then reports completion
and waits for a key; the build-and-start files run
`.venv\Scripts\python.exe -m flitzis_looper`. Existing build artifacts are reused
by the build tools. All four root BAT files use `scripts/start-app.bat`, so the
dedicated Release builder uses the same build pipeline as the Release starter.
It preserves the actual setup, build or profile-check exit code after waiting.

For checks that do not open the app or an audio device:

```powershell
.\start-release.bat --build-only
.\start.bat --check
.\start-dev.bat --build-only
```

`--check` uses exactly the normal Release preflight and exits before the app.
`--build-only` performs the normal setup, native build and profile verification
and exits before the app. Both return nonzero failures without pausing.

Close all running Looper windows before using any build entrypoint: both profiles
install the same native extension, which Windows cannot replace while the app is
using it. A failed setup or build stops the launch. Normal double-click starts
keep the terminal open on failure to show the error. Only the build entrypoints
require `uv` on `PATH` and the native build setup described below; `start.bat`
uses the existing environment. Release still runs the Python source app; these
files build/run a source checkout. The existing independent
[Portable installer](portable-windows.md) bootstraps its own source/runtime/tools;
it is a separate entrypoint, not a binary bundle produced by these starters.

The accompanying command window also receives timestamped control and worker
diagnostics, with operation, pad number and available request identity. Technical
failures are retained there rather than inside panels or dialogs; ordinary loading,
preparation, confirmed audio state and retry controls remain in the app. Diagnostics
run on the Python control thread and never add logging to the audio callback.
Unchanged polling is deduplicated; a new failed user attempt is reported again.

## Native Rubber Band Dependency

The Rubber Band Key Lock backend depends on the native Rubber Band C API. The
repository should discover that dependency through platform-appropriate build
metadata or explicit environment variables. Production code must not hardcode a
developer's local vcpkg directory, Linux home directory, or other workstation
path.

### Linux

Prefer distro packages plus `pkg-config` when the distro package provides
Rubber Band's LiveShifter C API (`rubberband_live_*`):

```bash
# Debian/Ubuntu
sudo apt install librubberband-dev pkg-config

# Fedora/RHEL-like
sudo dnf install rubberband-devel pkgconf-pkg-config

# Arch-like
sudo pacman -S rubberband pkgconf
```

The Rubber Band Key Lock backend needs a package with LiveShifter support, such
as Rubber Band 4.0.0. Ubuntu 24.04 `librubberband-dev` 3.3.0 is too old for this
branch because its C header and shared library do not provide
`rubberband_live_*` symbols. For older distro packages, install a newer Rubber
Band package or source build and expose it through `PKG_CONFIG_PATH`, or use
`RUBBERBAND_LIB_DIR` together with `RUBBERBAND_INCLUDE_DIR`.

The normal Linux development path should allow:

```bash
uv sync
uv run maturin develop
uv run python -m flitzis_looper
```

If a developer uses a custom Rubber Band install prefix, keep that configuration
outside source code, for example through `PKG_CONFIG_PATH`, `LD_LIBRARY_PATH`,
or future project-documented override variables.

### Windows

For local Windows development, vcpkg is the preferred route:

```powershell
git clone https://github.com/microsoft/vcpkg.git "$env:LOCALAPPDATA\vcpkg"
& "$env:LOCALAPPDATA\vcpkg\bootstrap-vcpkg.bat" -disableMetrics
& "$env:LOCALAPPDATA\vcpkg\vcpkg.exe" install rubberband:x64-windows
setx VCPKG_ROOT "$env:LOCALAPPDATA\vcpkg"
```

Build discovery order:

- `RUBBERBAND_LIB_DIR` for an explicit library directory override.
- `pkg-config` for non-Windows system packages that expose the required
  LiveShifter C API.
- `VCPKG_ROOT`, or `$env:LOCALAPPDATA\vcpkg` on Windows when present.

Optional overrides are `RUBBERBAND_INCLUDE_DIR` for header validation,
`RUBBERBAND_VCPKG_TRIPLET` for a non-default vcpkg triplet,
`RUBBERBAND_LINK_KIND` for `dylib` or `static`, and
`RUBBERBAND_EXTRA_LIBS` for comma- or semicolon-separated extra linker inputs.

If `cl.exe` is not visible in the normal shell, use the Visual Studio developer
environment before building native dependencies:

```powershell
cmd /s /c '"C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\Tools\VsDevCmd.bat" -arch=x64 -host_arch=x64 && uv run maturin develop'
```

For source runs that link Rubber Band dynamically, the Python wrapper registers
Windows DLL directories before loading the native extension. It checks
`RUBBERBAND_DLL_DIRS`, `RUBBERBAND_DLL_DIR`, `VCPKG_ROOT`, the default
`$env:LOCALAPPDATA\vcpkg` location, and `PATH` entries that contain
`rubberband-3.dll`. For an explicit shell override:

```powershell
$env:RUBBERBAND_DLL_DIR = "$env:VCPKG_ROOT\installed\x64-windows\bin"
uv run python -m flitzis_looper
```

Standalone Rust test binaries do not import the Python wrapper, so
`os.add_dll_directory(...)` is not available to them. On Windows, run Rust tests
through the repository helper so uv's selected Python runtime directory and the
Rubber Band runtime directory are prepended to `PATH` before `cargo test` starts
the test executable:

```powershell
.\scripts\run-rust-tests.ps1
```

The helper asks `uv run python` for the active Python runtime location and its
`purelib`/`platlib` package directories. It prepends those package directories
to process-scoped `PYTHONPATH` so standalone PyO3 tests can import the same
NumPy environment as the app, and restores the caller's `PATH`/`PYTHONPATH`
after Cargo returns. For
Rubber Band, it uses `RUBBERBAND_DLL_DIRS`, `RUBBERBAND_DLL_DIR`, a `bin`
sibling of `RUBBERBAND_LIB_DIR`, `VCPKG_ROOT`, the default
`$env:LOCALAPPDATA\vcpkg` location, and existing `PATH` entries that contain
`rubberband-3.dll`. Extra arguments are forwarded to Cargo, for example:

```powershell
.\scripts\run-rust-tests.ps1 publish_loaded_sample_rejects_full_queue_without_cache_insert
```

Observed vcpkg runtime DLLs for the current branch are `rubberband-3.dll`,
`sleefdft.dll`, `sleef.dll`, and `samplerate.dll`. Packaging scripts may copy
those DLLs next to the built native extension instead of requiring `PATH`.

### Performance audit and upgrade plan

The 2026-10-30 source audit starts from `69a8098d` and is tracked in the focused
[performance change](../openspec/changes/improve-performance-isolation/proposal.md),
with its [source findings and measurement contract](../openspec/changes/improve-performance-isolation/design.md)
and [nineteen sequential steps](../openspec/changes/improve-performance-isolation/tasks.md).
Production response measurements and the first exact-ready scheduling correction
are recorded in the design. Completed ordinary worker starts and playback groups
now also accelerate within their initial fallback interval, removing 160 ms of
scheduling wait in the paired 48 kHz cold cases. The Human clarified on 2026-10-31
that 30 ms is advisory, not a mandatory acceptance threshold. Optimize for the
shortest practical latency with preserved quality/functions and proportionate
complexity. Constant independent Piano births now also complete within their
initial interval: the paired six-case probe removes 160 ms of cold scheduling
wait at 48 kHz while leaving warm/unity controls and the DSP attack unchanged.
Initial recording groups also use wholly ready completion with common capture S/E,
the original event order and immutable reserved STOP. Genuine successful off-RT
legacy reprepare now rearms one completion for the exact rebuilt E, releasing
displaced request pins only on that worker. Every replanned group member needs
that receipt before the common E/capture S moves. A conclusively rejected old
atomic-group member now propagates rejection through the peers' own guarded CAS,
without releasing custody or capacity early. A failed CAS remains a live barrier.
Live-original chains now use only this pass's successful complete fresh binds
and exact queued-event order, retaining captured gaps inside a callback and the
final predecessor barrier. Busy/unknown/unbound owners cannot acquire that proof.
Genuinely deferred groups now have a separate later-E bind for wholly held,
unconsumed FirstSound physical births with complete fresh requests and exact
metadata/capture/STOP/order guards. Occupied displaced pins, live predecessors
and unavailable proof retain the real worker path. No reset or heavy preparation
enters the callback. STEP11's bounded 36-case raw Short/Medium assessment retains
the current DSP: Medium's principal attack is later in every matched pair, while
nominal-delay cropping loses genuine response energy. The separate 511-frame
adapter packing opportunity needs a new physical-birth actual-feed/control/
retirement proof; it is not a measured or adopted speedup. See the design's STEP11
result for independent source references and limits. STEP12 implements the required
scalar-only approximate BPM fallback with a persisted inaccuracy warning; failed
precise evidence remains failed. STEP13 separates committed component reads from
owners. STEP14 extends this to source/window preparation with complete pre-I/O
byte/owner/reader/history promises, moved history custody and explicit historical
PCM accounting. Owned shared contexts release the Python Engine borrow before
reconciliation/binding, both registrations and admission; original successful-order
watermarks and final source/intent/CAS/own-ACK fences remain authoritative. The
worker-only gate ends before publication. Legacy full conversion has a separate
host promise alongside its unchanged cap. Conversion, resource policy, providers,
native inference/Torch parity and final checks remain steps 15–19. Controlled
progress is not whole-GIL isolation, device latency or crackle acceptance.
Recording's whole-group capacity failure remains terminal.
A separate ordering correction releases later ready originals from conclusively
rejected predecessors while retaining those predecessors' queued cleanup and
worker/Native custody. Live-original ordering and recording S/E guards remain.

The first priorities are the 8192-frame prepared-start scheduling lead, selected
endpoint reserve misses and audible onset; then stem component-loading control
locks, GIL-bound sample conversion and background CPU budgets. Finite/shared PCM,
existing offline CUDA and CPU fallback are foundations to measure and preserve.
Rust conversion is justified for sample arithmetic; wholesale controller/model
ports and callback CUDA have no demonstrated benefit at this point.
New Rust processing/domain code should remain independent of thin Python/PyO3
bindings. The plan also audits PyTorch replacement and dependency footprint for
further useful Rust migration and future Windows EXE/Linux/macOS packaging. Keep
common Rust logic portable, OS-specific process/device/resource handling behind
adapters and CPU as the baseline with hardware-conditional CUDA. Actual platform tests remain
distinct from design intent. AMD-capable GPU alternatives are also an explicit
conditional implementation target, with provider-based selection and CPU fallback.
This includes Intel/AMD integrated GPUs and automatic per-workload selection
from supported CPU/GPU paths using measured total cost and resource headroom;
GPU presence alone is not a reason to offload or claim a speedup.
NVIDIA CUDA support is mandatory for this upgrade and any inference replacement;
its use/install is hardware-conditional with CPU fallback. Cost/benefit discretion
applies to additional AMD/Intel/iGPU backends, not removal of CUDA support.
Practical PyTorch
removal is an explicit target, conditional on feasibility and at least current
model quality, functions and interactions. KeyNet already uses Rust/ONNX; stems
and separate BeatThis workers retain full Torch pipelines. Model/device/installer
parity is required before removing their existing inference runtime.

Measure original input, requested target T, actual E, significant sound and
principal attack separately. The ready 2/4/6-PAD quantization-OFF engineering goal
is at most 30 ms to significant rendered output, with no lost beginning or hidden
attack displacement. Declare the machine, rates, partitions and cold/warm/refusal
coverage. Intentional grid wait, device latency and listening acceptance are
separate. Include rapid unselected/slowed PADs and concurrent inference,
conversion and component loading; stress at most 10 PADs.

Use the production-route tests as well as the adapter probe below. Adapter/native
delay and Rust allocation counters alone do not establish whole-route latency,
native FFT allocation freedom or audio stability during background work.
For a machine-local routed workspace, follow its runtime entry with the actual
checkout; the generic commands below do not override local cache/storage rules.

### Offline Key Lock Measurement

The standalone `key_lock_latency_probe` example imports the production backend,
processor, and preparation pool. It reports construction/reset/warming costs,
Rust allocations, nominal native delay, and synthetic impulse response across
44.1/48/96 kHz, ratios 0.5/0.75/1/1.5/2, and fixed/irregular callback partitions.
The adapter probe samples one immutable source-domain impulse at absolute output-frame
positions, so callback partitions do not relocate markers or interpolation endpoints.
It measures startup and settled markers separately; the production source-path tests
add fractional BPM, loops, seeks and prepared stems.
The current engine constructs 96 unique handles for 32 voices: 64 effective/
neutral-reserve handles are warmed at setup; 32 source reserves receive exact
pitch/reset and actual source priming on the worker. Historical 64-handle startup/
memory figures retain their original scope; current
[96-handle setup/resource measurements](../openspec/changes/archive/2026-10-30-cache-full-pcm-and-resident-loops/evidence/pcm-cache-measurements.md) are separate.
The fixed wet adapter
lead remains 511 frames. Native reset/cold pitch work stays off the callback;
reserve starvation produces bounded wet silence, while dry processing remains
reactive. See [Key Lock backend](../openspec/changes/archive/2026-10-30-add-key-transposition-performance/evidence/key-lock-backend.md) for the allocation audit,
productive source-prepared ownership/timed adoption and the separate pending
audible delay/crop/mode-transition work.

From the repository root, with the documented runtime override set to the
actual Rubber Band DLL directory, run:

```powershell
$env:RUBBERBAND_DLL_DIR = "$env:VCPKG_ROOT\installed\x64-windows\bin"
$env:PATH = "$env:RUBBERBAND_DLL_DIR;$env:PATH"
uv run cargo run --release --locked --manifest-path rust/Cargo.toml -p flitzis-looper --example key_lock_latency_probe -- 24 |
    Set-Content -Encoding utf8 ..\scratch\slice3-key-lock-latency.csv
uv run cargo run --release --locked --manifest-path rust/Cargo.toml -p flitzis-looper --example key_lock_latency_probe -- pool 48000 32 |
    Set-Content -Encoding utf8 ..\scratch\slice3-key-lock-pool.csv
```

Use the applicable triplet or an explicit installed directory when `VCPKG_ROOT`
is unset. Keep generated measurements in workspace `scratch/` or `exports/`.
The first command accepts a preparation repetition count; the second measures
the bounded pool at a chosen sample rate and voice count. It starts no audio
device. Calling-thread Rust counters exclude worker and C/C++/FFT allocations,
API delay does not equal every transient peak, and offline timings do not prove
live deadlines or hardware alignment. Record startup and settled response
separately; the local baseline and final prepared results are recorded in
`scratch/slice3-key-lock-latency-findings.md`.

Focused production-path preparation tests require no audio device:

```powershell
.\scripts\run-rust-tests.ps1 --release --lib prepared_native
.\scripts\run-rust-tests.ps1 --release --lib native_history_permit
.\scripts\run-rust-tests.ps1 --release --lib key_lock_preparation
```

These exercise the actual worker, copied canonical trajectory/shared adapter,
native/FIFO ownership, exact absolute adoption deadline and source/timing/runtime
permits. Independent algebraic/raw-native suffix comparisons cover real shifted
content; cancellation/unload tests observe source-pin retirement without later
source rendering. They establish numerical ownership/continuation contracts,
not audible delay compensation, listening, device alignment or live deadlines.
The non-live diagnostic fixture below remains distinct from this productive path.

The test-only exact-source preparation proof compares a coherent native/FIFO
continuation with an independent source/raw-native reference. It also exports
uncropped translated impulse timing, retained timing and discarded peak/energy
evidence. Run the release proof without an audio device:

```powershell
$env:FLITZIS_KEY_LOCK_SOURCE_PROBE_CSV = (Join-Path (Split-Path -Parent (Get-Location).Path) 'scratch/slice3d-source-preparation.csv')
.\scripts\run-rust-tests.ps1 --release --lib key_lock_source_preparation
Remove-Item Env:\FLITZIS_KEY_LOCK_SOURCE_PROBE_CSV
```

The optional export path must be absolute. Ordinary test runs write no CSV.
The proof uses an explicit constant ratio and already accepted immutable source
buffers; asynchronous live adoption is excluded. The nominal delay is only an
experimental discard. Assess uncropped residuals and clipping alongside retained
output before selecting compensation; this run provides no device/deadline evidence.

Run the explicit-history impulse/tone/percussion sweep separately:

```powershell
$env:FLITZIS_KEY_LOCK_HISTORY_PROBE_CSV = (Join-Path (Split-Path -Parent (Get-Location).Path) 'scratch/slice3e-source-history.csv')
.\scripts\run-rust-tests.ps1 --release --lib history_probe
Remove-Item Env:\FLITZIS_KEY_LOCK_HISTORY_PROBE_CSV
```

The absolute optional path writes the per-candidate/partition CSV and a companion
`slice3e-source-history.summary.csv`. The summary intersects timing and exact
energy/peak retention bounds for one common integer translation across all
tested phases/signals at each rate/ratio/history. Ordinary tests export nothing.
History H and raw discard D are separate; CSV candidate offset is D-H, and raw
residuals compare against the actual dry response. The declared q10/q50 two-ms
engineering budget, retained peak/energy checks and capture-tail bounds are
explained in the backend guide. A failed intersection is preserved as evidence;
neither reference equality nor a sampled candidate selects live compensation.

Run the longer-history onset/content diagnostic independently:

```powershell
$env:FLITZIS_KEY_LOCK_ONSET_PROBE_CSV = (Join-Path (Split-Path -Parent (Get-Location).Path) 'scratch/slice3f-onset.csv')
.\scripts\run-rust-tests.ps1 --release --lib onset_probe
Remove-Item Env:\FLITZIS_KEY_LOCK_ONSET_PROBE_CSV
```

The optional absolute path writes named metric rows plus a companion group summary.
The bounded matrix uses H32768/H65536, ratios0.5/1/2, phases17/511, six jointly varied
tone/percussion duration/frequency/carrier-phase combinations, and silent/nonzero
periodic stereo history. Ordinary tests export nothing. Independently initialized
native/source references prove nominal continuation under 512/irregular partitions.
Raw and launched quantiles, peak/energy retention, nonlinear background sensitivity,
history stability, fixed 2-ms dry plus 5-ms fade diagnostics and unchanged wet suffixes
remain separate. Whole-history mixtures and paired differences are labeled context
or nonlinear diagnostics; their numerical verdicts are not audible attack acceptance.
The bridge changes pitch briefly and is not selected live behavior. See the backend
guide for the unchanged budgets, mathematical lower bounds and remaining content gate.

Run the fixed unity-source attack candidate separately:

```powershell
$env:FLITZIS_KEY_LOCK_PITCH_PROBE_CSV = (Join-Path (Split-Path -Parent (Get-Location).Path) 'scratch/slice3g-pitch.csv')
.\scripts\run-rust-tests.ps1 --release --lib pitch_probe
Remove-Item Env:\FLITZIS_KEY_LOCK_PITCH_PROBE_CSV
```

The test-only candidate reuses the fixed two-ms hold/five-ms fade, but reads its initial source
branch at ratio1 from the declared logical fractional phase. Independent source/native oracles
verify addressing, partitions and later wet continuation. The nonzero-history matrix includes
markers0/17/511; launch-local and independently mapped target-local windows are distinct.
Local mixture energy/envelope diagnostics and weighted-source/wet interference do not replace
the original raw-native retention/timing gates. A unit-rate source branch preserves its own
pitch while departing from canonical tempo progression; it is not selected live behavior.
Ordinary tests export no audio or CSV and start no device.

Run the fixed causal scheduling/content proof separately:

```powershell
$env:FLITZIS_KEY_LOCK_CAUSAL_PROBE_CSV = (Join-Path (Split-Path -Parent (Get-Location).Path) 'scratch/slice3h-causal.csv')
.\scripts\run-rust-tests.ps1 --release --lib causal_probe
Remove-Item Env:\FLITZIS_KEY_LOCK_CAUSAL_PROBE_CSV
```

The optional path must be absolute. The probe reuses existing source/history/native fixtures
and compares strict first-sound gating with hypothetical earlier native emission for fixed
E/T cases. Actual nonzero-mixture content accounting remains separate from original isolated
peak/99.9%-energy retention. Unequal partitions must retain exact canonical native suffixes.
No fitted bridge, new acceptance threshold, live policy, device or ordinary-test export is added.
The backend guide defines the raw coordinate mapping, causal bounds and pending product decision.

### Nuitka Installer Direction

This is a historical packaging direction, not the current installation procedure
or an active task. Current Windows installation uses the generic
[Portable source installer](portable-windows.md). The retained binary-bundle
proposal was:

The later Windows installer should be built so non-technical users do not need
development tools. The Nuitka packaging step should bundle the app, the PyO3
extension, the required Rubber Band runtime DLLs, and any MSVC runtime
requirements not otherwise guaranteed by the target system.

Do not commit generated Rubber Band DLLs, vcpkg trees, Linux `.so` files, or
Nuitka build output into the repository. Keep redistributable binary handling in
packaging scripts and release artifacts. Before publishing a binary installer,
confirm that Rubber Band's GPL/commercial licensing requirements match the
intended distribution model.

## Validation

### Checks at product boundaries

During implementation, use quick compile checks and focused regressions for the
changed behavior. Stabilize those checks before starting expensive broad suites.
At a coherent product block's end, run the impacted integrated checks, a Release
build/import/preflight when native code or packaging changed, and an independent
final review. Full workspace, both-profile and long musical/resource tests belong
to the integration gates they prove or to a concrete uncovered regression risk.
Do not repeat them after every internal edit or documentation update.

Reuse successful checks only when their relevant source, runtime and dependency
inputs are unchanged; distinguish reused results from fresh runs. Preserve every
required final acceptance gate and the original test tolerances. Workflow/docs-only
edits need consistency, links, whitespace and diff review rather than audio builds.
Synchronize dependencies when setup or lockfiles require it, then use
`uv run --no-sync` within the validation batch. Keep native installation serial
and do not replace an extension held by a running user app.

### Offline and device validation

Ordinary `uv run pytest` runs the full offline suite and skips tests that open a
real audio device. Use `uv run pytest --audio-devices` only for an explicitly
authorized human/device session. Native cold-load tests exercise the productive
worker and callback command drain with virtual PCM output and no CPAL stream.

C1b warm/lifecycle checks use the same productive loader, guarded ACK and native
retirement paths. Fresh warm leases perform complete source and decoder/playback
verification; their hashing CPU and I/O are part of the workload. Explicit private
probes record the actual 600-second source path and save/export costs:

```powershell
$env:FLITZI_COLD_LONG_SOURCE = (Resolve-Path -LiteralPath '..\exports\g3c3-startup-corrected-20261007\wholequarter-unity\samples\acceptance-source.wav').Path
$env:FLITZI_C1B_WARM_EVIDENCE = [IO.Path]::GetFullPath('..\scratch\c1b-warm-lifecycle-20261007\warm-evidence.json')
.\scripts\run-rust-tests.ps1 -CargoArgs @('c1b_productive_long_warm_integrity_cost', '--', '--ignored', '--nocapture')
$env:FLITZI_C1B_SAVE_EVIDENCE = [IO.Path]::GetFullPath('..\scratch\c1b-warm-lifecycle-20261007\save-evidence.json')
.\scripts\run-rust-tests.ps1 -CargoArgs @('c1b_actual_native_export_and_project_save_integrity_cost', '--', '--ignored', '--nocapture')
```

Use a fresh output filename for each profile/run. Native Debug/Release installation
and dependent tests run serially because Windows retains open extension handles.
The historical C1b warm source probe and 32-second save/export fixture provide
preliminary integrity accounting. Current 200-pad startup/resources and long-source
save costs are reported separately in [C3 measurements](../openspec/changes/archive/2026-10-30-cache-full-pcm-and-resident-loops/evidence/pcm-cache-measurements.md).

### C2a finite saved-loop and accepted-set relocation probes

The ordinary native suite covers productive saved finite loads at 44.1/48/96 kHz,
source/window ACK, cancellation/backpressure, complete evidence without complete
ticket pins, exact interpolation/rate oracles and live native-history preservation.
Run these two additional ignored probes separately in both Debug and Release;
they isolate their project directory and never open a stream or device:

```powershell
$env:FLITZI_COLD_LONG_SOURCE = (Resolve-Path -LiteralPath '..\exports\g3c3-startup-corrected-20261007\wholequarter-unity\samples\acceptance-source.wav').Path
$env:FLITZI_C2A_FINITE_EVIDENCE = Join-Path (Resolve-Path -LiteralPath '..\scratch\c2a-residency-20261007').Path 'finite-debug-evidence.json'
.\scripts\run-rust-tests.ps1 -CargoArgs @('audio_engine::cold_residency_tests::c2a_productive_saved_finite_warm_restore_identity_bytes', '--', '--ignored', '--exact', '--nocapture')
.\scripts\run-rust-tests.ps1 -CargoArgs @('audio_engine::cold_residency_tests::productive_worker_stem_relocation_keeps_complete_set_and_live_reader', '--', '--ignored', '--exact', '--nocapture')
```

Add `'--release'` to each CargoArgs list and use a distinct evidence filename for
Release. The long probe independently hashes complete PCM24 decoder/playback
content, verifies actual cold and fresh warm ACK, and compares two loop seams
against original PCM while only `[42,42.5)` seconds remain resident. The other
probe prepares real complete stem artifacts, adopts their finite views, then
relocates the same already accepted complete set through the productive worker
and native callback with uninterrupted PCM output. These are identity, ownership
and output proofs. [C3 measurements](../openspec/changes/archive/2026-10-30-cache-full-pcm-and-resident-loops/evidence/pcm-cache-measurements.md) report startup/RAM
separately; human acceptance stays open.

The optional Beat This boundary is available only through explicit diagnostic
API calls and does not change normal load/manual analysis routing. Its
unconfigured mode needs no Beat This installation; real B1b inference uses the
[separately locked worker and explicit setup](beat-this-setup.md). Use the
existing project build for native PCM/KeyNet methods. [Offline analysis
boundary](offline-analysis.md) documents the API, focused tests and resource
bounds. Keep installed runtime/model files outside the Git repository.

The lossless diagnostic reader and native codec have deterministic regression
coverage in the normal suite. An additional ignored private-evidence test checks
complete real worker envelopes from the workspace without copying them into Git:

```powershell
$env:FLITZIS_PUBLICATION_EVIDENCE_DIR = (Resolve-Path -LiteralPath '..\scratch\b2b1').Path
.\scripts\run-rust-tests.ps1 -CargoArgs @('complete_private_worker_envelopes_pass_native_publication_validation', '--', '--ignored')
```

That test requires the recorded `worker-T04`, `worker-T05` and `worker-R01`
request/final-envelope files. It proves native result validation, independently
of native long-track PCM admission and complete-job acceptance.

Focused changes should run focused tests. When a full integration gate or
concrete cross-system risk requires the whole sequence, use the commands below.
Select the existing environment/profile deliberately; dependency synchronization
and native installation are needed only when their inputs changed or setup is
missing. An unchanged Release installation should remain available to the user:

```powershell
uv sync
uv run maturin develop
uv run cargo check --manifest-path rust/Cargo.toml --workspace
.\scripts\run-rust-tests.ps1
uv run pytest
uv run ruff check src
uv run mypy src
git diff --check
```

On non-Windows platforms, or in a Windows shell where the required runtime DLLs
are already visible to test executables, the Rust test command is:

```powershell
uv run cargo test --manifest-path rust/Cargo.toml --workspace
```

Rust formatting:

```powershell
uv run cargo fmt --manifest-path rust/Cargo.toml --all --check
```

Python formatting:

```powershell
uv run ruff format --check src
uv run ruff format src
```

## OpenSpec

Behavior changes must update OpenSpec before or alongside implementation unless
the existing spec already fully covers a defect correction.

Use active change deltas under:

```text
openspec/changes/<change-id>/specs/<capability>/spec.md
```

Every changed requirement body should start with a direct normative sentence
using `SHALL` or `MUST`, and every requirement needs at least one
`#### Scenario:`.

Official validation:

```powershell
openspec validate <change-id> --strict
```

Fallback:

```powershell
cmd /c npx @fission-ai/openspec@latest validate <change-id> --strict
```

Do not use repository docs as a substitute for OpenSpec requirements.

### Hardware-free G3c loop evidence

The accepted-owner musical PCM/onset proof is reproducible without
starting the app or an audio device:

```powershell
.\scripts\run-rust-tests.ps1 -CargoArgs @('musical_loop_proof_tests')
```

Run the separate ignored strict musical-period acceptance probe explicitly;
an ordinary suite's ignored count does not pass that gate:

```powershell
.\scripts\run-rust-tests.ps1 -CargoArgs @('strict_musical_rendered_onset_acceptance_gate', '--', '--ignored')
```

The private actual-WAV ignored probe requires `FLITZIS_G3C_WAV` pointing to the
unchanged workspace `test-audio/metronom_120_BPM.wav` and
`FLITZIS_G3C_OUTPUT_DIR` under workspace `scratch/` or `exports/`. It also requires
the retained complete G2 gate input (default local path, or
`FLITZIS_G3C_GATE_INPUT`). Private inputs, JSON evidence and audio snippets are
never repository fixtures. See [the proof domains and remaining gates](loop-period-proof.md).

### Hardware-free C2b complete-context and residency evidence

Complete-source waveform/export/default-analysis tests and actual cold-worker
control transactions run without an app or device:

```powershell
.\scripts\run-rust-tests.ps1 -CargoArgs @('c2b_')
.\scripts\run-rust-tests.ps1 -CargoArgs @('resident_control_worker_tests')
.\scripts\run-rust-tests.ps1 -CargoArgs @('resident_transaction_parity_tests')
.\scripts\run-rust-tests.ps1 -CargoArgs @('resident_long_cycle_tests')
.\scripts\run-rust-tests.ps1 -CargoArgs @('stem_publication_tests')
uv run pytest src/tests/flitzis_looper/controller/transport/test_residency.py src/tests/flitzis_looper/ui/test_input_timing.py
```

The ready-trigger regressions hold both cold workers and all queue reservations
deterministically, then drive real ImGui mouse frames through the shared
controller, native ACK/guarded launch and PCM renderer for full mix and accepted
stems. Identical resident handles require ACK but no cold-lane admission or
stem reload; changed windows/context still require preparation. Repeated identical
pending starts retain their preparation and latest original input timestamp.
Controller/input unit tests separately check coalescing, outside-pad release and
bounded retries. These checks establish the input/readiness contract without
claiming acoustic latency, general resource performance or human/device acceptance.

Repeat native checks with `--release` and install the matching debug/release
extension before the Python suite, serially. The long-cycle test compares the
actual native ACK/render route with complete-buffer output and an independent
PCM/period oracle at 75 and 1000 observed cycles. It certifies those numerical
contracts only. [C3 process-memory/performance measurements](../openspec/changes/archive/2026-10-30-cache-full-pcm-and-resident-loops/evidence/pcm-cache-measurements.md)
remain separate from these numerical proofs and the open human device/listening
gates. Generated source and proof exports stay outside the repository.

### C3 measured startup/resource acceptance

The [C3 measurement report](../openspec/changes/archive/2026-10-30-cache-full-pcm-and-resident-loops/evidence/pcm-cache-measurements.md) records the isolated
Debug/Release productive controller cold/warm matrix, actual native readiness/ACK,
PCM and process-resource scopes, current 96-handle setup, complete-context
exceptions, long-source save and cancellation/cleanup results. Its paired
finite/current-full comparisons include regressions and single-run/OS-cache limits.

Full Debug/Release numerical validation remains a separate required gate; isolated
phase-origin-zero dry renders do not replace it. Actual human/device/listening
acceptance remains open in its retained historical packet. No pre-port execution
stage is active through this guide. C3 does not begin full
application Rust-port planning or implementation.

### G3c productive device and listening preparation

The [human-run acceptance packet](device-loop-acceptance.md) documents the opt-in
`productive_loop_packet prepare|run|request` commands and offline
`loop_capture features|compare|listening-receipt` workflow. Preparation and offline
commands do not instantiate the application or open a device. Only the human
invokes `run`; it uses the normal controllers/UI in an isolated private project,
with explicit native accepted publication and derived acknowledgement. It never
starts a pad or a recorder automatically.

Demand-only native snapshots retain effective current-bound voice values separately
from Python intent and CPAL estimated output-clock observations. JSON, source
hashing/export and capture analysis stay off the callback. Native/Synthetic tests
and valid evidence receipts do not certify actual device or sustained listening.

## Python Packages Under `src/`

There are two Python packages by design:

- `src/flitzis_looper/`: the actual application package. It contains
  controllers, UI rendering, models, persistence, settings, input mapping, stem
  orchestration, constants, and `__main__.py`.
- `src/flitzis_looper_audio/`: the import package for the native Rust extension.
  Its `__init__.py` re-exports the compiled module, `__init__.pyi` describes
  the native API for type checking, and `py.typed` marks the package as typed.

`vendor/bs_roformer` is a separately installable, versioned local inference
dependency. Its `network/bs_roformer.py` and `network/attend.py` retain exact
upstream bytes and MIT notices; `NOTICE.md` binds their identities. Owned worker
modules and tests are checked separately with Ruff (the unchanged upstream
network is reviewed as third-party source). Main `src/` checks retain strict
typing and lint policy; model/tensor imports stay in the subprocess package.

After `uv run maturin develop`, a generated platform extension such as
`flitzis_looper_audio.cp314-win_amd64.pyd` may appear in
`src/flitzis_looper_audio/`. This is a build artifact, not hand-written source.
Do not edit, move, or delete the package directory as a cleanup step.

On Windows, finish Python tests and app processes using that extension before
installing another debug/release build. A loaded `.pyd` cannot be replaced;
overlapping `maturin develop` installation fails with file-in-use error 32.
Build/install and runtime tests are serial steps; static source checks can run
in parallel. After `uv sync`, `uv run --no-sync ...` avoids redundant environment
resolution during one validation batch.

## Runtime Local Data

The [pad-owned PCM program](../openspec/changes/archive/2026-10-30-adopt-pad-owned-pcm-residency/evidence/pad-owned-pcm-program.md)
is an inactive historical contract with retained incomplete gates, not an execution
queue. Current [media/PCM](architecture/media-and-pcm.md),
[pitch](architecture/pitch-and-key-lock.md) and [stems](architecture/stems.md) describe
implemented ownership. New work needs its actual relevant checks and review;
documentation work does not run the App, CPAL or devices. Human hearing/device
acceptance remains separate. Existing recovery/path readers do not authorize new
migration or deletion of private data.

The app may create local runtime files:

- `samples/`: project-local copied samples, stem cache, and project config.

Current global and M-ID original/PCM/stem containers remain readable during the
verified serial transition. New canonical material uses `samples/<Originalfilename>/<Originalfilename>`,
`.pcm-cache` and `stems`; #N is stable slot membership. Final acceptance requires
material-wise verified migration and no obsolete legacy-container dependence; the
canonical shared material store is intentional, without Copy-induced duplicates. Do not move/delete current runtime assets as a documentation
cleanup or confuse old root preservation snapshots with current session state.
Hidden `.material.json` and `.material-claims/` carry guarded full-content lookup;
they do not supply native runtime ACK or physical cleanup permission. Automated
validation uses isolated fixtures, never private project migration. See
[readable material validation](../openspec/changes/archive/2026-10-30-adopt-pad-owned-pcm-residency/evidence/readable-material-validation.md).
Optional durable native birth receipts restore exact root/marker ownership only
after current config/journal, inventory and fresh Native adoption checks. Existing
schema/path recognition alone never authorizes cleanup; the inventory gate stays
held through actual last-reader physical terminal. Recovery after verified marker
deletion uses a separate typed retired-original path: exact birth root identity,
absent original, genuinely empty root and unchanged latest created-target
journal's actual original-terminal/current config/intent/inventory evidence.
Present markers still require their birth FileID/full SHA; ordinary resolution
does not accept missing markers or create adoption authority. Unknown/replaced
data stays protected. The six-mode process-exit/fresh-child terminal exercise
and current complete Debug/Release suites pass. That report records the bounded
evidence separately from full predecessor, copy/move/new-generation and Human
acceptance, whose gates remain open.
- `config/input/`: local input mapping JSON files.
- `.pytest-tmp*`, `.pytest_cache`, `.ruff_cache`, `.mypy_cache`, `.venv`, and
  `rust/target/`: tool/build artifacts.

Do not confuse generated runtime data with source documentation or OpenSpec.

Supported stem producer reuse also requires execution to match the guarded local
sources. The current Demucs37/BS33 file sets fit Native's bounded 64-file guard and
include the bounded loaded-code policy and empty host package initializer. Parent
verification compares actual relevant code, supported wrappers/defaults/constants/
import aliases with compiled held source. Captured package roots enter the
immutable worker request, come
first in child `PYTHONPATH` with `PYTHONSAFEPATH=1`, and each attempt owns a fresh
empty bytecode namespace with writes disabled. Required unsupported bindings or
stale code leave compatibility UNKNOWN. Keep normal dependency-version/OS policy
limits explicit; these checks are not global interpreter attestation. See the
MATERIAL report for independently reviewed current fixture/child evidence and
the separate full integration/Human gates.

## Documentation Policy

Use `docs/README.md` as the maintained documentation map.

Keep `docs/architecture.md` current for technical architecture and ownership.
Keep product behavior in OpenSpec. Remove or rewrite completed planning prose
when it no longer helps future work.

## TODO List Policy

`docs/todos.md` is the maintained project TODO list for explicit user-requested
notes.

Rules:

- Add items when the user asks to record a TODO.
- Work from it only when the user asks to choose from, review, or complete a
  TODO item.
- Check off or remove completed items when that TODO is implemented or
  intentionally abandoned.
- Do not use TODO items as a substitute for OpenSpec. User-visible behavior
  changes still need specs, tests, and validation.

## R0 documentation validation boundary

The following is the original historical R0 delivery boundary. It preserves its
exact evidence/check obligations; it is not the current documentation workflow
or authorization to dispatch J0/P1. Use the proportional validation policy above
for the actual task.

R0 changes official plans/deltas/docs and local handoff only, with no build/install/
runtime suite or P0 re-audit. Validate each actually affected active change using
real `openspec validate <id> --strict`; retain genuine failed attempts/exits and
unaffected historical strict FAIL/archive INFO separately. Check full seven-group/
56+9 coverage,38 IDs/48 edges, affected links/diff/source/input preservation.
Actual author terminal and distinct native nonauthor final semantic/raw/canonical/
index/blob/tree review precede publication or J0/P1 dispatch. The extended program
owns later complete serialized runtime validation; devices/hearing remain human.

## Recording and local data

The [E11 plan](../openspec/changes/archive/2026-10-30-adopt-pad-owned-pcm-residency/evidence/pre-rust-extension-20261011.md) separates capture, format and
performer-control delivery gates. Native capture now provides bounded exact-segment
float spooling and truthful terminal/recovery metadata. A spool is an unverified
candidate, not a successfully published recording.

Global recording defaults are immutable `RecordingOptions` values in `ProjectState`,
edited through Settings and ordinary Config persistence: WAV float32, FLAC 24-bit
at compression 8 and MP3 CBR 320 kbit/s by default. The Settings controller replaces
the value for future takes without audio calls; the active take owner retains
its original value. Missing or invalid recording fields recover only their own
defaults, preserving valid sibling preferences and unrelated pad intent. The visible
`recording_settings_warning` uses existing console deduplication and does not create
or clear the separate unsafe-metadata `load_error` save fence.

RECORD sits to the left of START/STOP. A right press starts capture of the current
mix without changing pad playback. A left press retains the active or remembered
global group at the original input timestamp, prepares the selected writer and
codec, then attaches that same capture to the existing all-or-none Native group
start. Either press stops an effective recording. Preparing and Finalizing retain
the sole operation owner; failures require explicit acknowledgement after actual
retirement. The ordinary START/STOP control remains independent.

Each take uses a unique directory under `record/`, relative to the application's
startup working directory, and `/record/` is ignored by Git. A custom Config path
does not relocate this directory. Actual selected-codec preflight precedes start;
the off-realtime worker independently checks complete encoded output before a
non-replacing final link publishes it. WAV can use RF64 for long output. Startup
discovery only reports owned incomplete candidates; it does not automatically
repair, encode, publish or delete them.

The App polls recording through the ordinary runtime tick, including Settings
and closing. Its first close request fixes a 30-second deadline; later frames do
not extend it or release audio/source ownership early. Original stream failure is
observed independently of the capture's first error. Checked callback closure and
retirement-worker join permit original orphan settlement, while actual recording
worker return, live-spool custody release and file verification remain separate.
A known stream failure immediately clears the effective recording indication,
even if the last worker snapshot still describes an active capture.

The bounded Native, codec, long-file and controller checks are recorded separately
from complete App/device/listening acceptance in the existing change and test log.
Existing M-ID/legacy data remains supported until verified readable-root migration
and fresh current Native ACKs complete; documentation updates never move private
runtime files.

## Latest performance workload decision — 2026-10-24

The direct Human instruction sets2/4/6 simultaneous PADs as the productive
optimization workload and10 as the maximum synthetic stress cohort, run directly
and unconditionally. All older20 targets/20-to-conditional10 recipes below are
superseded. Preserve their measured positive AND negative evidence unchanged as
history; no further20-PAD optimization is required. Current4/6 misses remain real
until corrected evidence resolves them. Internal32 atomic/capacity/retirement and
layout correctness stays separate and unchanged. No hard10 app cap or device/
7–10 performance guarantee is introduced. The user-facing guidance is in the
[manual's performance section](user-guide.md#memory-and-performance) and
[root introduction](../README.md). Other original FULL-Goal gates remain
unfinished historical requirements where recorded; no FULL execution program is
activated by this guidance.
