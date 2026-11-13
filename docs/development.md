# Development guide

Read the [contributor instructions](../AGENTS.md), [architecture](architecture.md)
and applicable [OpenSpec contracts](../openspec/specs/). Work from the intended
revision in its managed checkout and inspect the branch and working tree before
editing. Use the publication permissions for the actual task.

## Setup

From the repository root, with uv, Rust and the platform's C/C++ build tools
installed, run:

```powershell
uv sync --locked
uv run --no-sync maturin develop --locked
```

Install Rubber Band with the LiveShifter C API before the Native build.
On Windows x64, provision the required BPM worker with
`uv run --no-sync python scripts/setup-runtime.py`.
The [model worker guide](model-worker-setup.md) covers worker/model provisioning
and FFmpeg. The Linux Native build uses the dependency discovery below, while
the locked BPM worker installer requires Windows x64. Launch a configured
development checkout with `uv run --no-sync python -m flitzis_looper`.
Use `uv run cargo …` or the Windows helper for toolchain commands involving
the project Python environment.

### Windows start files

| Entrypoint | Behavior |
| --- | --- |
| `start.bat` | Starts the existing Release profile with the existing interpreter. `--check` exits after the same preflight. |
| `build-release.bat` | Builds and verifies Release, then waits for a key without opening the App. |
| `start-dev.bat` | Builds and starts Debug. `--build-only` exits after build and verification. |
| `start-release.bat` | Builds and starts Release. `--build-only` exits after build and verification. |

The BAT files resolve the checkout from their file paths. Build entrypoints
synchronize the locked environment and install the selected Native profile.
Close the App before replacing its shared extension. Normal start verifies the
installed profile before launch. See [Portable](portable-windows.md) for the
independent installer and its private runtime.

### Native dependencies

Linux builds discover LiveShifter through `pkg-config` or
`RUBBERBAND_LIB_DIR` and `RUBBERBAND_INCLUDE_DIR`. Use a package or source
build with `rubberband_live_*` (for example Rubber Band 4.0.0). For a custom
prefix, set `PKG_CONFIG_PATH` and the runtime linker path as needed.

On Windows install the MSVC C++ toolchain and Windows SDK. Install Rubber Band
through vcpkg and the tracked [overlay](../scripts/native/vcpkg-overlay/).
The vcpkg revision for this build recipe is
`020c8202737fb3ae3bfef5dd78ca537137be0ea0`.

```powershell
git clone https://github.com/microsoft/vcpkg.git "$env:LOCALAPPDATA/vcpkg"
git -C "$env:LOCALAPPDATA/vcpkg" checkout 020c8202737fb3ae3bfef5dd78ca537137be0ea0
& "$env:LOCALAPPDATA/vcpkg/bootstrap-vcpkg.bat" -disableMetrics
& "$env:LOCALAPPDATA/vcpkg/vcpkg.exe" install rubberband:x64-windows "--overlay-ports=$PWD/scripts/native/vcpkg-overlay"
$env:VCPKG_ROOT = "$env:LOCALAPPDATA/vcpkg"
```

Use an x64 Visual Studio developer shell if the compiler is unavailable.
Explicit build overrides include `RUBBERBAND_LIB_DIR`, `RUBBERBAND_INCLUDE_DIR`,
`RUBBERBAND_VCPKG_TRIPLET`, `RUBBERBAND_LINK_KIND` and
`RUBBERBAND_EXTRA_LIBS`. The Windows Python wrapper checks
`RUBBERBAND_DLL_DIRS`, `RUBBERBAND_DLL_DIR`, vcpkg and `PATH` for dynamic
dependencies. The Windows Rust test runner adds the active Python and Native DLL
directories to process-scoped environment variables for the test executable.

## Validation

```powershell
uv run --no-sync pytest
uv run --no-sync ruff check src
uv run --no-sync mypy src
uv run --no-sync cargo fmt --manifest-path rust/Cargo.toml --all --check
.\scripts\run-rust-tests.ps1
```

On other platforms use `uv run --no-sync cargo test --manifest-path rust/Cargo.toml --workspace`.
The Windows helper forwards selected test arguments to Cargo. Usual Python
tests skip physical-device cases. Use `pytest --audio-devices` for an authorized
device session. Choose focused regressions first, then cover impacted integration
paths. Native changes can require a matching Release build and import/preflight.
Docs changes require source consistency, valid links/anchors, and diff review.

A preflight verifies the selected setup. Software-paced callback fixtures
verify their stated render behavior. Device settings, physical output and
listening results have separate scope. See the [archive index](../openspec/changes/archive/2026-11-13-archive-index.md)
for November 13 acceptance and historical run scope.

## OpenSpec and source layout

For a user-visible change, create a focused delta under
`openspec/changes/<change-id>/specs/<capability>/spec.md`, or use an existing
contract that fully covers the correction. Begin each requirement with a
normative SHALL or MUST sentence and include a scenario. Validate with
`openspec validate <change-id> --strict`.

| Location | Contents |
| --- | --- |
| [`src/flitzis_looper`](../src/flitzis_looper/) | App, controllers, UI, input, persistence and offline orchestration. |
| [`src/flitzis_looper_audio`](../src/flitzis_looper_audio/) | Extension wrapper, generated Native build product and public type stub. |
| [`src/tests`](../src/tests/) | Python tests for both packages and repository tooling. |
| [`rust`](../rust/README.md) | Native runtime, analysis, stem DSP and offline conversion crates. |
| [`workers`](../workers/) and [`vendor`](../vendor/) | Isolated worker profiles, model manifests and vendored build/runtime inputs. |

Update the Python stub with Native API changes. Keep runtime-generated models,
build outputs, temporary PCM, settings, recordings and private source material
out of commits. [State, input and UI](architecture/state-input-and-ui.md)
describes the persistent paths and recovery boundaries.
