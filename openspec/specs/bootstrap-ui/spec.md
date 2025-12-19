# bootstrap-ui Specification

## Purpose
TBD - created by archiving change add-dearpygui-ui-shell. Update Purpose after archive.
## Requirements
### Requirement: Python UI Entrypoint
The system SHALL provide a Python entrypoint to start the UI from the `flitzis_looper` package.

#### Scenario: Start UI via module entrypoint
- **WHEN** `python -m flitzis_looper` is executed
- **THEN** the UI starts without raising an exception

### Requirement: Fixed-Size Application Window
The system SHALL create a Dear PyGui application window with a fixed size of 960x630 pixels and prevent user resizing.

#### Scenario: Window starts at fixed size
- **WHEN** the UI is started
- **THEN** the viewport is created with width 960 and height 630
- **AND** the viewport is not user-resizable

### Requirement: Full-Size Primary Content Panel
The system SHALL render a single primary content window/panel that fills the viewport.

#### Scenario: Initial layout
- **WHEN** the UI is started
- **THEN** a primary content window is present
- **AND** the primary content window fills the viewport

### Requirement: Hello World Label
The system SHALL display a "hello world" label in the primary content window.

#### Scenario: Initial UI content
- **WHEN** the UI is started
- **THEN** the text "hello world" is visible

### Requirement: Application Logic Instantiation
The system SHALL instantiate an application logic object during UI startup, and the application logic SHALL instantiate `flitzis_looper_rs.AudioEngine`.

#### Scenario: Startup constructs application and audio engine
- **WHEN** the UI is started
- **THEN** an application object is created
- **AND** the application contains an `AudioEngine` instance
- **AND** audio playback is not started as part of this change

