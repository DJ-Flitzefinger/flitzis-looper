<h1 align="center">Flitzis Looper</h1>

<p align="center">
  A live scratch looper for building custom DJ performance instruments from loops,
  stems, tempo control, and pad-based mappings.
</p>

Flitzis Looper is a specialized performance tool for DJs and turntablists. It is
inspired by the classic scratch looper idea that became popular in DJ culture
around the late 2000s: load musical loops, trigger them quickly, and practice or
perform over a reliable rhythmic foundation.

This project extends that idea into a configurable live instrument. Instead of a
fixed collection of scratch beats, the performer can build a personal looper
from samples, loop regions, stems, tempo settings, pitch controls, mappings, and
per-pad mix controls. The goal is not to replace a DAW. The goal is to make a
focused tool that stays fast during performance and gives direct control over
the material that matters on stage.

## Performance Features

- **Performance pad grid:** A 6 x 6 grid across 6 banks gives 216 sample slots
  for loops, one-shots, prepared stems, and performance variations.
- **Multi Loop:** Multiple pads can play together, so a set can combine drums,
  bass lines, melodic loops, acapellas, textures, and scratch practice material
  instead of being limited to one loop at a time.
- **Exclusive playback:** When a tighter classic looper behavior is needed, pads
  can also be controlled in a one-at-a-time style.
- **BPM Lock:** Loops with tempo metadata can follow the master BPM, keeping
  different source loops aligned while the performer changes tempo.
- **Pitch:** Speed-based pitch control can push loops up or down in energy and
  help match the feel of different source material.
- **Key Lock:** Tempo changes can be applied while reducing unwanted pitch
  movement, which makes BPM changes more useful for melodic material.
- **Loop Editor:** Loaded audio can be inspected and adjusted with editable loop
  regions, beat-grid metadata, downbeat alignment, and waveform-based editing.
- **Stems:** Offline Demucs stem generation can prepare drums, bass, vocals, and
  melody layers for supported pads. During performance, prepared stems can be
  combined, muted, or used as alternate playback material.
- **Per-pad mix controls:** Gain, metering, and a 3-band DJ isolator make
  individual pads easier to balance inside a live set.
- **MIDI and keyboard learn:** Keyboard shortcuts and MIDI controls can be mapped
  to performance actions, so the looper can be played from controllers instead
  of only from the mouse.
- **Project persistence:** Sample assignments, loop settings, metadata, mappings,
  and performance intent are stored with the project so a prepared setup can be
  reopened and continued.

Used together, these features turn the looper into a performance surface rather
than a simple sample player. A performer can prepare a bank as a scratch
practice tool, another as a stem-based remix setup, another as a tempo-matched
loop kit, and move between them with keyboard or MIDI control. The important
musical decisions stay close to the pads: what plays, what stays in sync, what
gets isolated, and what should react immediately during a set.

## Basic Use

Start the app, load audio onto pads, then trigger pads from the grid:

- Left-click a pad to trigger or retrigger playback.
- Right-click a pad to stop that pad and select it.
- Middle-click a pad to open pad actions such as load or unload.
- Use the waveform editor to adjust loop start, loop end, beat-grid, and
  downbeat information.
- Use the bottom bar for transport, performance controls, quantization, stems,
  and settings.

## How to Install

Prerequisites:

- Python 3.14+
- [uv](https://docs.astral.sh/uv/)
- Rust toolchain
- Rubber Band native development/runtime library for Rubber Band Key Lock builds
- FFmpeg and Demucs model files only if you use stem generation

### Native Rubber Band Dependency

The Rubber Band Key Lock backend uses the native Rubber Band library through a
small Rust FFI wrapper. Build and runtime discovery must be set up before the
app starts; no library discovery happens in the realtime audio callback.

The current Key Lock backend requires Rubber Band's LiveShifter C API
(`rubberband_live_*`). Use Rubber Band 4.0.0 or another package that provides
that API. Some LTS distribution packages are too old: Ubuntu 24.04
`librubberband-dev` 3.3.0 does not satisfy this branch.

Linux development should use the distribution package where it provides the
LiveShifter C API:

```bash
# Debian/Ubuntu
sudo apt install librubberband-dev pkg-config

# Fedora/RHEL-like
sudo dnf install rubberband-devel pkgconf-pkg-config

# Arch-like
sudo pacman -S rubberband pkgconf
```

If the distribution package is older, install a newer Rubber Band package or a
source build and expose its `.pc` file with `PKG_CONFIG_PATH`, or use the
documented explicit library/include override variables.

Windows development can use vcpkg:

```powershell
git clone https://github.com/microsoft/vcpkg.git "$env:LOCALAPPDATA\vcpkg"
& "$env:LOCALAPPDATA\vcpkg\bootstrap-vcpkg.bat" -disableMetrics
& "$env:LOCALAPPDATA\vcpkg\vcpkg.exe" install rubberband:x64-windows
setx VCPKG_ROOT "$env:LOCALAPPDATA\vcpkg"
```

The Rust build script looks for `RUBBERBAND_LIB_DIR` first. If that is not set,
Windows builds use `VCPKG_ROOT`, or `$env:LOCALAPPDATA\vcpkg` when present.
`RUBBERBAND_VCPKG_TRIPLET` can override the default `x64-windows` triplet.
Custom builds may also set `RUBBERBAND_INCLUDE_DIR`, `RUBBERBAND_LINK_KIND`
(`dylib` or `static`), and `RUBBERBAND_EXTRA_LIBS`.

When running from source on Windows, the Rubber Band runtime DLL directory must
be on `PATH` before starting the app, unless the build or packaging step copies
the DLLs next to the native extension:

```powershell
$env:PATH = "$env:VCPKG_ROOT\installed\x64-windows\bin;$env:PATH"
```

The Python wrapper also registers documented DLL directories before importing
the native extension. Standalone Rust test binaries do not use that wrapper; on
Windows, run Rust tests with `.\scripts\run-rust-tests.ps1` so uv's selected
Python runtime and the Rubber Band runtime directory are added to `PATH` before
`cargo test` launches the test executable.

For later non-technical Windows distribution, the intended path is a Nuitka
installer that bundles the required runtime DLLs. End users of that installer
should not need vcpkg, CMake, Ninja, Rust, or Rubber Band development packages.
Rubber Band's GPL/commercial licensing model must be checked before publishing
binary installers.

Install dependencies and build the editable native extension from the repository
root:

```powershell
uv sync
uv run maturin develop
```

Start the app:

```powershell
uv run python -m flitzis_looper
```

Stem-generation setup is documented in
[docs/stem-generation-setup.md](docs/stem-generation-setup.md).

## Technical Overview

Flitzis Looper is a Rust/Python application with a deliberate split of
responsibilities:

- Python owns the Dear ImGui performance UI, project persistence, settings,
  mapping edit workflows, file dialogs, and offline/background jobs.
- Rust owns the realtime audio engine: transport, scheduling, mixing, loop
  playheads, playback-rate application, Key Lock, prepared-stem playback,
  parameter application, metering, and per-pad DSP.

The key realtime rule is simple: the audio callback only works with bounded
commands and already prepared audio data. It does not run file I/O, JSON
persistence, Python/GIL code, Demucs, plugin scanning, logging, blocking waits,
or unbounded work.

BPM-locked voices with valid tempo metadata are timed from the Rust
output-frame transport timeline. Their source loop phase is mapped from fixed
output-frame/source-frame anchors, while the Loop Editor grid and loop markers
remain source-domain editing state.

Simplified signal path:

```text
Python UI / controllers / persistence / background workers
-> PyO3 AudioEngine API
-> bounded Rust command and parameter rings
-> CPAL audio callback
-> transport, scheduler, mixer, loops, stems, Key Lock, DSP, metering
-> system audio output
```

The full architecture is documented in
[docs/architecture.md](docs/architecture.md).

## Repository Layout

```text
src/flitzis_looper/        Python app package: UI, controllers, models, persistence
src/flitzis_looper_audio/  Python wrapper and type stubs for the Rust extension
rust/src/                  Rust audio engine, PyO3 bridge, DSP, transport, scheduler
src/tests/                 Python tests and native-extension integration tests
docs/                      Maintained architecture, development, and setup docs
openspec/                  Product/behavior contracts and change deltas
```

`src/flitzis_looper/` is the Python application package.
`src/flitzis_looper_audio/` is intentionally separate: it is the Python import
package for the native Rust extension named `flitzis_looper_audio`.
`maturin develop` builds the platform-specific `.pyd`/extension module into
that package, `__init__.py` re-exports it, and `__init__.pyi` gives Python and
mypy a typed API surface. Do not delete or rename that folder unless the Rust
crate name, PyO3 module name, imports, and packaging are changed together.

## Documentation

Start with [docs/README.md](docs/README.md).

Important documents:

- [docs/architecture.md](docs/architecture.md): current technical architecture.
- [docs/development.md](docs/development.md): setup, validation, OpenSpec, and
  package layout.
- [docs/ui-toolkit.md](docs/ui-toolkit.md): Dear ImGui UI rules.
- [docs/stem-generation-setup.md](docs/stem-generation-setup.md): Demucs and
  FFmpeg setup.
- [docs/key-lock-backend.md](docs/key-lock-backend.md): current Rubber Band Key
  Lock backend and native dependency requirements.
- [docs/todos.md](docs/todos.md): explicit user-requested TODO notes.

Behavior contracts live in `openspec/specs/` and active changes live in
`openspec/changes/`. Do not treat old archived OpenSpec changes or local notes
as a feature backlog. `docs/todos.md` is maintained only when the user asks to
add, review, choose, or complete TODO items.

## Validation

Common checks from the repository root:

```powershell
uv run maturin develop
uv run cargo check --manifest-path rust/Cargo.toml
.\scripts\run-rust-tests.ps1
uv run pytest
uv run ruff check src
uv run mypy src
```

Use `uv run cargo ...`, not plain `cargo ...`, so PyO3 and maturin use the
project Python environment consistently. On non-Windows platforms, or when a
Windows shell already exposes the required runtime DLLs to test executables, use
`uv run cargo test --manifest-path rust/Cargo.toml` directly.

## License

[GNU General Public License](./LICENSE.txt)
