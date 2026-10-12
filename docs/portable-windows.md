# Portable Looper for Windows x64

`INSTALL-LOOPER-PORTABLE.bat` installs the current remote `main` from source.
Copy that single file into your chosen folder and double-click it. No terminal
commands or preinstalled Git, Python or Rust are needed. It creates
`Flitzis-Looper` beside the BAT and checks and acquires the required build tools.
Internet access is needed for installation and update checks. A project Release,
local bundle channel or developer checkout is not required.

## Install, start and update

The installer acquires private Git and uv, CPython 3.14.2, Rust 1.95.0,
vcpkg/Rubber Band and FFmpeg/FFprobe as required, then builds a non-editable
Release runtime from one exact remote `main` commit. Its own tools and caches
stay under the installation root; it does not change the developer environment
or persistent user PATH. Required tool downloads are pinned and verified before
execution.

During compilation the installer temporarily maps a free drive letter to the
chosen installation folder, because Microsoft build tools can reject long paths.
It keeps all files in that folder, preserves existing drive mappings and removes
its own mapping when the build ends. A starter can acquire a fresh mapping for
long runtime DLL paths, keep it while the app runs, then remove it. Starts do not
depend on a drive letter left over from installation.

Missing Microsoft C++ Build Tools and the Windows SDK are installed using the
supported Microsoft installer. These are shared Windows build components:
Windows may show a UAC administrator prompt, and their installation may require
a restart. Approve the Windows prompt if you want to continue. If elevation is
declined or a restart is required, installation stops without activating a
partial runtime. After the required restart, double-click the BAT again.

The first installation downloads and builds a substantial toolchain as well as
the application dependencies. Downloads, build caches and retained runtimes can
require many gigabytes, and initial compilation can take a long time. There is
no fixed time or disk-size promise: allow additional space for models, samples
and later versions. Progress and build logs help identify a failed step.

Start the installed application with `Flitzis-Looper/START-LOOPER.bat`. Its
`start.bat` and `start-release.bat` aliases select the same private Release
runtime; ordinary starts perform no build, dependency installation or update.

Run the installer again to check the current remote `main`. If the installed
commit is unchanged and its private runtime is healthy, it avoids another build.
A changed commit is built in fresh owned directories and checked without opening
the GUI or an audio device. Only after that preflight succeeds does
`current.json` change atomically. The next app start uses the new runtime and its
matching launch logic. Existing processes keep their retained old runtime files.
Failed updates retain the previous selection, runtimes and persistent user data.

The installation keeps these areas separate:

- `data`: projects, samples and settings used by the application.
- `models`: persistent separator/model caches.
- `tools`: installation-owned bootstrap and build tools.
- `cache`: downloads, source/build preparation and failed installation diagnostics.
- `versions`: complete private runtimes selected by `current.json`.
- `logs`: installation-local diagnostics.

The completed application, private runtime, data and models can be moved by
moving the whole `Flitzis-Looper` directory. Its starters resolve runtime and
data/model paths from the new location. Shared Microsoft Build Tools/SDK and
Windows GPU/audio drivers remain system components; they are not relocatable
parts of the application directory. Installing an update on another Windows
computer checks that computer's required build components again.

Copying only a starter or one version does not move the whole installation.
Audio imported by reference may still use external paths chosen by the user.
Separator checkpoints use the existing model setup and remain persistent across
updates; developer models, projects and settings are not copied. Separate
Portable and developer runtimes do not share settings or installed modules;
simultaneous apps still use the same CPU/GPU and audio devices.

For automation, the BAT accepts an optional target directory followed by
`--no-pause`. `START-LOOPER.bat --check` runs the isolated import/DLL/FFmpeg/worker
preflight without opening the GUI or an audio device. An installer lock
serializes concurrent updates. Nonempty unowned targets and reparse installation
paths are rejected; downloaded archives reject unsafe paths, duplicates and
symlinks.

## Installer maintenance and validation

Changes to dependency locks, Python/Rust ABI, MSVC/SDK/vcpkg requirements, native
DLLs, source layout, launch/data/model paths or FFmpeg must review and adapt the
BAT, `scripts/portable`, focused tests and this document in the same delivery.
The source installer must continue to work without a project binary release.

Focused missing-tool, unchanged-main, update, failure and relocation checks are
required when those paths change. Passing tests on a configured developer
computer does not establish a clean-Windows installation. Clean Windows x64
bootstrap and Human GUI/audio/device/listening acceptance remain separate gates;
they must be recorded from actual runs before being described as passed.

## Advanced maintainer bundle utility

`scripts/portable/build_bundle.py` remains an advanced packaging utility for an
already synchronized Windows x64 CPython 3.14 developer environment. It is not
the ordinary installation or update path. It reads installed non-development
dependency RECORDs and copies files without replacing the developer runtime.
The base interpreter's `site-packages` is excluded before copying the declared
dependency closure. A fresh non-editable Release wheel and its full Native source
commit are required; changed Native inputs are rejected. Runtime imports,
private loaded DLL paths and offline-worker imports must pass before packaging.

Required arguments are `--native-wheel`, `--native-source-commit`, `--ffmpeg-dir`,
one or more `--dll-dir`, external `--license-file NAME=PATH` inputs and `--output`
outside the repository. The output directory must be empty or owned by the
bundle utility. Application source comes from tracked `src/flitzis_looper`
files; private projects, settings and checkpoints are excluded. A generated ZIP
or manifest is a maintainer artifact, not a prerequisite for the standalone BAT.

The external license inventory includes `rubberband.txt`, `samplerate.txt`,
`sleef.txt`, `ffmpeg.txt`, `ffmpeg-README.txt`, `antlr-4.9.3.txt`,
`onnxruntime-1.24.2.txt`, `onnxruntime-1.24.2-notices.txt`,
`directml-1.15.4.html`, `msvc-2026.docx` and `source-urls.json`.
Python/package licenses and project/vendor notices are retained. Required DLL
names are declared in the builder, including the redistributable VC runtime;
do not source them from System32.

Clean reviewed source is the builder default. Its `--allow-dirty` flag is only an
explicit local candidate option and records the base commit, Native source
commit and actual source fingerprint. Optional Beat This diagnostic setup
remains a separate environment workflow.
