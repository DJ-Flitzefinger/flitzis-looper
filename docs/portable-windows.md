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

The optional `signalsmith-probe` Cargo feature is a development diagnostic.
Ordinary locked Portable builds do not enable it, compile its C++ shim or add a
Signalsmith runtime DLL. Its pinned MIT headers ship as tracked source; existing
MSVC/SDK selection and the productive RubberBand dependency remain unchanged.

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
  successful operation stages and fully owned short backups are removed. Previous
  checkouts containing unregistered contents are preserved in full as described below.
- `b`: reusable Native build output; its short name protects linker paths.
- `logs`: installation-local diagnostics.

The runtime preflight also requires `AudioEngine.pad_source_sync_status(id)` by
reflection, before starting an engine or opening a device. Source-SYNC intent is
separate from BPM Lock; saved intent and restored map metadata never substitute
for callback-confirmed mapped playback.

Ordinary Pause/Resume now requires `AudioEngine.ORDINARY_TRANSPORT_API_VERSION = 1`,
the immutable admission and observation types, their durable current-state queries
and the distinct ordinary feedback message. The current-slot observation query
also ensures a missing lifecycle event or a new source cannot strand the pause
display. Installer activation and the local launcher reflect the complete
interface before App start; an older binary that
only enqueues pause commands is rejected. The existing locked Release build
provides this API without another dependency or installation stage. Admission
alone does not confirm a pause, and this compatibility check does not establish
full SYNC or listening acceptance.

Native source-beatmap exports store immutable `map.json` sidecars under
`samples/<material>/.pcm-cache/source-beatmaps/v1/.ready-<32 hex>/`, or directly
under `samples/.pcm-cache/source-beatmaps/v1/` for flat legacy originals. The
existing sealed sample-tree relocation retains these files and their identities
without a bootstrap change. A compact receipt still needs fresh source
qualification after reopening; relocation grants no current map/audio authority.
This prerequisite does not yet save selected maps in Config or arrangement journals.

Portable validation and the local launchers require the exact integer
`SourceBeatmap.EDITOR_CORRECTION_API_VERSION = 4`, the existing bounded
`editor_span(first_anchor_index, last_anchor_index)` signature and the embedded
capture methods `editor_review_info()` and
`editor_review_page(first_anchor_index, last_anchor_index, start_index, max_events=128)`
before opening the App. Captured maps can be reviewed after transient candidate
discard or restore; legacy maps report this feature unavailable and retain their
existing edits. Native builds with only the earlier editor methods are refused.
These checks reflect the API without creating an Engine, evaluating a map or
starting an editor job. Explicit review labels do not qualify musical coverage.
Capability4 also permits an explicit detector-missed beat at a physical original
source sample, bound to the current map, embedded capture and fixed endpoints.
It preserves complete detector evidence; new intervals remain uncertain with
unknown meter and phase until separately reviewed. Earlier capability3 builds
are refused before opening the App.

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
the callback's pending, applied or rejected rate effect. Missing methods, receipt fields or
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

### Source Map editor Native compatibility

The launcher and Portable preflight require the manual Source Map creation/edit
and original-candidate review APIs, plus the shared Native editor/coordinate
queries. Portable reflects the review call's required keyword-only source
generation, original candidate and separate intent arguments, the create call's
optional `expected_source_generation=None` fence, and bounded query defaults
without constructing an Engine, opening a device or running jobs.
A stale Native module is rejected before the App starts; rebuild/update the existing
installation through its ordinary workflow. This API check does not establish
musical map quality, live-map audio adoption or device/Human acceptance.
