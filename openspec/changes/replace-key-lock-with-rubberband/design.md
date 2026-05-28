# Design: Rubber Band Key Lock backend

## Current State

The mixer already keeps separate output-frame and source-frame time domains. For
each active voice it computes a bounded tempo ratio from global speed or
BPM-lock metadata, reads that many source frames from the full mix or prepared
stems, and produces a fixed number of output frames for the callback.

The current `StretchProcessor` uses that source reader for varispeed output and,
when Key Lock is enabled, applies a custom delay-line pitch compensation stage.
That delay-line stage is the part being replaced.

## Tooling And Installed Library

Local Windows tool bootstrap completed for this branch:

- `cmake` 4.3.3: `C:\Program Files\CMake\bin\cmake.exe`
- `ninja` 1.13.2:
  `C:\Users\user\AppData\Local\Microsoft\WinGet\Packages\Ninja-build.Ninja_Microsoft.Winget.Source_8wekyb3d8bbwe\ninja.exe`
- `vcpkg`: `C:\Users\user\AppData\Local\vcpkg`
- vcpkg triplet: `x64-windows`
- Rubber Band package: `rubberband:x64-windows` version `4.0.0#1`
- Rubber Band headers:
  `C:\Users\user\AppData\Local\vcpkg\installed\x64-windows\include\rubberband`
- Link library:
  `C:\Users\user\AppData\Local\vcpkg\installed\x64-windows\lib\rubberband.lib`
- Runtime DLLs:
  `rubberband-3.dll`, `sleefdft.dll`, `sleef.dll`, `samplerate.dll` under
  `C:\Users\user\AppData\Local\vcpkg\installed\x64-windows\bin`

`rubberband-3.dll` also depends on the MSVC runtime DLLs. Maturin development
and packaging must either put the vcpkg runtime DLL directory on the process
`PATH` for local runs or copy the required DLLs next to the built extension.

## Rubber Band API Choice

Rubber Band exposes two relevant C API surfaces in `rubberband-c.h`:

- `rubberband_new` / `rubberband_process` / `rubberband_available` /
  `rubberband_retrieve` for the general stretcher.
- `rubberband_live_new` / `rubberband_live_get_block_size` /
  `rubberband_live_shift` for the live pitch shifter.

The general realtime stretcher is variable-output. Official integration notes
state that callers cannot provide a fixed input block and expect a fixed output
block; callers must check output availability and often operate in a pull model
using `getSamplesRequired`, `process`, `available`, and `retrieve`. That model
can be made bounded, but it is a larger adapter and has explicit start padding
and start-delay handling.

The existing Looper mixer already applies the time/tempo component by advancing
the source frame position according to the tempo ratio. For this branch, the
preferred runtime path is therefore Rubber Band LiveShifter:

- The mixer continues to read source frames by tempo ratio.
- Key Lock disabled returns the varispeed result directly.
- Key Lock enabled sends that varispeed result through LiveShifter with pitch
  scale approximately `1.0 / tempo_ratio`.
- LiveShifter has a fixed block size for its lifetime, so per-voice buffers and
  FIFO space can be allocated outside the callback.

This keeps the Rubber Band integration aligned with the current source-frame
ownership model and avoids an unbounded callback loop waiting for the general
stretcher to produce output.

If implementation evidence shows LiveShifter cannot satisfy the audible behavior
or packaging requirements, the design must be revised before switching to the
general stretcher. A stretcher-based fallback must still use a bounded
refill/retrieve budget and an explicit underrun fallback.

## Link Probe Result

A temporary Rust probe under `C:\Users\user\AppData\Local\Temp\codex` linked
against:

```text
C:\Users\user\AppData\Local\vcpkg\installed\x64-windows\lib\rubberband.lib
```

and ran successfully with the vcpkg DLL directory prepended to `PATH`:

```text
C:\Users\user\AppData\Local\vcpkg\installed\x64-windows\bin
```

For `rubberband_live_new(48000, 2, OptionWindowShort | OptionChannelsTogether)`,
the installed Rubber Band 4.0.0 backend reported:

```text
channels=2 block_size=512 start_delay=3678 pitch_scale=0.500
```

The 512-frame LiveShifter block size matches the current callback block target
well enough for a first implementation slice, while the reported 3678-sample
start delay must be documented and tested before user testing.

## Rust Boundary

Add a small Rust module, likely `rubberband_backend`, with manually declared FFI
for only the C symbols used by the selected backend. Do not expose C++ directly
to the mixer and do not add bindgen unless a later implementation step proves
that hand-written declarations are insufficient.

The wrapper should own the opaque Rubber Band handle, implement `Drop`, and
provide safe Rust methods for:

- construction outside callback rendering,
- channel and block-size query,
- start-delay/latency query,
- pitch-scale update,
- reset or state clear,
- fixed-block processing,
- debug level forced to `0`.

All unsafe code stays inside the wrapper. The mixer and voice slot code should
see only a small safe Rust API.

## Callback Processing Model

Each voice owns its Rubber Band state and fixed buffers before callback
rendering:

- deinterleaved input staging,
- deinterleaved Rubber Band output block,
- bounded FIFO/ring space for shifted output,
- channel pointer arrays for FFI calls,
- scalar counters for FIFO read/write and start-delay handling.

When Key Lock is enabled, the adapter accumulates varispeed output into
LiveShifter-sized blocks, calls `rubberband_live_shift` only when a full block is
available, and drains shifted frames into the callback output. It must not spin
waiting for output. If shifted output is unavailable for a requested callback
block, the adapter must fill the missing frames using a deterministic bounded
fallback chosen in implementation and covered by tests.

Key Lock mode changes and tempo-ratio updates are scalar state changes. They
must not stop, retrigger, reload, regenerate stems, or reanalyze pads.

## Latency And Playheads

Rubber Band latency must be queried and recorded for the chosen options and
sample rate. Playhead telemetry remains source-frame based because loop
wrapping, BPM-lock tempo ratio, and waveform-editor position are source-domain
concepts. Any audio-output start delay introduced by Rubber Band must be
documented and either compensated or explicitly accepted for the first
implementation slice.

Voice start, retrigger, seek, stop, unload, and stem-source changes must isolate
old Rubber Band state from new playback. Any call used in the callback for reset
or state clearing must be documented as callback-safe by code review and tests,
or moved to a non-realtime preparation path.

## Build And Runtime Integration

The first build-file change should be cross-platform and minimal:

- On Linux, prefer `pkg-config` discovery for the system `rubberband` package.
  Common development packages include `librubberband-dev` on Debian/Ubuntu,
  `rubberband-devel` on Fedora/RHEL-like distributions, and `rubberband` plus
  `pkgconf` on Arch-like distributions.
- On Windows, prefer `VCPKG_ROOT` with the `x64-windows` triplet for local
  development and link against `rubberband.lib`.
- Support explicit override environment variables only when automatic discovery
  is insufficient, for example include/lib/runtime directories supplied by a
  developer or packaging script.
- Ensure local `uv run maturin develop` can run with required runtime libraries
  available before the app starts.
- Keep vcpkg and downloaded build tools outside `repo/`; the repository must not
  vendor the Rubber Band source, DLLs, or vcpkg tree.
- Do not hardcode the local Windows path
  `C:\Users\user\AppData\Local\vcpkg` or any Linux workstation path in
  production source.

Runtime library loading is still platform-specific:

- Windows development can use the vcpkg `installed\x64-windows\bin` directory on
  `PATH`, or copy required DLLs next to the extension during packaging.
- Linux development should use the system dynamic linker path from the distro
  package by default. Custom library directories should be documented through
  environment variables or linker configuration rather than source-code paths.

The later Nuitka setup build should bundle the required Windows native DLLs with
the application installer. At minimum that means `rubberband-3.dll`,
`sleefdft.dll`, `sleef.dll`, and `samplerate.dll` from the selected Rubber Band
distribution, plus any MSVC runtime requirements not already supplied by the
target environment.

Rubber Band's GPL/commercial licensing model must remain visible in release
planning. The repository is GPL-licensed, but any future installer distribution
still needs a deliberate licensing check before publishing binaries.

## Validation Strategy

- Run a minimal Rust link probe against the installed vcpkg library before
  editing production Rust build files.
- Add wrapper lifecycle tests outside the audio callback.
- Add deterministic Rust mixer tests for Key Lock off vs on pitch behavior,
  neutral-ratio transparency, fixed block/FIFO bounds, underrun fallback, ratio
  changes while active, loop wrap, retrigger, stop/unload cleanup, and prepared
  stems.
- Run official strict OpenSpec validation for this change.
- Validate native dependency discovery on Windows and Linux before declaring
  the branch ready for merge or release packaging.
- Document Windows and Linux developer installation steps in `README.md` and
  `docs/development.md`.
- Before the branch is ready for user testing, run the full uv-managed Rust and
  Python validation sequence.
