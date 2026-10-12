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
owned source/build/version, verify tool downloads and archive containment, and
pass a private-runtime import/preflight without
starting the GUI/audio device before atomically selecting it. Existing runtime
files and persistent user data/models/settings MUST remain intact on failure
and after successful update. Simultaneous installers SHALL be serialized.

#### Scenario: Update while an old runtime exists

- **GIVEN** an accepted prior version and saved installation-local data
- **WHEN** a newer exact `main` source runtime passes staging and preflight
- **THEN** only the current-version pointer SHALL change atomically, with the old version and all persistent data retained
- **AND** an already running old process SHALL keep its old immutable runtime files

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
