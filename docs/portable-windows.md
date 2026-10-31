# Portable Looper for Windows x64

`INSTALL-LOOPER-PORTABLE.bat` installs the current remote `main` from source.
Copy that single file into your chosen folder and double-click it. It creates
`Flitzis-Looper` beside the BAT and acquires the required build tools. No terminal
commands, preinstalled Git/Python/Rust or project binary release are required.
Internet access is needed for installation, updates and missing model downloads.

The BAT is independent of a development workspace or Codex installation. It
fetches one exact current remote `main` revision, including its portable recipes.
Use the updated installer BAT when updating an older installation.

The optional native BeatThis developer trial is separate from this installer.
Its full-source CPU prototype and optional CUDA build do not replace the pinned
productive workers, locks or model recovery. A Debug EXE, a separately built
libsoxr and extracted ORT DLLs are not a supported Portable bundle. Native CUDA,
CPU fallback/CPU-only, all dependency/provenance and Linux/macOS acceptance remain
open as detailed in the [performance design](../openspec/changes/improve-performance-isolation/design.md).

## Install, start and update

The installer pins and verifies its private Git, uv, CPython 3.14.2, Rust 1.95.0,
vcpkg/Rubber Band and FFmpeg/FFprobe inputs, then builds a private Release runtime
from one exact remote `main` commit. It keeps one complete Git checkout on `main`
at `Flitzis-Looper/flitzis-looper`, including tracked source, assets, vendor and
build files. `Flitzis-Looper/Runtime` holds the private Python interpreter,
dependencies, FFmpeg and launch helpers. Native binaries sit beside their tracked
wrappers in the checkout. The app's working directory is this checkout.

The optional `signalsmith-probe` Cargo feature is a development diagnostic.
Ordinary locked Portable builds do not enable it, compile its C++ shim or add a
Signalsmith runtime DLL. Its pinned MIT headers ship as tracked source; existing
MSVC/SDK selection and the productive RubberBand dependency remain unchanged.

The bounded stem converter adds a dependency-free Rust workspace crate inside
the same Native extension. It changes no Python ABI, installer path, external
DLL or inference dependency. Runtime preflight checks its numeric API and v3
compiled-source processing identity before activating the matching Python code.
The arithmetic core has no OS/Python dependency; Windows host promises use the
existing memory adapter. Other OSes retain the hard PCM cap with explicitly
unsupported host monitoring until a platform adapter is supplied. Windows tests
alone do not certify Linux/macOS builds or distribution.

Start the installed app with `Flitzis-Looper/START-LOOPER.bat`. The `start.bat` and
`start-release.bat` aliases select the same private Release runtime. Ordinary
starts perform no build, dependency installation, update or model preflight.
`START-LOOPER.bat --check` runs the isolated runtime import/DLL/FFmpeg/offline-worker
checks without opening the GUI or an audio device.

The piano-control revision requires Native's immutable
`CapturedPitchAction.resets_key_correction` receipt field. The installer preflight
rejects an older incompatible binary before activation. Saved schema-1 key intent
remains compatible: a missing independent offset-retrigger preference inherits
the old shared preference, while note octaves and both explicit preferences
survive project and portable relocation. Existing running installations are
protected by the same update lock.

Every successful installation or update also prepares `demucs:htdemucs` and
`bs-roformer:musdb18hq` using the existing model resolvers. It verifies checkpoint
assets and loads each real model on CPU in a separate fresh process, sequentially.
Model failures prevent activation. Running the installer on an unchanged healthy
`main` repairs missing or corrupt model assets and repeats both CPU loads without
invoking the Native builder.

The installer also prepares a pinned BeatThis worker under
`analysis-runtime/beat-this`. Fresh installations use the existing CPU profile
(CPython3.12.13, Torch/Torchaudio2.8.0+cpu); a valid selected CUDA-auto-v2 profile
is retained on update. The worker includes a physical private interpreter,
standard library and locked packages with relative search paths. Its runtime
does not depend on the provisioning interpreter, a host venv or the install
folder's original name. Worker verification checks the checkpoint, receipts and
actual import environment without inference. An absent worker is an error,
not successful App readiness.

Candidate bundle creation first checks its Python/Native/codec runtime. The
installer then selects a verified worker and performs the complete preflight
before activating the candidate. Failed updates restore the previous worker
selection; unchanged-main repair does not require a Native rebuild.

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
- `flitzis-looper/record`: recordings and recovery custody, transferred with
  their file identities during updates and restored on rollback.
- `analysis-runtime/beat-this`: private pinned analysis installations and one
  verified active pointer; previous installations are retained.
- `Runtime`: the fixed private interpreter, dependencies, FFmpeg and launch files.
- `models`: persistent model caches shared by install checks and offline workers.
- `tools`: verified bootstrap and build tools.
- `cache`: useful package preparation caches and bounded failed-install diagnostics;
  successful operation stages and fully owned short backups are removed. Previous
  checkouts containing unregistered contents are preserved in full as described below.
- `cache/ui`: installation-local window geometry and UI layout; the app does not
  read or update another installation's user-profile layout file.
- `b`: reusable Native build output; its short name protects linker paths.
- `logs`: installation-local diagnostics.

Stem asset acquisition uses the same resumable pinned transfer helper before
network-forbidden CPU checks; legacy library downloaders are not used by the
Portable repair path.

Downloads retain stable partial files and resume transient interruptions with
interruptible backoff and no overall deadline. SHA256 pins are checked before
publication. Permanent unavailable/authentication responses fail explicitly and
retain the partial evidence. Portable launch and child processes bind package,
model and compute caches to this installation, ignoring inherited personal
workspace bindings.

The runtime preflight enforces the current constant-BPM and ordinary-transport
API, including immutable admission/observation receipts and their current-state
queries. Source SYNC, REALTIME BPM, tempo-map adoption and phase-following are
retired on main; old map/editor prerequisites are historical, not current
installation gates. Native checks do not establish hearing/device acceptance.

There is no second `Runtime/app` copy and no permanent per-version runtime tree.
The full checkout's installed-file inventory distinguishes shipped files from
Human changes. A recognized old checkout containing unregistered files, such as
an older `.venv/.gitignore`, or unregistered empty directories can be updated
without deleting them. The installer prepares a clean source/runtime even if
`main` is unchanged and never executes or copies those extras into the new checkout.
After data relocation and verification, it preserves the entire previous checkout
at its existing `cache/o-<operation>` path, reports the actual path and finishes
the update. Keep that backup until you have reviewed its contents yourself. The
new checkout resumes ordinary unchanged-main updates without repeated Native builds.
Models, tools, downloads and useful package/build caches keep their existing reuse
paths; missing or changed required assets can still need acquisition.

A recognized previous checkout also has one narrow exception for a changed
generated Native binary at
`src/flitzis_looper_audio/flitzis_looper_audio.cp314-win_amd64.pyd`.
The file must still exist, and its accepted `native_sha256` must match its original
installed-file inventory hash. The installer treats its changed bytes as contents
to preserve, builds and verifies a clean candidate even on unchanged `main`, and
retains the entire old checkout. It never imports or copies the changed previous
binary. This does not establish why that file changed. Missing Native, missing or
inconsistent declarations, other changed registered source, Runtime changes and
candidate Native changes still stop the relevant operation safely.

New manifests record checkout directories as well as files. Older file-only
manifests cannot prove ownership of otherwise empty folders, so one update may
conservatively retain an old checkout even without additional files. A directory
name alone never authorizes recursive deletion. Changed or missing registered
source files outside that exact previous generated-Native preservation case still
stop activation with their concrete preserved path. Existing
Runtime files, including licenses and DLLs, must also match their recorded
hashes; unknown or replacement bytes stop updates and retired-runtime cleanup. Missing
Runtime files remain eligible for repair. Files are not removed based only on
a directory name. Completed downloads and
successful operation-owned environment, wheel, license and bundle stages are
removed. The installer reports retained tool/build cache sizes and their reuse
purpose; it does not recursively purge caches merely to reduce disk usage.
Recovery validates complete candidate source/Runtime files, hashes and reparse guards
before running relocation, private preflight or model checks. Source directory
ownership is also checked; Runtime directory handling remains unchanged. Unknown
source files/directories or unknown/tampered Runtime files remain
preserved with their recovery evidence. Unknown installation ownership and links
remain refusals; checkout-extra preservation does not relax those boundaries.

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
Pair acknowledgement validation and fresh Pair and target-only occupied Copy
recovery acceptance. It also checks
original-ticket Hold release, captured MIDI trigger/window admission, owned pad
feedback, original timing-ticket/refresh Pending observations and the Native
placement-marker/read-token queries, resident-admission protocol version 1, the
budget setter/status API and the independent preload-interest parameter. These
checks also require the complete Native pitch interface: immutable
`CapturedPitchAction` receipts, native selection/projection, GUI and selected MIDI
capture with original timestamps, publish/cancel, input polling and atomic
`pitch_rows` runtime publication, plus the atomic source-restore keywords
`initial_base_shift`, `initial_extra_shift`, `initial_key_lock`, `initial_gain_db`
and `initial_eq_db`. The complete saved mixing tuple is installed inside the
original restored source claim before acknowledgement. Ordinary starts
also require the `CapturedPadStart` observations/cancellation methods and
`capture_pad_start`, `prepare_captured_pad_start` and `publish_pad_start`, including
optional original timestamp/source-generation/exclusive capture keywords and the
default `window=None` publication parameter. The callable `global_rate_status`
observation is also required so requested speed settings can be distinguished from
the callback's pending, applied or rejected rate effect. The class must declare
`GLOBAL_RATE_PROVENANCE_API_VERSION=1`: the same coherent observation carries
actual master provenance, distinguishing a qualified Manual/TAP/Legacy reference
from an unknown origin or a qualified accepted revision. Binding metadata carries
the matching original-source correlation without granting execution authority.
Missing methods, receipt fields or
incompatible keyword defaults reject the runtime before activation. These
checks inspect the built Release
class without constructing an engine or opening an audio device; an older
incompatible extension cannot activate the current Python controls. The source
installer builds these APIs from the selected main checkout through its existing
private-runtime pipeline. The installer BAT and dependency/toolchain inputs do not
need a new installation stage for these control APIs.

Mix Capture adds one reflected API family: the opaque `MixCaptureTicket` and its
`session_id()` observation, `prepare_mix_capture` with bounded-pool defaults
4096 frames / 32 blocks / 8388608 PCM bytes, explicit-ticket start/stop with
`target_frame=None` for Native next-admissible timing (or an explicit frame),
cancellation and status. GLOBAL publication must also expose the keyword-only
`capture=None` parameter for attaching the original recording ticket to the
original group. Cancellation reports whether it won before
the writer's terminal decision; close, join and RT release remain separate.
Preflight also requires the module-level
`inspect_mix_capture_spool(path)` recovery-metadata inspector. It checks their
callable signatures and defaults without constructing an engine, preparing a
capture, inspecting a spool or writing files. An older binary missing this
family cannot activate the current checkout.

Recording preflight also reflects `audio_stream_failed()` and
`quiesce_failed_audio_stream()` without invoking them. These keep the original
stream failure separate from the first capture failure; positive quiescence
requires actual checked callback-image retirement. Writer/file finalization and
outer worker retirement remain separately required. Ordinary development starters
also reject a Native binary missing these lifecycle methods before starting App.

These interfaces capture an internal float spool. A closed successful spool is
not an encoded or publicly published recording. The separate inspector reads
bounded recovery metadata, always requiring verification and granting no publication
authority even for a `sealed_candidate`; unknown or corrupt spools are rejected
without modification. API compatibility supplies no RF64/codec, RECORD UI, device,
listening or complete recording acceptance. The existing source-build pipeline
provides the APIs; no additional installer or dependency stage is introduced.

The ordinary installer always selects remote `main`. Local uncommitted changes
are not included by double-clicking the BAT. A development test of such changes
requires a separately labelled complete source snapshot and a matching fresh
Native Release; its runtime and project data must remain separate from development.

Native Samples relocation validates complete Legacy Material, Pair and occupied
Copy journal histories with their distinct schemas before creating relocation
output. Musical seals, actual receipts and successor lineage remain intact across
the directory move and rollback, with original journal bytes and FileIDs preserved.
Historical Config addresses and their moved aliases resolve to the same held files.
Known foreign Config contents are not read. These installer checks grant no runtime
source, timing or stem acceptance; the application acquires fresh owners on restore.

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

Preflight also reflects the opaque `CapturedGlobalStart` and the engine's
`capture_global_start` / `publish_captured_global_start` interface without
constructing an engine or invoking native preparation. A runtime missing this
contract cannot activate: deferred GLOBAL gestures must retain their original
native participants and actual resident dependency rather than recapture after a wait.

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

### Current constant-BPM Native compatibility

The launcher and Portable preflight validate the supported constant-BPM Native
profile before startup. The Portable bootstrap explicitly rejects the removed
`SourceBeatmap` API; an obsolete map-enabled engine is not a valid current runtime.
Update through the ordinary installation workflow. API checks establish neither
model quality nor audio/device/Human acceptance.

The matching Native must also expose `OfflineChildMemoryReservation.limit_bytes`
and `close`; preactivation rejects an older extension before offline imports use
the shared commit budget. This adds no dependency or OS setting. Windows uses the
existing suspended Job Object adapter; Linux/macOS process-tree and host-monitoring
evidence remains a separate distribution gate. No running installation is updated
by development verification.

The opt-in native BeatThis complete-source experiment is not part of installation or
model repair. Its ONNX graph and standalone developer EXE have no productive
cutover or complete CUDA/CPU-only/quality/DLL-closure acceptance. Keep the original
App and separate BeatThis locks/models/workers; their runtime removal requires
the complete parity and recovery gates in the
[performance design](../openspec/changes/improve-performance-isolation/design.md).

Its separate `native-inference-dynamic` CPU-only and CUDA trial builds now require
explicit digest-checked runtime manifests. That standalone loading contract does
not alter Portable runtime versions, bootstrap pins, licenses, model repair or
recovery. Optional separately authenticated declared ORT component manifests pin
the listed files through all standalone CLI work; they do not attest actual loader
resolution, mapped dependencies or full producer/CRT closure. The runtime-only
contract and productive Portable bootstrap remain unchanged. Extracted trial DLLs
and a developer Debug EXE are not an installable
bundle; CRT/provider/driver closures, redistribution obligations and clean-system
acceptance remain mandatory before any productive runtime replacement.
