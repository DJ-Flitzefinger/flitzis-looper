# Development Guide

This guide covers local development, validation, OpenSpec usage, and package
layout for the current project.

## Branch And Repository

Expected development branch:

```text
main
```

Before repository edits:

```powershell
git branch --show-current
git status
```

The user handles GitHub pushes manually. Codex must not push or change remotes.

## Setup

Run commands from the repository root:

```powershell
uv sync
uv run maturin develop
```

Start the app:

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

### Windows Start Files

Double-click a start file in the repository root, or call it from a terminal:

- `start-dev.bat` builds and starts the app with the Rust Debug profile.
- `start-release.bat` builds and starts the app with the optimized Rust Release
  profile.

Both files run from their own repository directory, synchronize dependencies
with `uv sync --locked`, install the selected native profile with
`uv run --no-sync maturin develop --locked` (adding `--release` for Release),
then run `uv run --no-sync python -m flitzis_looper`. Existing build artifacts
are reused by the build tools. The shared implementation is
`scripts/start-app.bat`.

Close all running Looper windows before using either file: both profiles install
the same native extension, which Windows cannot replace while the app is using
it. A failed setup or build stops the launch, and the terminal stays open to
show the error. The launchers require `uv` on `PATH` and the native build setup
described below. Release still runs the Python source app; these files do not
create the future standalone installer.

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
[96-handle setup/resource measurements](pcm-cache-measurements.md) are separate.
The fixed wet adapter
lead remains 511 frames. Native reset/cold pitch work stays off the callback;
reserve starvation produces bounded wet silence, while dry processing remains
reactive. See [Key Lock backend](key-lock-backend.md) for the allocation audit,
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
save costs are reported separately in [C3 measurements](pcm-cache-measurements.md).

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
and output proofs. [C3 measurements](pcm-cache-measurements.md) report startup/RAM
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

Focused changes should run focused tests. Broader Rust/audio, persistence,
OpenSpec, bridge, or UI-control changes should run the full sequence:

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
uv run pytest src/tests/flitzis_looper/controller/transport/test_residency.py
```

Repeat native checks with `--release` and install the matching debug/release
extension before the Python suite, serially. The long-cycle test compares the
actual native ACK/render route with complete-buffer output and an independent
PCM/period oracle at 75 and 1000 observed cycles. It certifies those numerical
contracts only. [C3 process-memory/performance measurements](pcm-cache-measurements.md)
remain separate from these numerical proofs and the open human device/listening
gates. Generated source and proof exports stay outside the repository.

### C3 measured startup/resource acceptance

The [C3 measurement report](pcm-cache-measurements.md) records the isolated
Debug/Release productive controller cold/warm matrix, actual native readiness/ACK,
PCM and process-resource scopes, current 96-handle setup, complete-context
exceptions, long-source save and cancellation/cleanup results. Its paired
finite/current-full comparisons include regressions and single-run/OS-cache limits.

Full Debug/Release numerical validation remains a separate required gate; isolated
phase-origin-zero dry renders do not replace it. Actual human/device/listening
acceptance remains open until the final pre-port stage. C3 does not begin full
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

The app may create local runtime files:

- `samples/`: project-local copied samples, stem cache, and project config.
- `config/input/`: local input mapping JSON files.
- `.pytest-tmp*`, `.pytest_cache`, `.ruff_cache`, `.mypy_cache`, `.venv`, and
  `rust/target/`: tool/build artifacts.

Do not confuse generated runtime data with source documentation or OpenSpec.

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
