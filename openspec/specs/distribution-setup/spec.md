# distribution-setup Specification

## Purpose

Define native dependency discovery and installation-owned Windows Portable
runtime preparation, verification, resumable acquisition and data-preserving updates.

## Requirements

### Requirement: Source Clones Own Required Model Resources
An ordinary source checkout SHALL default downloaded models and its required BeatThis worker to an ignored repository-root `models/` directory. Source setup SHALL provide explicit resumable pinned acquisition and fresh non-inference worker verification; startup and analysis SHALL NOT acquire dependencies. Explicit managed developer overrides and independent Portable installation bindings SHALL remain authoritative.

#### Scenario: A new clone prepares automatic BPM
- **GIVEN** installed main dependencies and a built Native extension
- **WHEN** the documented source setup command completes
- **THEN** the required worker is verified before selection under `models/beat-this/`
- **AND** optional separator setup uses `models/torch/hub/checkpoints/` unless explicitly overridden
- **AND** setup retains interrupted transfers outside temporary environment staging without an overall download deadline

### Requirement: Portable Analysis Readiness Is Installation Owned
The source-main Windows Portable installer SHALL prepare and verify the required BeatThis analysis runtime before declaring a candidate ready. Fresh installations SHALL use the existing pinned CPU profile; updates SHALL retain a valid selected CUDA profile. The worker interpreter, standard library, packages, checkpoint and relative receipts SHALL remain usable after moving the installation without a host virtual environment or developer workspace.

#### Scenario: A fresh candidate has no analysis worker
- **GIVEN** a verified current-main candidate and no installed worker
- **WHEN** the Portable installer prepares its dependencies
- **THEN** it acquires the pinned private worker resources and verifies their real non-inference environment
- **AND** the final runtime check rejects an absent or invalid worker
- **AND** candidate and final checks select the explicit installation Root, not the candidate checkout's parent

#### Scenario: Unchanged main needs worker repair
- **GIVEN** an otherwise healthy installed main whose worker is missing or damaged
- **WHEN** the same installer runs again
- **THEN** it repairs and verifies the worker without rebuilding an unchanged healthy Native runtime
- **AND** inherited personal cache/profile overrides do not redirect the private runtime

### Requirement: Portable Updates Preserve Recording and Worker Custody
Portable update transactions SHALL preserve samples, input settings, recordings and recovery data with their file identities. A candidate worker selection SHALL be journaled before activation, and failed updates SHALL restore the previous selection without deleting previous worker installations. Recovery SHALL remain compatible with older journals that have no recording or worker-selection fields.

#### Scenario: Recording data survives an update or rollback
- **GIVEN** an installed recording/recovery tree and no active installation owner
- **WHEN** an update succeeds or is rolled back
- **THEN** the appropriate active checkout retains the same recording bytes and file identities
- **AND** unknown files or conflicting selections are preserved and stop unsafe cleanup

### Requirement: Portable Payload Transfers Resume Verified Bytes
Pinned Portable payload downloads SHALL retain stable partial bytes, resume transient interruptions with interruptible backoff and impose no overall transfer deadline. Complete hashes SHALL be verified before publication; permanent unavailable or authentication failures SHALL report a concrete prerequisite while retaining evidence.

#### Scenario: A pinned download is interrupted
- **GIVEN** a partial owned payload from an interrupted transfer
- **WHEN** the installer retries
- **THEN** it resumes or verifies the retained prefix rather than restarting into another UUID download
- **AND** incorrect complete bytes are not published as a ready dependency

<!-- From replace-key-lock-with-rubberband -->
### Requirement: Cross-platform Rubber Band Dependency Discovery
The system SHALL support Rubber Band native library discovery for Windows and Linux without hardcoding developer-local paths in production code.

On Linux, the preferred development path SHALL use the system Rubber Band development package and `pkg-config` metadata when that package provides the LiveShifter C API required by the Key Lock backend. On Windows, the preferred development path SHALL use a documented vcpkg installation or explicit environment variables for include, library, and runtime DLL directories.

#### Scenario: Linux build uses system package metadata
- **GIVEN** a Linux developer has installed the Rubber Band development package and `pkg-config`
- **WHEN** the developer runs the repository build through `uv run maturin develop`
- **THEN** the build discovers Rubber Band headers and link libraries through system package metadata or documented environment overrides
- **AND** the production source does not contain workstation-specific library paths

#### Scenario: Linux package is too old
- **GIVEN** a Linux developer has installed a Rubber Band development package whose C header does not provide `rubberband_live_*` declarations
- **WHEN** the developer runs the repository build through `uv run maturin develop`
- **THEN** the build fails before runtime with a diagnostic that the Key Lock backend requires a Rubber Band package with LiveShifter C API support
- **AND** the app does not produce a native extension that later fails at import because of unresolved Rubber Band symbols

#### Scenario: Windows build uses documented vcpkg paths
- **GIVEN** a Windows developer has installed Rubber Band with vcpkg
- **AND** `VCPKG_ROOT` or documented override variables identify that installation
- **WHEN** the developer runs the repository build through `uv run maturin develop`
- **THEN** the build can discover Rubber Band headers and `rubberband.lib`
- **AND** the production source does not hardcode the developer's local vcpkg path


<!-- From replace-key-lock-with-rubberband -->
### Requirement: Runtime Libraries Are Available Before Audio Starts
The system SHALL make required Rubber Band runtime libraries available before the audio engine enters realtime callback rendering.

Runtime library discovery, DLL or shared-library path configuration, missing-library diagnostics, and packaging-copy decisions MUST happen outside the CPAL audio callback. The callback SHALL receive only initialized backend state and preallocated audio buffers.

#### Scenario: Windows runtime DLLs are available before startup
- **GIVEN** the Windows build links to Rubber Band dynamically
- **WHEN** the app starts or the packaged installer lays out application files
- **THEN** `rubberband-3.dll` and its native runtime dependencies are available through the process DLL search path or next to the packaged extension
- **AND** the audio callback does not search for or load DLL files

#### Scenario: Linux runtime shared library is available before startup
- **GIVEN** the Linux build links to Rubber Band dynamically
- **WHEN** the app starts
- **THEN** the Rubber Band shared library is resolvable through the system dynamic linker configuration, package manager installation, rpath, or documented environment configuration
- **AND** the audio callback does not search for or load shared libraries


<!-- From replace-key-lock-with-rubberband -->
### Requirement: Nuitka Packaging Remains Feasible
The system SHALL keep Rubber Band integration compatible with a later Nuitka-based Windows installer for non-technical users.

The repository SHALL NOT require end users of that installer to install vcpkg, CMake, Ninja, Rust, or Rubber Band development packages. The installer packaging plan SHALL bundle or install the required runtime libraries and SHALL document Rubber Band licensing obligations before binary distribution.

#### Scenario: Installer bundles runtime DLLs
- **GIVEN** a later release build is packaged with Nuitka for Windows
- **WHEN** the installer is created
- **THEN** it includes the Rubber Band runtime DLLs needed by the native extension
- **AND** installed non-technical users can start the Looper without installing build tools

#### Scenario: Development setup remains documented before installer exists
- **GIVEN** no end-user installer has been published yet
- **WHEN** a technically versed user wants to run the Looper from the repository
- **THEN** the README and development documentation describe the required Windows or Linux native dependency setup
- **AND** the setup path remains separate from realtime audio processing


