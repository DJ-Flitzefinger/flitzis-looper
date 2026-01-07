# bpm-controller Specification

## Purpose
Extract BPM-related functionality from TransportController into a focused BPMController subcomponent that manages manual BPM overrides, tap BPM detection, and master BPM computation.

## ADDED Requirements

### Requirement: BPMController provides BPM override management
BPMController SHALL provide methods to set, clear, and retrieve manual BPM overrides for individual pads, including tap BPM detection and effective BPM computation.

#### Scenario: Set manual BPM override
- **GIVEN** a sample with ID 0
- **WHEN** `BPMController.set_manual_bpm(0, 128.0)` is called
- **THEN** `ProjectState.manual_bpm[0]` is set to 128.0
- **AND** `AudioEngine.set_pad_bpm(0, 128.0)` is called
- **AND** if BPM lock is active and pad 0 is the anchor, master BPM is recomputed

#### Scenario: Clear manual BPM override
- **GIVEN** a sample with ID 0 that has manual BPM set to 128.0
- **WHEN** `BPMController.clear_manual_bpm(0)` is called
- **THEN** `ProjectState.manual_bpm[0]` is set to None
- **AND** `AudioEngine.set_pad_bpm(0, None)` is called
- **AND** if BPM lock is active and pad 0 is the anchor, master BPM is recomputed

#### Scenario: Tap BPM detection
- **GIVEN** a sample with ID 0 and tap BPM window size of 5
- **WHEN** `BPMController.tap_bpm(0)` is called with three taps at 120 BPM intervals
- **THEN** `SessionState.tap_bpm_timestamps` contains the tap timestamps
- **AND** `ProjectState.manual_bpm[0]` is set to approximately 120.0
- **AND** the method returns the computed BPM value
- **AND** `AudioEngine.set_pad_bpm(0, 120.0)` is called

#### Scenario: Tap BPM returns None with insufficient taps
- **GIVEN** a sample with ID 0
- **WHEN** `BPMController.tap_bpm(0)` is called with only 1 or 2 taps
- **THEN** the method returns None
- **AND** `ProjectState.manual_bpm[0]` is not modified

#### Scenario: Effective BPM returns manual override when set
- **GIVEN** a sample with ID 0 with manual BPM set to 128.0
- **WHEN** `BPMController.effective_bpm(0)` is called
- **THEN** the method returns 128.0

#### Scenario: Effective BPM returns detected BPM when no manual override
- **GIVEN** a sample with ID 0 with no manual BPM and detected BPM of 120.0
- **WHEN** `BPMController.effective_bpm(0)` is called
- **THEN** the method returns 120.0

### Requirement: BPMController manages master BPM computation
BPMController SHALL provide private methods to recompute master BPM when BPM lock is active, considering anchor pad BPM and global speed multiplier.

#### Scenario: Master BPM recomputed with BPM lock active
- **GIVEN** BPM lock is enabled
- **AND** anchor pad ID is 0 with effective BPM 120.0
- **AND** global speed is 1.0
- **WHEN** `BPMController._recompute_master_bpm()` is called
- **THEN** `SessionState.master_bpm` is set to 120.0
- **AND** `AudioEngine.set_master_bpm(120.0)` is called

#### Scenario: Master BPM recomputed with speed multiplier
- **GIVEN** BPM lock is enabled
- **AND** anchor pad ID is 0 with effective BPM 120.0
- **AND** global speed is 1.25
- **WHEN** `BPMController._recompute_master_bpm()` is called
- **THEN** `SessionState.master_bpm` is set to 150.0 (120.0 * 1.25)
- **AND** `AudioEngine.set_master_bpm(150.0)` is called

#### Scenario: Master BPM cleared when BPM lock disabled
- **GIVEN** BPM lock is disabled
- **WHEN** `BPMController._recompute_master_bpm()` is called
- **THEN** `SessionState.master_bpm` is set to None
- **AND** `AudioEngine.set_master_bpm()` is called with None

### Requirement: BPMController validates input values
BPMController SHALL validate all input values according to project validation rules, raising exceptions for invalid inputs.

#### Scenario: Set manual BPM with negative value raises error
- **GIVEN** a sample with ID 0
- **WHEN** `BPMController.set_manual_bpm(0, -10.0)` is called
- **THEN** a ValueError is raised with message indicating BPM must be > 0

#### Scenario: Set manual BPM with zero raises error
- **GIVEN** a sample with ID 0
- **WHEN** `BPMController.set_manual_bpm(0, 0.0)` is called
- **THEN** a ValueError is raised with message indicating BPM must be > 0

### Requirement: BPMController integrates with TransportController
BPMController SHALL be accessible through TransportController delegation, preserving the existing public API surface.

#### Scenario: TransportController delegates to BPMController
- **GIVEN** a TransportController instance
- **WHEN** `TransportController.set_manual_bpm(0, 128.0)` is called
- **THEN** the call is delegated to `TransportController._bpm_controller.set_manual_bpm(0, 128.0)`

#### Scenario: TransportController delegates effective_bpm
- **GIVEN** a TransportController instance
- **WHEN** `TransportController.effective_bpm(0)` is called
- **THEN** the call is delegated to `TransportController._bpm_controller.effective_bpm(0)`

#### Scenario: TransportController delegates tap_bpm
- **GIVEN** a TransportController instance
- **WHEN** `TransportController.tap_bpm(0)` is called
- **THEN** the call is delegated to `TransportController._bpm_controller.tap_bpm(0)`

## MODIFIED Requirements

### Requirement: TransportController passes Ruff public-method limits
The Python control layer SHALL provide `TransportController` as the entry point while decomposing controller responsibilities into smaller focused modules/classes so that no single class violates Ruff `PLR0904` public-method thresholds.

This requirement is modified from the original looper-controller spec to apply specifically to TransportController.

#### Scenario: TransportController passes Ruff public-method limits
- **WHEN** the developer runs Ruff on the project
- **THEN** `TransportController` class produces no `PLR0904` findings
