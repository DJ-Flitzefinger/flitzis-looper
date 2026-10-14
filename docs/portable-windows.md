# Portable Looper for Windows x64

`INSTALL-LOOPER-PORTABLE.bat` installs the current remote `main` from source.
Copy that single file into your chosen folder and double-click it. It creates
`Flitzis-Looper` beside the BAT and acquires the required build tools. No terminal
commands, preinstalled Git/Python/Rust or project binary release are required.
Internet access is needed for installation, updates and missing model downloads.

## Install, start and update

The installer pins and verifies its private Git, uv, CPython 3.14.2, Rust 1.95.0,
vcpkg/Rubber Band and FFmpeg/FFprobe inputs, then builds a private Release runtime
from one exact remote `main` commit. It keeps one complete Git checkout on `main`
at `Flitzis-Looper/flitzis-looper`, including tracked source, assets, vendor and
build files. `Flitzis-Looper/Runtime` holds the private Python interpreter,
dependencies, FFmpeg and launch helpers. Native binaries sit beside their tracked
wrappers in the checkout. The app's working directory is this checkout.

Start the installed app with `Flitzis-Looper/START-LOOPER.bat`. The `start.bat` and
`start-release.bat` aliases select the same private Release runtime. Ordinary
starts perform no build, dependency installation, update or model preflight.
`START-LOOPER.bat --check` runs the isolated runtime import/DLL/FFmpeg/offline-worker
checks without opening the GUI or an audio device.

Every successful installation or update also prepares `demucs:htdemucs` and
`bs-roformer:musdb18hq` using the existing model resolvers. It verifies checkpoint
assets and loads each real model on CPU in a separate fresh process, sequentially.
Model failures prevent activation. Running the installer on an unchanged healthy
`main` repairs missing or corrupt model assets and repeats both CPU loads without
invoking the Native builder.

The starter holds the same exclusive installation lock as the updater for the
entire private app process lifetime. Updates stop with a close-and-retry message
when the app is running. They also detect an older installation's actual private
Python processes, whose starters did not use that shared lock. The installer
does not terminate them. Native data custody guards additionally protect against
an active material owner.

Changed source or a damaged private runtime is prepared in short owned cache
directories. The installer validates the complete candidate checkout, runtime,
both models and existing file ownership before activation. A durable transaction
journal covers short checkout/runtime backups, data transfer and starter
selection. Repeating the same BAT after an interruption restores the previous
installation or finishes cleanup of a fully verified accepted candidate before
starting another update. A starter refuses an unfinished transaction and points
to that recovery step.

The installation uses these areas:

- `flitzis-looper/samples`: project samples, PCM caches, material inventories and
  `flitzis_looper.config.json`.
- `flitzis-looper/config/input`: persisted input configuration.
- `Runtime`: the fixed private interpreter, dependencies, FFmpeg and launch files.
- `models`: persistent model caches shared by install checks and offline workers.
- `tools`: verified bootstrap and build tools.
- `cache`: useful package preparation caches and bounded failed-install diagnostics;
  successful operation stages and short backups are removed.
- `b`: reusable Native build output; its short name protects linker paths.
- `logs`: installation-local diagnostics.

There is no second `Runtime/app` copy and no permanent per-version runtime tree.
The full checkout's installed-file inventory distinguishes shipped files from
Human changes. Unknown files or changed tracked files outside persistent sample
and input areas stop activation with their concrete preserved path. Existing
Runtime files, including licenses and DLLs, must also match their recorded
hashes; replacement bytes stop updates and retired-runtime cleanup. Missing
Runtime files remain eligible for repair. Files are not removed based only on
a directory name. Completed downloads and
successful operation-owned environment, wheel, license and bundle stages are
removed. The installer reports retained tool/build cache sizes and their reuse
purpose; it does not recursively purge caches merely to reduce disk usage.

An older recognized `versions/main-...` plus `data` installation migrates once.
The installer proves the old runtime's files from its exact source revision,
pinned interpreter/FFmpeg and installed distribution inventories. Native capture
binds the real sample directory, config files and journals before any code swap.
Native moves that same directory into the final checkout and projects location
references while preserving material birth ownership, latest config state and
external references. Input files move intact. Unknown old runtime files,
ambiguous configs, active owners, reparse points, destination collisions or
unsupported custody capabilities stop before swaps and keep the old installation.
Sample migration requires same-volume Windows FileIDs and directory alternate
data streams; NTFS provides the tested capability. Unrelated files left in the
old `data` area remain preserved.

Temporary drive mappings protect long paths during builds and runtime DLL
loading. All files stay in the chosen folder. Each operation acquires a free
letter, preserves existing mappings, keeps its own mapping for its child process
lifetime and removes it afterward. Starts do not depend on an installation-time
drive letter. Reparse installation paths, unsafe archive paths, duplicate archive
entries and archive symlinks are rejected.

Missing Microsoft C++ Build Tools and the Windows SDK use Microsoft's supported
installer. These shared Windows components may show a UAC prompt and require a
restart. Declining elevation or requiring a reboot stops without activating a
partial runtime; after the restart, run the BAT again. The private installation
does not change the developer environment or persistent user PATH.

The first installation downloads a substantial toolchain, application dependencies
and both separator checkpoints. Compilation and real CPU model loads can take a
long time and require substantial disk and memory. There is no fixed time or disk
size promise. Logs identify the failed stage. Developer settings, samples and
models are not copied into the installation. Referenced external audio keeps the
external paths chosen by the user. Shared Build Tools/SDK and GPU/audio drivers
remain system components. For automation, the BAT accepts an optional target
directory followed by `--no-pause`.

## Installer maintenance and validation

Changes to dependencies, ABI, toolchain, Native DLLs, source layout, data/model
paths or FFmpeg must review the BAT, `scripts/portable`, focused tests and this
document together. Keep the Native relocation contract authoritative; installer
code must not copy sample trees or rewrite JSON references independently.

The runtime activation preflight checks the complete Native pad Copy and internal
Pair capture/prepare/publish/reconcile/finish/cancel interfaces, including opaque
Pair acknowledgement validation and fresh recovery acceptance. It also checks
original-ticket Hold release, captured MIDI trigger/window admission, owned pad
feedback, original timing-ticket/refresh Pending observations and the Native
placement-marker/read-token queries, resident-admission protocol version 1, the
budget setter/status API and the independent preload-interest parameter. These
checks inspect the built Release
class without constructing an engine or opening an audio device; an older
incompatible extension cannot activate the current Python controls. The source
installer builds these APIs from the selected main checkout through its existing
private-runtime pipeline. The installer BAT and dependency/toolchain inputs do not
need a new installation stage for these control APIs.

Focused missing-tool, deep-path/SUBST, DLL isolation, unchanged-main model repair,
update/failure/recovery, legacy migration, ownership and actual-process lifetime
checks are required when these paths change. Passing tests on a configured
developer computer does not establish a clean-Windows bootstrap. Clean Windows
x64 installation and Human GUI/audio/device/listening acceptance remain separate
gates and must be recorded from actual runs before being described as passed.

## Advanced maintainer bundle utility

`scripts/portable/build_bundle.py` remains an advanced utility for an already
synchronized Windows x64 CPython 3.14 developer environment. Its ZIP uses the same
full `flitzis-looper` checkout plus fixed `Runtime` layout and starter contract.
It reads installed non-development dependency RECORDs, excludes the base
interpreter's `site-packages`, and copies the declared dependency closure without
replacing the developer runtime. A fresh non-editable Release wheel and exact
Native source commit are required. Runtime imports, private loaded DLL paths and
offline-worker imports must pass before packaging. The source BAT remains the
ordinary install/update path and performs the model preparation gate.

Required arguments are `--native-wheel`, `--native-source-commit`, `--ffmpeg-dir`,
one or more `--dll-dir`, external `--license-file NAME=PATH` inputs and `--output`
outside the repository. The output must be empty or utility-owned. Private
projects, input settings and checkpoints are excluded. The external license
inventory includes `rubberband.txt`, `samplerate.txt`, `sleef.txt`, `ffmpeg.txt`,
`ffmpeg-README.txt`, `antlr-4.9.3.txt`, `onnxruntime-1.24.2.txt`,
`onnxruntime-1.24.2-notices.txt`, `directml-1.15.4.html`, `msvc-2026.docx` and
`source-urls.json`. Python/package licenses and project/vendor notices are kept.
DLL names, including the redistributable VC runtime, are declared in the builder;
do not source them from System32.

Clean reviewed source is the builder default. `--allow-dirty` is an explicit local
candidate option recording the base commit, Native source commit and source
fingerprint; the ordinary installer rejects such candidates. Optional Beat This
diagnostic setup remains a separate environment workflow.
