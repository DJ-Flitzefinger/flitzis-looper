# Source-built portable runtime and safe updates

The standalone BAT embeds enough bootstrap logic to work from an arbitrary
folder on Windows x64 without Git, Python, Rust or a project binary Release.
It checks required tools, downloads verified pinned bootstrap components into
its owned root, resolves remote `main` and prepares one exact source commit.
Subsequent repeat runs check remote `main`; an unchanged commit with a healthy
runtime avoids another build. A source acquisition or bootstrap failure leaves
the accepted version selected.

Private Git and uv, CPython 3.14.2, Rust 1.95.0, vcpkg/Rubber Band and
FFmpeg/FFprobe use installation-owned tools/caches and process-local environment
variables. Developer environments and persistent user PATH remain untouched.
Missing Microsoft C++ Build Tools/Windows SDK use the supported signed Microsoft
installer with Windows elevation. Those build components are system-wide and
do not move with the application. Declined elevation or required restart stops
before activation and provides a clear rerun instruction.

An owned source/build candidate produces a non-editable Release extension and
private CPython runtime with non-development dependencies, native DLLs,
FFmpeg/FFprobe, keynet and licenses. Candidate work does not copy the developer
virtual environment, audio, settings, project assets or model checkpoints.
Pinned download hashes and archive containment are verified before execution
or extraction. Source/tool preparation and preflight happen outside realtime
audio and do not open the GUI or an audio device.

Native preparation uses a temporary free DOS drive mapped to the owned root,
avoiding MSVC/CMake long-path limits and build-path separators. Existing mappings
are retained; only that exact temporary mapping is removed in the build's finally
path. The installed runtime is checked and started at its real path afterward.

The installation separates `tools`, `cache`, `b`, `versions`, `data`, `models`
and `logs`. The short owned `b` native target directory also bounds physical
dependency paths that Cargo embeds after canonicalizing the temporary drive.
A lock serializes installers. A candidate passes isolated imports,
private loaded-DLL checks and offline-worker/FFmpeg checks before atomic
activation via `current.json`. Existing runtime directories and all persistent
data remain intact, including for a running old process. Unsafe, reparse or
unowned nonempty installation paths are rejected.

A stable root dispatcher resolves the pointer once and invokes that exact
version's launch script. `START-LOOPER.bat`, `start.bat` and `start-release.bat`
select the private Release runtime; normal starts do not install or build.
Candidate preflight and actual starts therefore use the same version-specific
setup. The whole completed installation can move; launch/runtime/data/model
paths resolve from its new root. System drivers and Microsoft build components
remain installed on their Windows host.

Launches use stable `data` as their working directory and explicit local model,
Torch/cache, FFmpeg and DLL paths. The existing standard model cache remains
the default for developer launches; Portable supplies an explicit override.
Optional Beat This diagnostic setup is separate from the required normal app.

Operational build/runtime paths preserve short DOS aliases; canonical paths are
used only for private-origin/containment checks. Portable-only `sitecustomize.py`
normalizes long absolute ctypes DLL filenames to Windows verbatim paths when a
dependency canonicalizes an alias. It loads in the private interpreter and its
offline child processes, without altering developer Python or dependency files.

Runtime ABI, locks, native DLL/toolchain layout, source layout and launch/data/
model path changes must adapt the BAT, helpers, tests and portable documentation
together. The previous bundle builder remains available for advanced maintainer
packaging, without gating ordinary installation on its ZIP or manifest.

Focused checks cover missing-tool bootstrap, unchanged `main`, changed-commit
activation, preparation/preflight failures, retained data/old runtimes and
relocation. First installation has substantial downloads, builds and disk use;
the UI records progress and failure logs rather than promises a fixed duration.
Configured developer-machine checks do not establish clean-Windows bootstrap
or Human GUI/audio/device/listening acceptance. Those results stay pending
until the corresponding actual runs occur.
