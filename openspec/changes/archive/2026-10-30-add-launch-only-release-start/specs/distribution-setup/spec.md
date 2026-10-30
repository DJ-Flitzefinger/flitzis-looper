## ADDED Requirements

### Requirement: Launch an existing Release without building
The system SHALL provide `start.bat` as the official Windows source-app starter,
using the existing project Python environment and an already installed Release
native module without dependency synchronization, installation or compilation.

#### Scenario: Bound the preflight to the installed Release
- **WHEN** the official launch-only starter is invoked
- **THEN** the starter MUST run from its own repository directory and verify the actual
  loaded native build profile before the application entrypoint. Missing or older
  native modules, import failures, missing environments and Debug builds SHALL stop
  startup with instructions to use `start-release.bat --build-only`.

#### Scenario: Existing Release starts directly
- **GIVEN** the project environment contains a native module reporting Release
- **WHEN** the user invokes `start.bat` from any working directory
- **THEN** the starter checks the loaded profile and starts the app from the repository
- **AND** it does not invoke dependency or build tools

#### Scenario: A Debug or older module is installed
- **GIVEN** the installed native module reports Debug or lacks the profile getter
- **WHEN** the user invokes `start.bat`
- **THEN** startup fails before the application entrypoint
- **AND** the diagnostic identifies `start-release.bat --build-only` as the remedy
- **AND** no automatic setup, build or profile switch occurs

### Requirement: Separate build starters and device-free checks
The system SHALL preserve the Debug and Release build-and-start BAT commands and
provide `start.bat --check` and build-starter `--build-only` modes that stop before
the application entrypoint or any audio engine/device initialization.

#### Scenario: Preserve the checked build and failure contracts
- **WHEN** a CLI check or `--build-only` mode runs
- **THEN** the check mode MUST execute the same existing-Release preflight as normal startup.
  CLI build-only modes MUST perform locked dependency setup, the selected native build
  and actual installed-profile verification. Failures SHALL preserve nonzero exit
  codes; these CLI modes SHALL report failures without an interactive pause.

#### Scenario: Check the installed Release without starting the app
- **GIVEN** the existing project environment and native module are usable
- **WHEN** the user invokes `start.bat --check`
- **THEN** the actual native profile is checked without synchronization or compilation
- **AND** no application entrypoint or audio device is opened

#### Scenario: Build-only failure prevents application startup
- **GIVEN** dependency setup, compilation or installed-profile verification fails
- **WHEN** the user invokes either build starter with `--build-only`
- **THEN** the failed step's nonzero exit code is returned with a visible diagnostic
- **AND** no application entrypoint is run and no interactive pause occurs

#### Scenario: Existing build-and-start behavior remains available
- **GIVEN** the selected Debug or Release setup and build succeed
- **WHEN** the user invokes its build starter without an option
- **THEN** the selected installed native profile is verified
- **AND** the app starts from the repository using the existing project Python environment

### Requirement: Build Release by double-click with a visible result
The system SHALL provide `build-release.bat` as a no-argument Windows Release
build-only entrypoint using the existing shared locked dependency setup, native
Release build and actual installed-profile verification pipeline. It SHALL
report success or the failed step, wait for a key on both success and failure,
and return the actual exit code after waiting without entering the application
or initializing an audio engine or device.

#### Scenario: Double-click a successful Release build
- **GIVEN** the Release build toolchain is usable
- **WHEN** the user invokes `build-release.bat` without arguments, including from
  a different working directory or a repository path containing spaces
- **THEN** the entrypoint runs locked dependency setup and the native Release
  build from its own repository directory
- **AND** it verifies that the actual installed native module reports Release
- **AND** it shows successful build completion and waits for a key before returning exit code zero
- **AND** no shortcut arguments are required and no application entrypoint is run

#### Scenario: Dependency setup or native build fails
- **GIVEN** dependency setup or the Release build returns a nonzero exit code
- **WHEN** the user invokes `build-release.bat` without arguments
- **THEN** subsequent build/profile/application steps do not run
- **AND** the diagnostic identifies the failed step and its exit code and waits for a key
- **AND** the entrypoint returns exactly the failed step's exit code after waiting

#### Scenario: The installed profile check fails after building
- **GIVEN** setup and the Release build complete but the installed-profile check
  fails, including an unusable native module or a module reporting Debug
- **WHEN** the user invokes `build-release.bat` without arguments
- **THEN** the entrypoint shows the profile-check failure and waits for a key
- **AND** it preserves the profile-check failure code after waiting and does not start the app

#### Scenario: Reject options before work
- **WHEN** the user supplies an option or extra argument to `build-release.bat`,
  including `--build-only` or `--check`
- **THEN** the entrypoint reports usage and argument-validation failure before
  directory setup, dependency tools or Python execution
- **AND** it waits for a key and returns exit code two without starting the app
