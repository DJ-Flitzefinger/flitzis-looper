# Portable Looper for Windows x64

Portable runs from its own directory with private CPython, the compiled Release
extension, application dependencies, DLLs, FFmpeg/FFprobe and keynet. Git,
Python, Rust and developer terminal commands are not required on the target.
The GPU/audio drivers remain part of Windows. Separator model checkpoints are
installed through the existing model setup; developer models/settings are not copied.

## Install, start and update

For the current local test, use the personal `INSTALL-LOOPER-PORTABLE.bat` in the
workspace's `portable-releases` directory. Copy only that BAT into the desired
folder on this computer and double-click it. It creates `Flitzis-Looper` beside
the BAT. Keep the source `portable-releases` channel available for future updates.
The generic BAT in the repository also discovers that adjacent workspace channel
when run from this checkout.

Start the installed application with `Flitzis-Looper/START-LOOPER.bat`. Its
`start.bat` and `start-release.bat` aliases also use the same private Release
runtime; they perform no build, dependency install or update. The development
checkout's existing launchers retain their existing behavior.

Run the installer again to check for a newer bundle. An unchanged package is not
downloaded again. A new package is verified and checked without GUI/audio startup
in a fresh version directory before `current.json` changes atomically. Old
runtimes remain available to existing processes. The next app start selects the
new runtime and its matching start logic. Failed updates retain the previous
pointer, runtimes and user data. The updater keeps old versions and downloaded
archives; plan at least 7 GB initially, plus space for samples/models and updates.

The installation contains:

- `data`: projects, samples and settings used by the application.
- `models`: persistent separator/model caches.
- `cache`: temporary work, downloads and failed installation diagnostics.
- `versions`: complete immutable runtimes selected by `current.json`.
- `logs`: reserved installation-local diagnostics.

The entire installed directory can be moved together. Internal starters and
runtime/data/model paths are resolved from its new location. Copying only a
starter or an individual version does not move the whole application.
Audio imported by reference can still depend on the user's chosen external paths.
Separate Portable and development runtimes do not share settings or installed
modules; simultaneous apps still use the same computer, CPU/GPU and audio devices.

For a non-interactive installation, the BAT accepts an optional target directory
followed by `--no-pause`. `START-LOOPER.bat --check` runs the isolated import/DLL/
FFmpeg/worker check without opening the GUI or an audio device.

## Public download channel

The generic standalone installer uses a HTTPS release manifest when no local
channel is configured. The repository currently has no published Portable binary
release. The personal local installer is ready for testing on this computer;
another computer needs the portable bundle/channel or a separately authorized
published HTTPS release. The installer reports a missing release as an error.

A channel manifest has schema version 1, a release ID, source commit, relative
immutable ZIP filename, byte size and SHA256. A local/HTTPS channel can be
explicitly supplied through `LOOPER_PORTABLE_CHANNEL` as a URI. Archives reject
escaping/ambiguous Windows paths, duplicate entries and symlinks. Nonempty
unowned targets and reparse installation paths are rejected.

## Preparing a bundle

Use `scripts/portable/build_bundle.py` from the already synchronized Windows x64
CPython 3.14 developer environment. It reads installed non-development dependency
RECORDs and copies files; it never installs into or replaces the developer runtime.
The base interpreter's `site-packages` is excluded before copying that dependency
closure. A fresh non-editable Release wheel must be supplied together with its
full Native source commit; changed Native inputs are rejected. Runtime imports,
private loaded DLL paths and offline-worker imports must pass before packaging.

Required arguments are `--native-wheel`, `--native-source-commit`, `--ffmpeg-dir`,
one or more `--dll-dir`, the external `--license-file NAME=PATH` inputs and
`--output` outside the repository. The output directory must be empty or owned
by the portable channel. The builder emits an immutable ZIP, a channel manifest
and the generated personal standalone BAT. Application source comes from tracked
`src/flitzis_looper` files; private projects, settings and checkpoints are excluded.

The external license inventory includes `rubberband.txt`, `samplerate.txt`,
`sleef.txt`, `ffmpeg.txt`, `ffmpeg-README.txt`, `antlr-4.9.3.txt`,
`onnxruntime-1.24.2.txt`, `onnxruntime-1.24.2-notices.txt`,
`directml-1.15.4.html`, `msvc-2026.docx` and `source-urls.json`.
Python/package licenses and the project/vendor notices are also retained.
The current required DLL names are declared in the builder, including the
redistributable VC runtime; do not source them from System32.

Clean reviewed source is the default. `--allow-dirty` is an explicit local-test
candidate option: the package records that fact, its base commit, Native source
commit and actual source fingerprint. The delivered first test bundle uses this
candidate option. It does not claim to be a clean published binary release.
Optional Beat This diagnostic setup remains a separate environment workflow.

GUI, audio-device and listening acceptance require the Human's actual test.
