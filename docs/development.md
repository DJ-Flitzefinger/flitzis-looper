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
git -C "$env:LOCALAPPDATA\vcpkg" checkout 020c8202737fb3ae3bfef5dd78ca537137be0ea0
& "$env:LOCALAPPDATA\vcpkg\bootstrap-vcpkg.bat" -disableMetrics
& "$env:LOCALAPPDATA\vcpkg\vcpkg.exe" install rubberband:x64-windows "--overlay-ports=$PWD\scripts\native\vcpkg-overlay"
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
native inference/Torch parity and final checks remain steps 15–22. Controlled
progress is not whole-GIL isolation, device latency or crackle acceptance.
Recording's whole-group capacity failure remains terminal.
A separate ordering correction releases later ready originals from conclusively
rejected predecessors while retaining those predecessors' queued cleanup and
worker/Native custody. Live-original ordering and recording S/E guards remain.

The first priorities are the 8192-frame prepared-start scheduling lead, selected
endpoint reserve misses and audible onset; then stem component-loading control
locks, GIL-bound sample conversion and background CPU budgets. Finite/shared PCM,
existing offline CUDA and CPU fallback are foundations to measure and preserve.
STEP15 now runs bounded sample arithmetic in an independent Rust core through
thin detached bindings, with the established Python WAV/commit chain and exact
byte oracles. CPU/process budgets remain the next isolation task. Wholesale controller/model
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
principal attack separately. For ready 2/4/6-PAD quantization-OFF workloads, 30 ms
to significant rendered output is an advisory reference, not a mandatory acceptance
gate. Seek the shortest practical response without lost beginnings or hidden attack
displacement. Declare the machine, rates, partitions and cold/warm/refusal
coverage. Intentional grid wait, device latency and listening acceptance are
separate. Include rapid unselected/slowed PADs and concurrent inference,
conversion and component loading; stress at most 10 PADs.

The explicit `paced_production_group_response` diagnostic captures each PAD's
actual contribution in the same mono render and checks its sum against the mixed
output. Its 12 cold/warm, one/two-wet combinations report individual burst landmarks,
host brackets, raw callback spans, deadline overruns and pacing lateness. Timings
include the test fixture, control drain and capture overhead; each combination has
one input start. Callback-duration percentiles do not establish onset percentiles,
device latency, concurrent background-work acceptance or the whole final matrix.
Here warm means selected PAD 0 is ready, not that all group members are prewarmed.
Principal displacement uses the authored source burst; a matched-rate Key Lock OFF
comparison remains separate. See the measured scoped result in the
[performance design](../openspec/changes/improve-performance-isolation/design.md#measured-scoped-result).

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

The opt-in `native-inference-trial` analysis feature now provides a standalone
BeatThis complete-source mono PCM trial, separate from the application. Its native
HQ resampler, LogMel, true variable attention lengths, border6/keep_first and
minimal postprocessor have bounded original-worker comparisons; the original
fixed1500 evidence remains separately recorded. The
[performance design](../openspec/changes/improve-performance-isolation/design.md)
records its exporter/tooling lock, exact native-logit/provider evidence and
remaining CUDA/cancellation/quality/distribution gates. It has no PyO3 or
LibTorch dependency. This is a developer experiment; productive Torch workers,
CPU fallback, normal installation and recovery remain required. A Windows x64
Debug EXE is not a self-contained portable bundle or Linux/macOS acceptance.

The trial's complete-source admission now matches512MiB/768kHz and the original
250000-prediction ceiling. Six native resampled/spectrogram pairs remain byte equal;
two high-rate original comparisons and ten early refusals pass without new model
references. A real CPU termination has164 same-run kernel events and actual
tree/reader settlement. CUDA initialization attempts remain unsuccessful with
cause UNKNOWN. The optional arena-limit receipt/verifier is isolated diagnostics,
not productive fallback policy. Every unproved native parity/producer/platform/
distribution gate continues below; no productive cutover is established.

Standalone dynamic trials now require an explicit runtime manifest before ORT
initialization. Build `native-inference-dynamic` for CPU-only dynamic loading or
`native-inference-cuda` for the CUDA candidate; both remain opt-in. The independent
`runtime MANIFEST SHA256` command checks runtime files/version/API and commits
environment options without loading a model. For `infer`, set
`FLITZIS_TRIAL_ORT_MANIFEST` and `FLITZIS_TRIAL_ORT_MANIFEST_SHA256` to that same
owned manifest and its exact lowercase SHA256. Ambient `ORT_DYLIB_PATH` is not
the dynamic trial's authority. Static CPU trials reject dynamic configuration.
The manifest schema and reconstruction recipe are in the
[STEP21 design](../openspec/changes/improve-performance-isolation/design.md#step21-impact-explicit-standalone-runtime-admission).
Checked files do not establish a complete loaded-image/dependency closure or an
installable bundle. STEP22 releases serialized graphs only after the copying
session loads and original PCM only after full resampling, with the original
source length/duration retained. It does not establish OS memory release, a lower
peak or CUDA success; no CPU NN is repeated for lifetime or binary hashes.
STEP23 optionally admits a separate declared ORT package component manifest with
`FLITZIS_TRIAL_ORT_PACKAGE` and `FLITZIS_TRIAL_ORT_PACKAGE_SHA256`. Every listed
file is size/SHA/role/identity checked and pinned through all CLI readers. The
strict schema, bounds and reconstruction recipe are in the
[STEP23 design](../openspec/changes/improve-performance-isolation/design.md#step23-impact-declared-ort-package-file-custody).
Listed-file custody does not attest loader resolution or complete dependencies.
STEP24 adds optional `FLITZIS_TRIAL_ORT_LOAD_POLICY=package-system32` for a fresh
Windows dynamic standalone process with both manifests. It irreversibly changes
that child's default DLL search policy, restricts the initial runtime load, and
checks actual returned/export-owner modules and observed declared backing files
against full file identities/SHA. Pins and module references remain until process
exit; no CUDA provider is eagerly loaded. Legacy custody-only requests retain
their behavior. See the [STEP24 design](../openspec/changes/improve-performance-isolation/design.md#step24-impact-opt-in-standalone-windows-loader-and-observed-images)
for the exact policy, terminal failures and still-unknown closure/producer limits.
STEP25 adds an independent `flitzis-stem-dsp` crate for the pinned BS-RoFormer
STFT/features/complex masks/ISTFT, with the original complete chunk length.
Its [CPU Torch arithmetic oracle](../scripts/bs_spectral_trial_reference.py) uses
synthetic PCM and four distinct complex masks without model/weight/CUDA work.
See the [STEP25 design](../openspec/changes/improve-performance-isolation/design.md#step25-impact-independent-bs-spectral-frontend-and-reconstruction)
for complete geometry, payload promises, cancellation, numerical tolerances and
reconstruction. This standalone crate is not linked into the App; its workspace
lock entry only uses the existing `rustfft` dependency. Productive installation/
repair, Python/model locks and workers remain unchanged. STEP26 adds bounded
native [wholetrack orchestration](../rust/crates/stem-dsp/src/track.rs): release
reflection, tail padding, batch2, per-chunk fades and rolling overlap-add. Its
[original arithmetic oracle](../scripts/bs_track_trial_reference.py) uses the
unchanged vendored source/OLA/PCM16 path with distinct arithmetic predictions;
it loads no learned weights and initializes no CUDA. See the
[STEP26 design](../openspec/changes/improve-performance-isolation/design.md#step26-impact-independent-bs-wholetrack-orchestration).
STEP27 adds the opt-in `stem-dsp/learned-trial` complete learned CPU candidate:
fixed-schema Torch-free export of the original SHA-pinned checkpoint, full
release axial attention with bounded score tiles, and four learned mask heads.
The [learned reference generator](../scripts/bs_learned_trial_reference.py) needs
the original complete offline admission and `CUDA_VISIBLE_DEVICES=-1` before
import. Run it as `python -B -m scripts.bs_learned_trial_reference` from an
authenticated checkout. The private `bs_learned` executable can compare original-feature masks
and composed spectral/model/PCM; it never publishes application artifacts. See
the [STEP27 design](../openspec/changes/improve-performance-isolation/design.md#step27-impact-independent-learned-bs-cpu-network).
The standalone CLI accepts an invocation-specific `FLITZIS_STEM_CANCEL_PATH`.
Creating that marker requests cooperative cancellation through preparation,
numerical callbacks and block I/O. After checked file cleanup and private-output
deletion it emits one `FLITZIS_STEM_CANCELLED_V1:{"stage":"..."}` line and exits130.
Cleanup/terminal failure suppresses that qualified outcome. The empty private
directory remains; no application output is published. Blocking native calls
still require supervisor escalation. ACK never replaces actual tree and reader
retirement. `--cancellation-control NEW_PRIVATE_DIRECTORY` exercises this protocol
without weights or a neural network; its Windows regression is opt-in through
`FLITZIS_BS_CANCELLATION_EXE`. Production Python inference recognition is unchanged.
BS diagnostic terminal output now owns two noninheritable duplicates of the
inherited Windows handles. Its ASCII writes use no stdio lock, LineWriter or
console conversion, and each transfer is at most8192 bytes. The pinned Rust File
path retains pending-I/O buffer safety; unsupported asynchronous handles may
still abort. Both duplicates receive one checked consuming close. Runtime-owned
original handles and post-main std/CRT cleanup remain separate. Exact private
output paths survive status writing, terminal flush and both closes; failure
invalidates only those files and returns1. A marker first observed after terminal
closure also invalidates and returns1 because no cancellation ACK can be sent.
Exit0/130, actual tree END and both supervisor pipe-reader END remain distinct.

This reduces a real owner dependency without establishing a finite successful
WHOLE-process allowance. CRT argv/environment heaps, native API internals, TLS,
loader, images, stack, panic and exit still lack a complete prospective contract.
The existing arena and its page backing describe one owner and cannot be added
again as process margin. No admission floor, numerical geometry or productive
Torch path changes; full original admission/parity/CUDA/platform gates remain.

Both standalone BeatThis entries use the same exact invocation marker and a shared
owner for input/hash, frontend, runtime preparation, ORT runs and output completion.
Session loading uses the actual retained builder's LoadCanceler. Load and run
termination helpers join before profiling and builder/session release. Unknown
load errors retain ordinary failure classification; a run error qualifies as
cancellation only for the exact native termination status after successful own
termination. Profile
files live in a newly created private directory under the requested prefix; the
returned profile path identifies the actual file. Primary errors and cleanup
qualification are independent. Only exclusively created files are invalidated,
including after late cancellation or terminal failure. ORT's process-global module,
directory and file guards intentionally remain until actual process exit.
`beat_this_audio --cancellation-control SOXR SHA PCM RATE NEW_JSON` runs real
nonneural frontend work with private partial output. Its opt-in Windows regression
uses `FLITZIS_BEATTHIS_CANCELLATION_EXE` and `FLITZIS_BEATTHIS_SOXR`. This control
proves neither learned/mid-ORT/CUDA execution nor Native/Torch cutover. Synchronous
native loading, session construction, I/O and close have no bounded interruption
claim; the owning supervisor still establishes tree and both pipe-reader END.
STEP28 retains ALL native parity requirements and FINAL29 the ENTIRE former
FINAL28 regression scope, including every earlier unproved gate. No productive
Torch dependency or installer/recovery path is removed by this trial.
The actual original oracle failed importing Torch and both full-geometry native
completion attempts failed allocation. Export/schema/operator controls pass;
complete learned execution and original numerical parity remain unproved.

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

## Independent HTDemucs spectral arithmetic

`flitzis-stem-dsp::demucs` now supplies callable Demucs 4.0.1 HTDemucs
`_spec` and scale-zero `_ispec` arithmetic without Python or a tensor runtime.
It uses the original 4096-point normalized FFT, 1024-sample hop, periodic Hann,
short-input zero-before-reflect padding, discarded Nyquist, time-frame cropping
and exact final physical length. CaC storage is `[lane, real/imag, frequency,
time]`; reconstruction takes estimated complex spectra directly. Flattened lanes
can represent batch/source/channel axes. This is before statistical neural
normalization, with FFT normalization already applied.

An exclusive reusable workspace keeps FFT work and a 4096-sample inverse ring;
analysis and reconstruction allocate no vectors after preparation. Checked
geometry bounds logical PCM/CaC/work payload to 512 MiB. FFT plans, allocator
overhead, model/caller owners and native/runtime backing need separate accounting
and actual admission. Cancellation or any error invalidates the entire caller
output. Immutable inputs, output storage and external reader/admission leases
remain owned through actual return. A Rust return is not process or peer-reader
END. This offline core has no application dispatch or realtime integration.

The `demucs_spectral` example bridges little-endian float32 stdin/stdout solely
for arithmetic controls. Its supervisor owns private files, complete process
settlement and final readers; failed writes/flush/exit invalidate the whole output.
Build the example once and run `scripts/demucs_spectral_trial_reference.py
--executable <built-example> --output <new-private-directory>` with the pinned
Demucs environment. It calls unchanged original methods without constructing a
model or loading weights. Ten complete length/tail comparisons passed, including
the released 343980-sample segment, eight independent inverse lanes and a longer
705601-sample source. Maximum spectral and inverse PCM differences were
`4.77e-7` and `3.58e-7`; these controls establish arithmetic only.

Learned Demucs, waveform-branch composition, statistical normalization, training
padding, whole-track split/overlap, shifts/bags, resampling, model/export/runtime,
CUDA/fresh CPU, complete producer/platform/distribution/quality and footprint
acceptance remain open. The productive Torch path remains active with zero
installed savings. The separate finite successful BS whole-process allowance and
all original learned-mask/PCM/AMP/tail gates remain unchanged.


### Independent Demucs host preparation and physical track reconstruction

`stem-dsp::demucs::host` supplies synchronous, one-batch-item host arithmetic.
`Statistics` separates CLI channel-mean reference from flattened per-item HT
branch statistics. Standard deviation uses correction one. CLI has no epsilon
(and refuses undefined singleton/constant references); HT normalization uses
1e-5, while source restoration always uses raw std. Independent deterministic
f64 two-pass reductions round mean/std to f32 before elementwise operations.
They implement source mathematical semantics, not Torch device-dependent
vector/Welford reduction topology. Actual high-DC/tiny-variance controls FAILED
original normalization parity (maximum branch0.09281, CLI0.16128 difference;
unchanged arithmetic atol/rtol2e-5). The explicitly independent measurement APIs
remain diagnostic. `from_reference` validates caller-supplied original statistics
for standalone normalize/restore controls; it does not establish their provenance
or supply the missing original-dispatch reduction. Complete independent native
normalization compatibility remains OPEN; no full learned parity is claimed.

`Chunk` reads centered context from the complete immutable source, zeroing only
beyond whole-source edges. An explicit shorter valid segment is then separately
right-zero-padded to training length. `TrackWorkspace` retains the complete
prediction until callback return, crops the right training pad before asymmetric
center trim, and adds original normalized triangular weights and predictions in
chronological order. The final chunk uses its actual short physical length.
Segment samples retain original int(rate*segment), stride uses f64 overlap math,
and transition power acts on f32 weights. No BS track math is substituted.

Preparation fallibly allocates host vectors; run has no vector allocation. The
512 MiB checked logical payload includes source, complete caller output, full
weight envelope, triangle, and training input/output buffers. It excludes model,
FFT plans, allocator and native/runtime backing and is not prospective WHOLE
admission. The caller holds immutable source readers, exclusive output and
admission through every actual synchronous callback return. Every model output
scalar must be written; the first detected error, nonfinite value or cancellation
(including the final check) invalidates the entire result. Reuse starts fresh.

The standalone `demucs_host` bridge and original-method control script isolate
host arithmetic with deterministic context-sensitive callbacks, not learned
models or weights. Unchanged FFT evidence is reused. There is no productive
Torch dispatch change, realtime integration, process/reader-END claim or runtime
completion protocol. Random shifts/averaging, weighted bags, resampling, complete
learned spectral/waveform composition and exact affected numerical compatibility
remain open, as do all original CPU/CUDA fault-to-freshCPU, producer, platform,
installer/recovery, quality and footprint gates and every FAIL/UNKNOWN.
Torch remains active with zero installed savings; finite BS WHOLE stays open.


### Supplied-offset Demucs shifts, no-split and weighted bags

The callable `demucs::host::apply::Workspace` composes model-major bags, each
model's independent supplied shifts, each shifted split and the existing model
boundary. Shift coordinates use the original whole padded source (L+44100),
inclusive offsets 0..22050, logical length L+22050-offset, and crop22050-offset.
Nested splits retain the entire backing context. Empty offsets mean shifts=0.
No-split uses one fixed, independently established model valid/training boundary;
it refuses a logical extent beyond valid length. Arbitrary dynamic valid_length
methods and original Python random generation/dispatch are not supplied.

Shift sums and division retain chronological f32 arithmetic. Every model has
separate segment/training lengths and offsets. Bags require identical ordered
source names, rate and channels; per-source weights multiply model estimates
before chronological bag addition. Totals accumulate in original Python-f64
order and convert to f32 for final tensor-style division. Nonfinite weights or
results and zero/nonrepresentable denominators are refused. Negative finite
weights with valid nonzero totals retain original arithmetic.

Preparation checks all simultaneous source/output, padded source, shifted result,
model estimate, per-model envelopes/triangles/training buffers and numeric
controls against the logical512MiB limit before allocating workspaces. Runs
allocate no vectors. This is not learned/runtime/allocator/process WHOLE admission.
Immutable inputs/model controls/readers, exclusive output and actual admission
survive synchronous callbacks; first error, unwritten output, nonfinite or final
cancellation invalidates the complete run. Reuse clears sums.

`demucs_apply` and `demucs_apply_trial_reference.py` compare complete original
apply_model outputs and callback order with deterministic nonneural callbacks
and lexically supplied original random draws. They establish host arithmetic,
not a learned model, random host, device transition, cancellation runtime or
Torch replacement. Independent High-DC normalization parity remains FAIL; no
unchanged statistical/FFT replay is required. Full normalization, productive conversion,
learned branches, producer/runtime, CPU/CUDA/fault-to-freshCPU, all platforms,
distribution/recovery, footprint and quality remain open. Torch stays active
with zero installed savings; BS finite successful WHOLE and every retained
FAIL/UNKNOWN remain unchanged.

### Independent Demucs audio conversion

`stem-dsp::demucs::audio` implements Demucs 4.0.1 channel conversion and the
Julius 0.2.7 rational FIR. Use `Channels` or `ResampleFrac` separately, or
`ConversionGeometry` and `Converter` to convert channels before resampling.
Geometry checks the combined source, intermediate (when needed), kernels and
output against 512 MiB before allocation. Equal rates use direct channel
conversion; for differing rates, equal channel counts borrow the source. Kernels are reusable and
replicate padding is virtual. The physical output preserves Julius's f32
length rounding, phase ordering and final tail. The bounded planar f32 API
refuses empty/nonfinite input and oversized kernels, including some coprime
ratios within the supported 1–768000 Hz rate range. No caller may publish any
output after error or cancellation, or release borrowed readers before return.

The `demucs_audio` example and `scripts/demucs_audio_trial_reference.py` compare
complete kernels and PCM with the pinned original CPU operators at `atol=rtol=2e-5`.
Independent f32 reductions and trigonometry do not promise bitwise equivalence.
The normal Demucs CLI tries FFmpeg first; Julius is the torchaudio fallback's
conversion path. Decoding, productive routing, independent High-DC normalization
parity, and learned/runtime/device acceptance remain open. See the
[upstream notices](../rust/crates/stem-dsp/THIRD_PARTY_NOTICES.md).

### Native HTDemucs eval composition

The offline `flitzis_stem_dsp::demucs::hybrid` boundary accepts finite planar
`[B,C,L]` float32 PCM at 44.1 kHz and a caller-established training length.
It pads on the right and uses the existing normalized FFT and CaC layout.
The caller supplies separate original-dispatch mean and correction-one standard
deviation per batch item. The spectral axes are channel, complex part, frequency
and step. The waveform axes are channel and padded time. The predictor receives
both inputs normalized with `1e-5 + std` and supplies complete normalized decoder
tensors in batch, source and channel order.

The boundary restores both tensors using the raw standard deviations and means.
It reuses scale-zero CaC inverse reconstruction with one training-length lane
buffer, adds the restored waveform over the entire training extent and copies
the original-length prefix. The split/apply host owns surrounding context and
any center trim. Unwritten or nonfinite predictions, callback errors and
cancellation invalidate the entire result. The logical 512 MiB preflight includes
the simultaneous borrowed input, output and statistics, the branch vectors, the
maximum FFT vector workspace and the caller's declared other live payload,
including predictor allocations. FFT plans, allocator and runtime backing need
separate admission.

The `demucs_hybrid_trial_reference.py` control uses the pinned CPU
`HTDemucs.forward` and parameter-free layers to compare all physical PCM,
including the release segment and shorter prefixes. Independent High-DC
statistics parity remains failed. Learned execution, productive routing, device
transitions and whole-process admission remain open.

### Explicit original CPU reduction profile

The offline `demucs::reduction::CpuProfile` admits the pinned Windows x64 MSVC
Torch 2.12.0+cu130 commit
`7661cd9c6b841b62b7f411aa52ec51f05457263b`, reported AVX512 and one intra-op
thread. The caller establishes the original wheel, CPU dispatch and strides.
The Native reducer has no Torch dependency. Its scalar Rust operations preserve
the observed eight-lane sum order, four accumulator groups and four cascade
levels. The final scalar tail enters before the eight lane sums. Mean uses
f32 division. A single output std uses f64 squared deviations around the
separate rounded f32 mean. Multiple batch outputs use f64 Welford per item.
Both std paths apply correction one.

`time` reduces packed planar mono/stereo or packed interleaved stereo `[B,C,L]`
after right padding. `cac` reduces packed original stereo `[B,4,2048,T]`.
`cli` first forms each rounded channel mean for packed planar mono/stereo or
interleaved stereo, then reduces that reference globally. The APIs accept exact
shape and element-stride metadata and reject other layouts. Each call includes
borrowed input, output statistics, fixed reduction scratch and declared other
live logical payload in the 512 MiB check before dispatch. Cancellation is
bounded during both reductions and checked through return. Any failure
invalidates the entire statistic output. Readers and source owners remain held
through return.

`demucs_reduction_trial_reference.py` compares scalar bits and all normalized
and raw-std-restored values against the separate pinned original mean/std calls.
Its isolated oracle uses the explicitly selected serial CPU profile. The
unmodified default CPU execution reports 16 intra-op threads. This bounded
profile requires one. Full release tensors, High-DC/tiny variance, distinct
batch/channel axes, zero branch variance, SIMD tails and cascade boundaries
belong to the controls. The original values of each tensor are identical for
both reducers, including actual Torch CaC features. The existing Native FFT
proof is separate. Physical mono view admission, broader CPU/thread dispatch,
learned execution and productive routing remain open. The retained independent
f64 API and earlier High-DC failure evidence retain their original scope.

ATen sources and attribution are recorded in the
[third-party notices](../rust/crates/stem-dsp/THIRD_PARTY_NOTICES.md).

### Independent offline Demucs normalization composition

`demucs::hybrid::Workspace::run_independent` accepts the explicit serial CPU
profile and measures the actual Native waveform after right padding and its
Native CaC features. `planar_cac` maps packed Native mono features to the
original part/frequency-interleaved row traversal: one 4096-element cascade
per time row, followed by f32 accumulation. Original physical mono view strides
remain outside `cac` admission; the adapter does not relabel a packed slice.
Single-result centered std and multiple-result Welford remain distinct.

`demucs::offline::Workspace` composes independent hybrid calls with the existing
physical host, split/context/trim, supplied shifts and weighted bags. `run_cli`
forms the whole-track channel-reference statistics before any context/shift,
normalizes once without epsilon and restores once after the complete bag.
Constant or singleton CLI reference is refused. `run` omits this CLI envelope.
All learned core outputs are supplied synchronously by the caller. Existing
mathematical-independent and supplied-original-statistic APIs keep their scope.

Allocation-free planning checks the simultaneous union of every retained model
workspace, normalized track, input/output, owned and borrowed statistics, fixed
FFT/reducer scratch and the caller's predictor/external allowance against
512 MiB before allocation. Borrowed callback views are counted once. Runtime,
allocator and WHOLE admission remain separate. Errors and late cancellation
invalidate the complete result; all immutable source readers survive return.

`demucs_independent_trial_reference.py` compares complete normalized branches
and PCM with pinned original `HTDemucs.forward`, same-tensor statistic bits,
Native FFT differences separately, and full original CLI/apply compositions.
Fixtures have parameter-free decoders; they establish no learned execution.
The integrated 23-forward/six-apply matrix passes normalized inputs and complete
PCM. Separately, five High-DC raw FFT comparisons retain 1,478 failing values
at unchanged bounds, with maximum absolute difference 0.00048828125.
Broader dispatch, learned model/export/runtime, productive routing and Torch
cutover remain open. Historical independent-f64 High-DC failures remain retained.

### Fixed HTDemucs learned-core export and CPU boundary

`scripts/export_htdemucs_trial.py` checks the installed Demucs and einops
source pins and the complete `955717e8` checkpoint. Its CLI checks these pins
and extracts the core without loading Torch. The producer stages require
separate, independently admitted Windows CPython 3.14 children. The original
loader checks an owned checkpoint snapshot before deserialization and closes
it before initializing the stock model and strictly loading its state.
The selected statements from the pinned `HTDemucs.forward` retain
both original convolutional encoder/decoder stacks, skips, DConv, frequency
and sinusoidal position embeddings, and cross-transformer. Raw normalization,
FFT, inversion, restore and apply remain outside the exported graph.

The graph interface is fixed to stereo B1 at 44.1 kHz, 343,980 training frames
and 336 spectral steps. The input tensors are float32 `[1,2,2,2048,336]` and
`[1,2,343980]`. The outputs are `[1,4,2,2,2048,336]` and `[1,4,2,343980]`,
ordered drums, bass, other, vocals with real then imaginary parts. The
`capture_reference` captures the normalized inputs and decoder outputs with
hooks on the stock forward. It writes and hashes those four tensors and the
complete original PCM, then releases the original mix, raw decoder backings and
PCM before running the extracted core. It compares the new decoder outputs
against the persisted originals in C-order float32 chunks of up to 256 KiB.
Each comparison checks the extent and digest of the same bytes it reads.
Numeric equality allows either sign of zero. The trial child uses stock CPU
dispatch with one intra-op and one inter-op thread.
`trace_core` reloads the same pinned model in a fresh child and verifies
both captured normalized inputs against their recorded extents and digests.
Only this child disables the MHA fastpath for legacy export. It writes a
marked inline graph without carrying the original forward or decoder captures
into tracing. `rewrite_graph` lowers the exact marked bytes, validates the
signature, and binds the checkpoint, source, trace, mix and reference tensors
in the graph metadata. `check_graph` independently validates the exact
rewritten bytes and runs the full ONNX checker.

The owner supplies immutable expected artifact identities and admits each
phase before child startup. It adopts the child result only after successful
exit, actual Job/tree end, and both output readers and artifact writers ending.
Partial outputs from failed phases are ineligible. The child requires a fresh
process and invokes its phase-specific admission callback before heavyweight
imports. The callback represents the caller's decision. The CLI remains a
source and checkpoint check. ONNX is absent from the original producer's
import path. Trace, rewrite and check use the explicit checked overlay and
verify package versions, module origins and binary digests.

The CPU release producer uses the separately verified Torch 2.12.0+cpu
Windows CP314 wheel at the same Torch source commit. It requires its own
phase admission. An independently admitted `prepare_model_state` child can
convert the pinned FP16 checkpoint to an aligned FP32 bank in bounded chunks.
It derives the exact stock parameter layout on the meta device and records the
checkpoint, source and ordered tensor identities. After the preparation child
and its readers end, pass the bank directory and returned `ModelStateBank` as
`mapped_state` to `capture_reference` or `trace_core_separated`. Each consumer
requires the pinned CPU runtime and checks the manifest and the bytes of the
read-only view from the same Windows file handle. The file denies write and
delete sharing. Strict assignment keeps the stock parameter layouts and the
storage retains the view through the last tensor alias, including trace weights
used during initializer serialization.

The bank contains 167,937,824 bytes of FP32 payload and 2,144 bytes of alignment
padding. Its 4 KiB page-rounded extent is 167,940,096 bytes. Include mapped
residency separately in each phase admission. Only the separated trace selects
the CPU Flash SDPA backend.
The stock reference keeps its original dispatch. In the mapped CPU CAC
reference, the pinned forward releases the complex STFT view after magnitude
conversion and drops the redundant magnitude alias before normalization.
The CAC mask reads only the decoder output. The two released storages total
22,156,608 logical bytes at the fixed input geometry. In a checkpoint-free
front-end comparison, the pinned c10 decommit profile released the Windows
commit covering both former storage ranges by the first spectral encoder.
Account for the full reference's allocator commit independently. The separated path uses the
pinned TorchScript graph conversion with ONNX constant folding disabled. It
serializes the graph with the learned parameters as named inputs, then writes
each parameter to a bounded raw bank with its name, shape, dtype and SHA-256.
Using the binding's deferred export flag would instead create a Python bytes
object for every parameter. Both graph-wide shape sweeps receive tensor type
metadata without the initializer values. Per-symbolic shape inference stays
enabled. After the real CPU trace, the legacy output-shape pass receives
metadata-only float32 CPU views with the original shapes, strides and offsets.
This releases 55,133,696 bytes of decoder backing storage from the output
examples before lowering and retains the real graph outputs. The trace
precomputes the pinned transformer's sinusoidal position data
with the original source functions. It stores the two spectral axes separately
and expands and concatenates them in the graph. The two axes and the temporal
table add 3,104,768 bytes to the initializer bank and use the same custody as the
learned state.

`assemble_trace` runs in a separate ONNX-only child. It matches the parameter
manifest to every nonpublic input, checks each tensor and the whole bank as it
streams them into the inline graph, and binds the exact assembled bytes as
the trace identity used by rewrite and check. The approved Scan64 worker
requires CPU trace provenance. The original reduction profile retains its
existing pin. The isolated CPU producer is a development tool and adds no
installed dependency.

The opt-in `demucs-inference-trial` feature adds a synchronous CPU adapter.
It checks the trusted graph digest and metadata and validates both decoder
outputs through NaN-initialized ORT bindings before writing caller buffers.
The same adapter can invoke `demucs::offline::Workspace` to restore the
branches and reconstruct PCM. The caller owns exact reader lifetimes and
independent process admission. A standalone dynamic runtime must use the
checked ORT package and `PROCESS_GUARD` through process exit.

The exporter marks the original unmasked SDPA sites during legacy Torch
tracing and replaces all ten markers with standard opset-18 Scan. Each
iteration takes 64 consecutive queries and computes scaled dot products,
a last-axis softmax over all keys, and the full value product. This keeps
the legacy exporter's Q and K pre-scaling and the query/head order. The largest
FP32 score tile is 5,505,024 bytes. Accounting charges both score and
softmax tiles, with input/output transposes, context storage, other
operators and allocator costs accounted for separately. The pinned ORT
CPU Scan implementation executes each iteration synchronously and writes
context tiles into slices of the assembled output.

#### Logical ownership and admission

The fixed extents below are bytes of identified owners. Use a separate
prospective allowance for each child, covering startup through its last reader.

| Phase | Identified logical owners | Remaining allowance work |
| --- | --- | --- |
| Original and trace checkpoint load | 84,141,911-byte immutable snapshot and 533 half storages totaling 83,968,912. The half storages then overlap 167,937,824 bytes of initialized FP32 state during strict load. | ZIP and pickle staging, conversion and initialization scratch, and allocator retention after snapshot release. |
| Original reference | 2,751,840-byte mix, 13,761,888 bytes of normalized captures and 92,890,560 bytes in eight skips at transformer entry. The largest expected fused CPU score is 231,211,008 bytes with in-place softmax. At that site also charge 16,515,072 bytes packed Q/K/V, two 8,257,536-byte activation pairs and a 5,505,024-byte normalized input. The five files total 79,816,800 bytes. Cropped decoder captures can retain 44,126,208 and 11,007,488 bytes of backing. The persisted comparison removes 68,892,896 bytes of original backings from the second forward's live owners. | The first spectral self-attention subtotal remains conditional at 547,088,288 bytes including weights. Account for stock oneDNN descriptor scratch and blocked reorders, MKL FFT and BLAS scratch, caches, and allocation and exit retention. |
| Legacy trace | 167,937,824-byte model state, 13,761,888 bytes of captured inputs, and 55,047,552 bytes of traced decoder outputs. C++ holds a ModelProto, a serialized string and Python bytes at the export binding. Torch subsequently parses its Python bytes into an ONNX ModelProto while the JIT graph, parameter table, model and outputs are still live. | Bound actual flash/math dispatch, JIT and intermediate graph passes, tensor copies, protobuf and allocation. The 192 MiB accepted artifact limit applies after Torch serialization. |
| Separated CPU trace | The pinned model state and 3,104,768 bytes of position initializers are carried as named inputs. The two graph-wide shape sweeps cannot serialize those parameter values into temporary node graphs. The 55,133,696 bytes of output-example backings are released before lowering. | Budget the live decoder backings during tracing, Flash projection and non-tile owners, convolution scratch and retention, JIT constants, per-node ModelProto and inference, folding and skeleton serialization. The accepted skeleton extent is checked after export. |
| Rewrite | At most 201,326,592 input bytes, the parsed inline graph, old/new node storage during replacement, and a new serialized owner during write. | Bound protobuf memory, replacement growth before serialization, scratch and retained allocation. |
| Final check | At most 201,326,592 hash-verified bytes, a Python protobuf for inline/signature/provenance validation, then a parsed C++ graph with a full shape-inference copy. The native parser reads the same immutable byte buffer. | Bound C++ parser, inference and allocator retention across these operations. |
| Native load and run | Inline graph bytes, FP32 initializer storage and provider copies, normalized tensors, skips, outputs, Scan layouts and context. Charge up to 11,010,048 bytes for two score-tile owners in a Scan body. At the graph extent ceiling, the file and two FP32-sized owners alone total 537,202,240 bytes. | Establish actual graph/initializer/provider ownership, allocator and operator scratch, IOBinding, profiler, output and synchronous reader lifetime. |

Each allowance also needs the actual Python/Torch/ONNX/ORT and transitive DLL
closure for that child, including stacks, runtime initialization, errors,
panic and exit. The Windows Torch build loads DLLs from its CUDA-enabled
wheel even for CPU execution. A cap, artifact size or observed peak cannot
certify prospective successful whole-process execution. See the pinned
[Torch export binding](https://github.com/pytorch/pytorch/blob/7661cd9c6b841b62b7f411aa52ec51f05457263b/torch/csrc/jit/python/python_ir.cpp#L243-L280),
[CPU attention](https://github.com/pytorch/pytorch/blob/7661cd9c6b841b62b7f411aa52ec51f05457263b/aten/src/ATen/native/transformers/attention.cpp#L348-L382)
and [ONNX checker](https://github.com/onnx/onnx/blob/v1.23.2/onnx/checker.cc#L1257-L1294).

The separately checked Windows CPython 3.14 export tools use ONNX 1.23.2,
protobuf 6.33.6 and ml_dtypes 0.5.4 with the existing NumPy and
typing_extensions. The standalone `demucs_attention_control` example
exercises the ten Scan sites on synthetic Q/K/V through the controlled
ORT CPU package. The learned export still requires finite per-phase
producer allowances. The dense CPU adapter still refuses before ORT bootstrap.
Full graph/runtime and process admission, complete learned branch and
PCM/tail comparisons, and production cutover remain open.

#### Windows source-checkout HTDemucs CPU worker

The App captures the explicit `FLITZIS_HTDEMUCS_NATIVE_CPU=1` selection when
it resolves an HTDemucs stem request. It decodes the original first audio
stream with the shared FFmpeg codec and stages finite float32 PCM for one
Native model process. The model process checks the actual capped Windows Job,
maps the digest-pinned learned ONNX file read-only, and loads the pinned
ORT 1.24.4/API 24 CPU package through the package-system32 process guard.
It uses Native channel conversion and Julius resampling, the release segment
and shifts, CLI normalization, split/apply and four named decoder outputs.
After process and reader retirement, the App verifies all four PCM digests,
encodes PCM16 and uses the normal aligned five-artifact cache preparation.
The original App source and publication owners retain their existing checks.
Native results require separate producer and codec evidence before they can
grant completed-set reuse.

The Windows executable target is `native-htdemucs-worker` with the
`native-demucs-worker` feature of `flitzis-looper-analysis`. The source-checkout
worker layout is `native-cpu` under the configured HTDemucs checkpoint
directory. It contains `native-htdemucs-worker.exe`, `htdemucs.onnx` and the
independent checked CPU `ort` package with its `step23-runtime.json` and
`step23-package.json` manifests. Native selection requires the source-owned
`workers/htdemucs/native_cpu/release.json` record, compiled into the worker.
It binds the exact graph extent and digest, trace and canonical attention
digests, and reviewed extra load/run bytes for the complete pinned graph and
runtime. The separate source-owned `worker.json` record pins the finished
Windows executable by digest and extent. Both records currently contain
`null`. The App refuses explicit Native selection before codec or model
launch while approval is absent.

The producer's final checker independently recognizes the ten canonical Scan64
chains in the final artifact and checks the same artifact's inline weights,
fixed signature, provenance and full ONNX validity. The producer still needs a
compatible, independently admitted development-time Torch export. The CUDA
wheel failed to load a dependency within the previous 512 MiB producer Job.
The stock reference and separated CPU trace still require independent
whole-phase budgets. The stock capture hooks write the two normalized inputs
and both cropped decoder views in 256 KiB chunks as they pass the original
model's boundaries. They retain only the file identities. The extracted-core
comparison reopens the two contiguous normalized inputs, verifies their
digests while reading them, and compares its decoder outputs against the
original files. Stock dispatch and the original PCM calculation are intact.

For Windows stock CPU admission, build
`scripts/htdemucs_ideep_recorder.cpp` with the CPU wheel's oneDNN 3.11.2
headers and the exact iDeep `e539e0f9774e2018f0d56fe865da66581f692e3d` headers.
Use x64 release MSVC, C++20, the dynamic CRT (`/MD`), `NDEBUG` and
`_ITERATOR_DEBUG_LEVEL=0`. The recorder binds the exported CPU engine of
the loaded official Torch 2.12.0+cpu wheel. By default it delegates allocations and
frees to the original 4096-byte-aligned allocator and keeps its callbacks
resident through process teardown.

Run `scripts/probe_htdemucs_ideep.py` in a fresh independently admitted
CP314 Windows Job with an exact CPU overlay, the recorder DLL and its
SHA-256. Set `ONEDNN_VERBOSE=profile_create,profile_exec` before launch.
Each invocation covers one fixed geometry without a checkpoint. Pass
`--journal` with a new private file path. The recorder publishes fixed-width
phase, allocation-attempt, allocation-result, free and access-fault rows to
a mapped file. It samples Windows private commit around callbacks and at the
ATen call boundaries. It records a failed nonzero allocation before ending the
probe, so oneDNN never receives the failed handle. The fault handler leaves
Windows exception dispatch in place. The journal can contain raw fault
addresses. Keep it private and read it with a file mapping after Job, process
tree and both readers end.

The default callback uses the pinned UCRT. For the largest spectral
transpose, `--cold-only --page-backed` gives nonzero iDeep requests dedicated
pages and releases them with `MEM_RELEASE`. It leaves zero-byte requests with
the UCRT. The BRG descriptor's 33,181,848-byte user scratch includes the
nested convolution's workspace. BRG primitive and JIT metadata, reorder
buffers and descriptor resources have separate lifetimes. The ATen return
can overlap the 44,126,208-byte iDeep output with two dense ATen backings of
the same logical size. ATen arena commit also includes allocator rounding.

Use `--cold-only` first for the largest transpose. The guarded repeat
requires `--warm-reserve-bytes` from an independently settled cold run of the
same geometry and allocator profile. Derive it by subtracting private commit
before the cold call from the observed cold Job peak. The supervisor must
verify the actual 512 MiB Job before resume. Set these options before the
pinned Torch image loads.

- `MIMALLOC_PURGE_DELAY=0`
- `MIMALLOC_PURGE_DECOMMITS=1`
- `MIMALLOC_ARENA_EAGER_COMMIT=0`
- `MIMALLOC_ALLOW_LARGE_OS_PAGES=0`

The repeat checks that dropping the cold ATen output released at least its
64 MiB arena extent and that current private commit plus the reserve fits.
It charges the larger of the independent reserve and this process's cold
peak increment. This profile preserves the stock BRG and reorder dispatch
and checks the full dense output SHA-256 for the diagnostic geometry. The
original default-allocator warm failure remains part of the evidence.
Whole learned admission still requires the complete oneDNN, MKL, allocator,
plain-retry and error/exit bounds.

The mapped Native loader reduces serialized private
ownership and still counts the file-backed span and both conservative
initializer owners. The compact position buffers require 6,209,536 bytes
for two additional initializer owners during load and 3,104,768 bytes for
one during run, in addition to the serialized graph extent and operator
workspace.
The dense trial retains its pre-bootstrap refusal.

#### Exact inline graph load control

The Windows-only `demucs_inline_load_control` accepts the single fixed fixture
from `scripts/demucs_inline_load_fixture.py`. The graph is 134,217,914 bytes and
contains two 67,108,864-byte inline FP32 MatMul initializers. Its source-pinned
SHA-256 covers the complete artifact, including tensor contents and metadata.
No external data, alternative digest or learned artifact is accepted.
The separately admitted child uses the pinned ORT 1.24.4/API 24 CPU package
under the existing package-system32 process guard. Both modes use sequential
execution, one intra-op and one inter-op thread, with graph optimizations,
prepacking, CPU arena and memory patterns disabled.

The copied control holds a Rust allocation of the exact serialized extent
through synchronous session creation. The mapped control uses one unnamed
read-only disk-file view from the same handle that established the extent.
The handle rejects write and delete sharing and outlives the view. The slice
hashed by Rust is the slice passed to ORT's `CreateSessionFromArray`. ORT
never reopens the graph path. The control releases the serialized owner and
file before evaluating every output value of both MatMuls.

The mapping removes the serialized private heap allocation. Its entire extent
can still occupy file-backed physical pages, with separate address-space,
page-table and fault costs. ORT parses the same bytes into a ModelProto,
resolves shapes and then materializes large initializers one at a time during
graph transformation, including with the optimizer disabled. While each
initializer is copied, its protobuf raw data coexists with its new CPU tensor
and the other initializers. The fixture therefore has a 201,326,592-byte
initializer subtotal at that handoff, before allocator retention or other
work. Adding the serialized extent gives 335,544,506 logical bytes in both
modes with distinct private versus file-backed ownership. The CPU session
state reuses these materialized tensors. Small initializers, nested graph
attributes, required transforms, provider copies and other allocations need
separate treatment for the learned graph. Keep the conservative learned load
estimate until those lifetimes are closed.

See [ORT array loading](https://github.com/microsoft/onnxruntime/blob/2d924974ef147392ced8409d36bd6d2e7fcc8a74/onnxruntime/core/session/inference_session.cc#L1101-L1151),
[initializer conversion](https://github.com/microsoft/onnxruntime/blob/2d924974ef147392ced8409d36bd6d2e7fcc8a74/onnxruntime/core/graph/graph.cc#L3740-L3799),
[CPU initializer reuse](https://github.com/microsoft/onnxruntime/blob/2d924974ef147392ced8409d36bd6d2e7fcc8a74/onnxruntime/core/framework/session_state_utils.cc#L363-L441)
and [Windows file view semantics](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-mapviewoffile).

The Job commit observation and ORT profiles describe the controlled fixture.
A map failure or in-page error invalidates the child result. The parent must
observe process tree, Job and both pipe readers through retirement.
The release graph, metadata, adapter refusal and independent learned
producer and Native admission gates remain in effect.
