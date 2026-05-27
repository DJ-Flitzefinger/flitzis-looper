# Development Guide

This guide covers local development, validation, OpenSpec usage, and package
layout for the current project.

## Branch And Repository

Expected development branch:

```text
gen3
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

## Native Rubber Band Dependency

The Rubber Band Key Lock backend depends on the native Rubber Band C API. The
repository should discover that dependency through platform-appropriate build
metadata or explicit environment variables. Production code must not hardcode a
developer's local vcpkg directory, Linux home directory, or other workstation
path.

### Linux

Prefer distro packages plus `pkg-config`:

```bash
# Debian/Ubuntu
sudo apt install librubberband-dev pkg-config

# Fedora/RHEL-like
sudo dnf install rubberband-devel pkgconf-pkg-config

# Arch-like
sudo pacman -S rubberband pkgconf
```

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
- `pkg-config` for non-Windows system packages.
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

For source runs that link Rubber Band dynamically, ensure the vcpkg runtime DLL
directory is available before starting Python:

```powershell
$env:PATH = "$env:VCPKG_ROOT\installed\x64-windows\bin;$env:PATH"
uv run python -m flitzis_looper
```

Observed vcpkg runtime DLLs for the current branch are `rubberband-3.dll`,
`sleefdft.dll`, `sleef.dll`, and `samplerate.dll`. Packaging scripts may copy
those DLLs next to the built native extension instead of requiring `PATH`.

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

Focused changes should run focused tests. Broader Rust/audio, persistence,
OpenSpec, bridge, or UI-control changes should run the full sequence:

```powershell
uv sync
uv run maturin develop
uv run cargo check --manifest-path rust/Cargo.toml
uv run cargo test --manifest-path rust/Cargo.toml
uv run pytest
uv run ruff check src
uv run mypy src
git diff --check
```

Rust formatting:

```powershell
uv run cargo fmt --manifest-path rust/Cargo.toml --check
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
