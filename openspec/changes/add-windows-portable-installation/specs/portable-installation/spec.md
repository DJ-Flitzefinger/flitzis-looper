## ADDED Requirements

### Requirement: Independent Windows portable installation

The system SHALL provide `INSTALL-LOOPER-PORTABLE.bat` as a standalone Windows
x64 installer that creates a child installation without requiring Git, Python,
Rust, administrator access or developer commands on the target computer.
The installation MUST contain a compatible private Python runtime, Release
native extension, application, non-development dependencies, required DLLs,
FFmpeg/FFprobe, keynet and licenses. It SHALL include simple portable starters.
Installing or starting Portable MUST NOT modify the developer runtime/settings.
All package preparation and installation SHALL remain outside realtime audio.

#### Scenario: Personal standalone installer from another folder

- **GIVEN** a verified local bundle/channel and a generated personal BAT naming that channel
- **WHEN** the Human copies only that BAT to another folder on the same computer and runs it
- **THEN** it SHALL install an independent child directory containing its complete private runtime and portable starters without copying developer projects/settings/private models
- **AND** the starter SHALL use persistent installation-local data, models and caches, with no dependency on the developer checkout or environment

#### Scenario: Published download channel

- **GIVEN** an authorized published HTTPS channel naming an available immutable bundle with size and SHA256
- **WHEN** the generic BAT runs on a compatible Windows x64 computer
- **THEN** it SHALL fetch and verify that bundle before installing it
- **AND** an absent release or failed download SHALL report the failure without inventing a successful installation

### Requirement: Safe repeatable portable updates

Repeated installation SHALL check the configured channel and avoid downloading
an unchanged bundle. An update MUST prepare a fresh owned version, verify archive
containment and contents, and pass a private-runtime import/preflight without
starting the GUI/audio device before atomically selecting it. Existing runtime
files and persistent user data/models/settings MUST remain intact on failure
and after successful update. Simultaneous installers SHALL be serialized.

#### Scenario: Update while an old runtime exists

- **GIVEN** an accepted prior version and saved installation-local data
- **WHEN** a newer channel bundle passes staging and preflight
- **THEN** only the current-version pointer SHALL change atomically, with the old version and all persistent data retained
- **AND** an already running old process SHALL keep its old immutable runtime files

#### Scenario: Corrupt or escaping bundle

- **GIVEN** an accepted prior version and a new payload with a wrong hash, unsafe archive path, symlink or failed preflight
- **WHEN** installation is attempted
- **THEN** the update SHALL fail without changing the current pointer, existing runtime or user data
- **AND** unowned/nonempty target directories or reparse paths SHALL NOT be overwritten

### Requirement: Explicit portable model cache

Portable launch SHALL resolve its separator model cache inside its persistent
installation directory. Ordinary developer launches SHALL retain the existing
standard user cache when no explicit Portable override is supplied.

#### Scenario: Models remain across runtime updates

- **GIVEN** Portable supplies its installation-local model cache
- **WHEN** a runtime update is activated and separator configuration is resolved
- **THEN** the same persistent model directory SHALL be used independently of the selected version or developer user-cache defaults
