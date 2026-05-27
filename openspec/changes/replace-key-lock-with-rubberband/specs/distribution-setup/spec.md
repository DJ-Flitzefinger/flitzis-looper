## ADDED Requirements

### Requirement: Cross-platform Rubber Band Dependency Discovery
The system SHALL support Rubber Band native library discovery for Windows and Linux without hardcoding developer-local paths in production code.

On Linux, the preferred development path SHALL use the system Rubber Band development package and `pkg-config` metadata when available. On Windows, the preferred development path SHALL use a documented vcpkg installation or explicit environment variables for include, library, and runtime DLL directories.

#### Scenario: Linux build uses system package metadata
- **GIVEN** a Linux developer has installed the Rubber Band development package and `pkg-config`
- **WHEN** the developer runs the repository build through `uv run maturin develop`
- **THEN** the build discovers Rubber Band headers and link libraries through system package metadata or documented environment overrides
- **AND** the production source does not contain workstation-specific library paths

#### Scenario: Windows build uses documented vcpkg paths
- **GIVEN** a Windows developer has installed Rubber Band with vcpkg
- **AND** `VCPKG_ROOT` or documented override variables identify that installation
- **WHEN** the developer runs the repository build through `uv run maturin develop`
- **THEN** the build can discover Rubber Band headers and `rubberband.lib`
- **AND** the production source does not hardcode the developer's local vcpkg path

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
