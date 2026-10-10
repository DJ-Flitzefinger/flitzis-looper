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
- **WHEN** a check or build-only mode runs
- **THEN** the check mode MUST execute the same existing-Release preflight as normal startup.
  Build-only modes MUST perform locked dependency setup, the selected native build
  and actual installed-profile verification. Failures SHALL preserve nonzero exit
  codes; check/build-only modes SHALL report failures without an interactive pause.

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
