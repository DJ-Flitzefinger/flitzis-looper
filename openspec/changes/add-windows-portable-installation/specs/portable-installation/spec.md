## ADDED Requirements

### Requirement: Independent Windows portable installation

The system SHALL provide `INSTALL-LOOPER-PORTABLE.bat` as a standalone Windows
x64 installer that creates a child installation from the latest repository
`main` without requiring preinstalled Git, Python, Rust or developer commands.
It MUST check required build components and automatically acquire missing ones.
Acquired Git, uv, Python, Rust and vcpkg SHALL use installation-owned tools/caches
rather than modify developer environments or persistent user PATH. A usable
installed Git MAY be reused read-only; missing Git SHALL be installed privately. Missing Microsoft
C++ Build Tools and Windows SDK SHALL be installed using the supported signed
Microsoft installer with Windows elevation when necessary. Required reboot or
declined elevation SHALL be reported without activating an incomplete runtime.
The installation MUST contain a compatible private Python runtime, Release
native extension, application, non-development dependencies, required DLLs,
FFmpeg/FFprobe, keynet and licenses. It SHALL include simple portable starters.
Installing or starting Portable MUST NOT modify the developer runtime/settings.
All package preparation and installation SHALL remain outside realtime audio.

#### Scenario: Beginner double-click installation from another folder

- **GIVEN** a compatible Windows x64 computer with internet access and no Git, Python or Rust installed
- **WHEN** the Human copies only the BAT to a folder and double-clicks it
- **THEN** it SHALL acquire verified missing tools, clone the current `main`, build its Release runtime and install an independent child directory with portable starters
- **AND** the starter SHALL use persistent installation-local data, models and caches, with no dependency on the developer checkout or environment

#### Scenario: Updates follow main without project releases

- **GIVEN** an existing installation and a reachable repository `main`
- **WHEN** the BAT is run again
- **THEN** it SHALL resolve the current remote `main` commit and build that exact source snapshot when it differs from the installed commit
- **AND** it MUST NOT require a project release, local bundle/channel, developer checkout or manually installed build tools
- **AND** unchanged source with a healthy runtime SHALL avoid another build

#### Scenario: Installation in a deep folder

- **GIVEN** a chosen installation folder with long paths or special characters
- **WHEN** native build tools require short paths
- **THEN** the installer SHALL use a free temporary drive mapping to that owned folder while retaining all files there
- **AND** it SHALL preserve existing mappings and remove only its own mapping after the build
- **AND** the completed application SHALL NOT depend on that build mapping; its starter SHALL acquire a fresh owned mapping when runtime DLL loading needs short paths, retain it until the process exits and then remove only that mapping

### Requirement: Safe repeatable portable updates

Repeated installation SHALL check remote `main`. An update MUST prepare a fresh
owned source/build candidate, verify tool downloads and archive containment, and
pass a private-runtime import/preflight without
starting the GUI/audio device before atomically selecting it. Persistent user
data/models/settings MUST remain intact on failure and after successful update.
Successful activation SHALL use one complete current-main checkout and one fixed
private runtime, with only transaction-owned temporary backups until verification
completes. Starts and updates SHALL share an exclusive installation lock; a start
SHALL hold it until its application process exits.

#### Scenario: Update while an old runtime exists

- **GIVEN** an accepted prior installation and saved installation-local data
- **WHEN** a newer exact `main` source runtime passes staging and preflight with no running installation-owned application
- **THEN** journaled activation SHALL select its complete checkout and fixed private runtime together and preserve every sample, PCM cache, configuration and input mapping
- **AND** interruption SHALL recover either the previously accepted installation or the fully verified new installation, without merging partial runtime files
- **AND** a running current or recognized legacy application, or an unqueryable relevant process owner, SHALL stop mutation with a clear close-and-rerun message and SHALL NOT be terminated

#### Scenario: An enumerated Python process exits before its path can be read

- **GIVEN** the owner check retains an original Python process object from its process snapshot
- **WHEN** its executable-path query fails or returns no path
- **THEN** the installer MAY ignore that object only after positively confirming that the same original process has exited
- **AND** a live process or unqueryable termination state SHALL still block mutation
- **AND** every enumerated process object SHALL be disposed on success or failure without terminating a process
- **AND** installation locking and physical-root path checks SHALL remain unchanged

#### Scenario: Corrupt or escaping bundle

- **GIVEN** an accepted prior version and failed source acquisition/build, a tool download with a wrong hash or unsafe archive path/symlink, or failed preflight
- **WHEN** installation is attempted
- **THEN** the update SHALL fail without changing the current pointer, existing runtime or user data
- **AND** unowned/nonempty target directories or reparse paths SHALL NOT be overwritten

### Requirement: Maintained source installer

The installer SHALL be reviewed and updated with its helpers/tests/docs in the
same change when Python/Rust ABI, dependency locks, native DLLs/toolchains,
source layout, runtime launch/persistence paths or model/FFmpeg prerequisites change.
Bootstrap tool downloads MUST be pinned and verified before execution. Build
logs and clear progress SHALL be retained for a beginner-facing failure report.

#### Scenario: Dependency or runtime change

- **WHEN** a repository change alters an installation or runtime prerequisite
- **THEN** its delivery SHALL include the corresponding installer adaptation and relevant clean-tool/bootstrap/update regression evidence

### Requirement: Explicit portable model cache

Portable launch SHALL resolve its separator model cache inside its persistent
installation directory. Ordinary developer launches SHALL retain the existing
standard user cache when no explicit Portable override is supplied.

#### Scenario: Models remain across runtime updates

- **GIVEN** Portable supplies its installation-local model cache
- **WHEN** a runtime update is activated and separator configuration is resolved
- **THEN** the same persistent model directory SHALL be used independently of the selected version or developer user-cache defaults

### Requirement: One complete checkout and native data layout
The accepted installation SHALL contain one complete Git checkout of the exact
selected `main` at `flitzis-looper` and one fixed private runtime. The application
working directory SHALL be that checkout, using its native `samples`,
`samples/flitzis_looper.config.json` and `config/input` paths. Application Python
and the Release extension SHALL resolve from that checkout rather than a second
application copy. Persistent `versions/main-*` runtimes SHALL NOT be retained
after a verified successful update. Installation-owned tool/build caches and
failure logs SHALL remain distinct from application data.

#### Scenario: Fresh and unchanged-main installation
- **WHEN** a fresh installation or repeat run prepares the selected current `main`
- **THEN** the complete accepted checkout, private runtime, starters and actual private-origin preflight SHALL agree on source, working directory and native data paths
- **AND** unchanged healthy source/runtime SHALL avoid another Native build, while missing or invalid model assets SHALL still be repaired and checked
- **AND** ordinary GUI starts SHALL NOT acquire a new model download or build job

#### Scenario: Recognized checkout contains unregistered files or directories
- **GIVEN** a recognized accepted checkout whose registered source files remain complete and hash-correct, with unregistered files such as an older `.venv/.gitignore` or unregistered empty directories
- **WHEN** the installer is repeated on the same or a newer exact `main`
- **THEN** it SHALL prepare and verify a clean candidate without executing the contaminated checkout, even when the selected source commit is unchanged
- **AND** after journaled data relocation and successful activation it SHALL preserve the entire previous checkout at its existing transaction backup path and report the actual retained path
- **AND** it SHALL finish the accepted transaction without copying unregistered contents into the active checkout, deleting them, or requiring otherwise healthy model/tool/package/build caches to be downloaded again
- **AND** missing or changed registered source, except the explicitly bound previous generated-Native preservation case below, unregistered or changed Runtime files, unknown installation ownership and reparse paths SHALL remain refusals that preserve existing bytes
- **AND** a later repeat on the new healthy checkout SHALL retain the unchanged-main model-repair path without another Native build

#### Scenario: Previous generated Native artifact has changed
- **GIVEN** a recognized previous or rollback-restored checkout contains changed bytes at exactly `src/flitzis_looper_audio/flitzis_looper_audio.cp314-win_amd64.pyd`, the generated untracked CPython 3.14 Windows x64 Native artifact
- **AND** that file remains present, the accepted manifest declares a valid `native_sha256` equal to its original registered file hash, and every other registered source file remains complete and hash-correct
- **WHEN** update admission or old-checkout cleanup validates that previous checkout
- **THEN** it MAY classify only those changed generated Native bytes as contents to preserve, SHALL force a clean candidate even on unchanged `main`, and SHALL preserve the entire previous checkout at its existing backup path
- **AND** it SHALL neither execute/import the changed previous Native nor copy it into the candidate; model, tool, package and build cache reuse SHALL remain available
- **AND** this preservation case SHALL NOT allow a missing Native file, missing/invalid/inconsistent Native declaration, another changed registered file, another ABI path, Runtime changes or candidate Native changes
- **AND** candidate preparation, activation and recovery SHALL retain complete strict source/Runtime validation before execution; preservation SHALL grant no source, Native, audio, timing or acknowledgement authority

#### Scenario: Candidate recovery must validate before executing
- **GIVEN** an interrupted activation or an accepted transaction awaiting cleanup
- **WHEN** recovery requires candidate relocation, private preflight or model execution
- **THEN** it SHALL first verify the exact journal-bound schema-2 source commit, clean source declaration, complete source and Runtime file inventories, hashes and reparse guards
- **AND** staged source checkouts SHALL reject unknown files and directories; Runtime SHALL reject unknown or changed files, and active source checkouts SHALL exempt only persistent Samples/Input data and their structural configuration parent
- **AND** directory ownership in this update SHALL apply to the source checkout; Runtime directory inventory and empty-directory coverage SHALL remain unchanged and SHALL NOT be claimed as verified by these checks
- **AND** unknown or tampered candidate contents SHALL stop recovery before execution and retain transaction and data evidence
- **AND** a restored recognized prior checkout MAY retain extras under the same preservation policy without executing them

### Requirement: Verified legacy data relocation
Recognized legacy installation data SHALL be relocated once under the installation
lock, preserving exact project, journal, input, sample and PCM-cache bytes and
actual material/file identities. The Native boundary SHALL capture a bounded
quiescent plan before relocation, verify recognized configuration/journal bindings
and actual file identities, and revalidate those identities after a journaled
same-volume rename. Durable configuration-location projection SHALL resolve only
the exact captured owners through one Native resolver; it SHALL NOT grant material
birth, retirement, source readiness or replayed Native ACK authority. Existing
current-configuration/revision and fresh source-ACK checks SHALL remain required.

#### Scenario: Canonical material and migration history survive legacy relocation
- **GIVEN** recognized legacy samples and configurations with complete Native material and migration evidence and no live readers or project owners
- **WHEN** the installer relocates them into the native checkout layout
- **THEN** fresh-process restore SHALL resolve their exact captured configuration owners and retained original/material/marker identities without rewriting historical project or journal bytes
- **AND** pending versus newer-current-config recovery SHALL retain its existing precedence, and each source SHALL require its own fresh callback acknowledgement
- **AND** byte-identical distinct configurations SHALL remain distinct owners

#### Scenario: Unsafe or unknown legacy state is preserved
- **WHEN** relocation encounters a target collision, cross-volume identity loss, reparse path, unknown or incomplete journal/inventory, changed capture, forged projection, live or unqueryable owner, or unavailable required configuration
- **THEN** it SHALL stop before destructive activation and preserve the existing installation, data and unknown files with a concrete reason
- **AND** copied equal bytes with different actual identities SHALL NOT satisfy relocation or material authority
- **AND** external references and unknown data SHALL remain untouched

### Requirement: Both actual separator models are prepared before activation
Installation SHALL automatically prepare and verify exactly `demucs:htdemucs`
and `bs-roformer:musdb18hq` through the existing authoritative asset resolvers.
Their persistent cache SHALL be installation-local outside the checkout, shared
by controller configuration and actual offline children. The installer SHALL
validate reused Demucs checkpoint checksums and the pinned BS-RoFormer extent and
digest, repairing absent or corrupt assets. It SHALL then load each actual model
in its own fresh private CPU subprocess, sequentially, without inference or an
audio device. Download or CPU-loader failure SHALL block activation.

#### Scenario: Repeat run repairs missing models without rebuilding Native
- **GIVEN** unchanged healthy source and runtime but missing or corrupt required separator assets
- **WHEN** the installer is run again
- **THEN** it SHALL repair authoritative assets and pass both real fresh CPU model loads while the Native-builder count remains unchanged
- **AND** the controller and offline children SHALL use the same accepted cache after activation and whole-installation relocation

### Requirement: Background Native work releases the public Engine borrow
The system SHALL perform long file, JSON, verification, preparation and persistence
work exposed through AudioEngine by capturing owned input under a short Engine borrow, releasing that
borrow before detached work, and reacquiring it only for guarded current adoption.
Concurrent public event polling and mutable control SHALL remain usable during
that work. Source, request, timing, intent, retirement/queue capacity, material
birth and own-ACK guards SHALL remain authoritative before publication. The
callback SHALL NOT acquire disk/JSON, Python/GIL, blocking locks or heavy work.

#### Scenario: Polling during real background preparation and second-process restore
- **WHEN** a real public preparation/verification worker is held at its actual I/O boundary, or a fresh Engine/controller reopens saved state
- **THEN** public `receive_msg`, mutable control and callback rendering SHALL remain usable without an Engine `Already borrowed` error
- **AND** completion SHALL adopt only its own still-current authority; changed source, intent, unload or capacity SHALL reject stale work while preserving confirmed audio

### Requirement: Cleanup respects ownership and useful caches
Successful installation SHALL remove only recorded transaction-owned completed
source/environment/wheel/license/check stages, finished downloads and retired
runtime backups after accepted-runtime and containment/reparse verification.
Samples, `.pcm-cache`, model assets, unknown files and useful tool/build caches
SHALL NOT be classified as disposable staging. Actual retained cache byte counts
and their build/download reuse purpose SHALL be reported without fixed resource
or speed promises.

#### Scenario: Verified success cleans completed owned stages
- **WHEN** the installation transaction has verified the accepted runtime and data
- **THEN** recorded contained temporary stages and backups SHALL be removed without traversing links or touching unknown data
- **AND** failed or interrupted transactions SHALL retain the evidence and recovery material needed to recover the accepted installation

#### Scenario: Known Runtime filenames contain replacement bytes
- **GIVEN** a recorded Runtime license or DLL filename whose present bytes no longer match the accepted ownership hash
- **WHEN** the installer checks an unchanged installation, prepares an update or cleans a recorded retired Runtime
- **THEN** it SHALL stop before treating the replacement bytes as disposable and preserve those bytes
- **AND** an absent recorded Runtime leaf SHALL remain eligible for Runtime repair; absence SHALL NOT authorize deletion of a present changed leaf

#### Scenario: Directory ownership is incomplete in an older checkout manifest
- **GIVEN** an older accepted manifest records files without a directory inventory
- **WHEN** update admission or retired-checkout cleanup assesses ownership
- **THEN** it SHALL derive owned directory ancestors only from recorded file paths and SHALL preserve unproved empty directories conservatively with the entire previous checkout
- **AND** current directory names or a scan of the existing tree SHALL NOT grant deletion authority
- **AND** new manifests SHALL record all checkout directories, including empty Git directories and the structural `config` parent, before persistent input data is introduced
- **AND** strict execution checks of an already active file-only legacy manifest MAY validate files without claiming unrecorded empty directories for cleanup
- **AND** retired-checkout cleanup SHALL NOT exempt Samples/Input paths from ownership checks; leftover unregistered data SHALL preserve the old checkout
- **AND** only Runtime validation MAY exempt its own unrecorded root `release-manifest.json`; Source and Input validation SHALL treat such an unregistered file as unknown
