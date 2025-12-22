## ADDED Requirements

### Requirement: Flitzis Looper Application Controller Tests
The system SHALL have comprehensive unit tests for the `LooperController` class that verify all application logic, state management, and audio engine integration.

#### Scenario: Controller initialization and state management
- **WHEN** A LooperController is instantiated
- **THEN** It correctly initializes ProjectState and SessionState
- **AND** It initializes the AudioEngine and starts the audio thread
- **AND** All initial state values match expected defaults

#### Scenario: Sample loading and unloading
- **WHEN** A sample is loaded into a slot
- **THEN** The sample path is stored in ProjectState.sample_paths
- **AND** If a sample already exists in the slot, it is unloaded first
- **WHEN** A sample is unloaded
- **THEN** Any active playback is stopped
- **AND** The sample path is cleared from state
- **AND** Active sample IDs are removed from session state

#### Scenario: Playback control with MultiLoop modes
- **WHEN** MultiLoop is disabled and a pad is triggered
- **THEN** All other active pads are stopped
- **AND** Only the triggered pad plays
- **WHEN** MultiLoop is enabled and a pad is triggered
- **THEN** Only the target pad is stopped and retriggered
- **AND** All other active pads continue playing

#### Scenario: Audio parameter validation and clamping
- **WHEN** Volume is set to an out-of-range value
- **THEN** It is clamped to the valid range [0.0, 1.0]
- **AND** The clamped value is stored in state and sent to audio engine
- **WHEN** Speed is set to an out-of-range value
- **THEN** It is clamped to the valid range [SPEED_MIN, SPEED_MAX]
- **AND** The clamped value is stored in state and sent to audio engine

### Requirement: Pydantic State Model Tests
The system SHALL have comprehensive tests for `ProjectState` and `SessionState` Pydantic models that verify validation rules, type safety, and serialization behavior.

#### Scenario: ProjectState validation and defaults
- **WHEN** A ProjectState is created with no arguments
- **THEN** All fields have expected default values
- **AND** sample_paths initializes with NUM_SAMPLES None values
- **AND** multi_loop, key_lock, bpm_lock default to False
- **AND** volume defaults to 1.0 and speed defaults to 1.0
- **WHEN** An invalid value is assigned to any field
- **THEN** Pydantic validation raises an appropriate error

#### Scenario: SessionState validation and collection management
- **WHEN** A SessionState is created with no arguments
- **THEN** active_sample_ids is an empty set
- **AND** pressed_pads is a list of NUM_SAMPLES False values
- **AND** file_dialog_pad_id defaults to None
- **WHEN** An invalid sample ID is added to active_sample_ids
- **THEN** The validation function raises ValueError

### Requirement: UI Context and Read-Only State Tests
The system SHALL have comprehensive tests for `UiContext` and its subcomponents to ensure proper state access patterns and read-only enforcement.

#### Scenario: UiState computed properties and read access
- **WHEN** A pad has an audio file loaded
- **THEN** UiState.pad_label() returns the filename for Unix paths
- **AND** UiState.pad_label() correctly parses Windows paths with backslashes
- **WHEN** A pad has no sample loaded
- **THEN** UiState.pad_label() returns an empty string
- **WHEN** The UI attempts to modify state through UiState
- **THEN** ReadOnlyStateProxy raises AttributeError

#### Scenario: AudioActions delegation to controller
- **WHEN** Any AudioActions method is called
- **THEN** It delegates to the corresponding LooperController method
- **AND** Increase and decrease speed methods modify speed by SPEED_STEP
- **AND** Toggle methods flip boolean flags in ProjectState

#### Scenario: UiActions state mutations
- **WHEN** UI action methods are called
- **THEN** They correctly update UI-related state in ProjectState and SessionState
- **AND** File dialog operations update file_dialog_pad_id
- **AND** Sidebar toggles flip sidebar expanded flags
- **AND** Pad and bank selection updates selected IDs

### Requirement: ReadOnlyStateProxy Access Control Tests
The system SHALL have tests that verify the ReadOnlyStateProxy prevents accidental state mutation while allowing read access.

#### Scenario: ReadOnlyStateProxy read access
- **WHEN** An attribute is read from the proxy
- **THEN** It returns the same value as the underlying model
- **AND** All public attributes of the model are accessible

#### Scenario: ReadOnlyStateProxy write protection
- **WHEN** An attempt is made to set an attribute on the proxy
- **THEN** AttributeError is raised with a helpful message
- **AND** The underlying model is not modified
- **EXCEPT** When setting the internal _model attribute during initialization

### Requirement: Test Infrastructure and Fixtures
The system SHALL provide pytest fixtures that enable isolated, repeatable tests without requiring real audio hardware.

#### Scenario: Test fixture availability
- **WHEN** Tests import from conftest.py
- **THEN** They have access to controller, project_state, session_state, and ui_context fixtures
- **AND** The controller fixture provides a LooperController with mocked AudioEngine
- **AND** No test requires actual audio hardware to run
- **AND** All fixtures can be used independently or in combination

#### Scenario: Mock AudioEngine isolation
- **WHEN** Tests use the controller fixture
- **THEN** The AudioEngine is mocked with autospec=True
- **AND** Audio calls can be asserted without triggering real audio
- **AND** Tests remain fast and deterministic

## MODIFIED Requirements

### Requirement: Python UI Entrypoint
The system SHALL provide a Python UI entrypoint that constructs the application with proper error handling and validation.

#### Scenario: Start UI via module entrypoint
- **WHEN** The application is started via `python -m flitzis_looper`
- **THEN** It constructs a LooperController instance
- **AND** It passes UiContext to the UI rendering layer
- **AND** **All controller and state components have been tested during startup validation**

## REMOVED Requirements

### Requirement: Legacy FlitzisLooperApp Tests
**Reason**: The `FlitzisLooperApp` class has been removed and replaced by `LooperController`, `ProjectState`, `SessionState`, and `UiContext`.

**Migration**: All tests previously targeting `FlitzisLooperApp` must be rewritten to target the new architectural components:
- App state management → Test `LooperController` methods and Pydantic models
- Pad label generation → Test `UiState.pad_label()` method
- MultiLoop behavior → Test `LooperController.trigger_pad()` with mode toggles
- Speed and volume control → Test `LooperController.set_speed()` and `set_volume()`