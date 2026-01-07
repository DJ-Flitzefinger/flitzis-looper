# global-modes-controller Specification

## Purpose
Extract global mode management from TransportController into a focused GlobalModesController subcomponent that manages multi-loop, key lock, and BPM lock modes.

## ADDED Requirements

### Requirement: GlobalModesController manages multi-loop mode
GlobalModesController SHALL provide methods to enable and disable multi-loop mode, which controls whether pads can loop concurrently.

#### Scenario: Enable multi-loop mode
- **GIVEN** multi-loop mode is currently disabled
- **WHEN** `GlobalModesController.set_multi_loop(enabled=True)` is called
- **THEN** `ProjectState.multi_loop` is set to True
- **AND** project changed flag is marked

#### Scenario: Disable multi-loop mode
- **GIVEN** multi-loop mode is currently enabled
- **WHEN** `GlobalModesController.set_multi_loop(enabled=False)` is called
- **THEN** `ProjectState.multi_loop` is set to False
- **AND** project changed flag is marked

### Requirement: GlobalModesController manages key lock mode
GlobalModesController SHALL provide methods to enable and disable key lock mode, which affects audio engine processing.

#### Scenario: Enable key lock mode
- **GIVEN** key lock mode is currently disabled
- **WHEN** `GlobalModesController.set_key_lock(enabled=True)` is called
- **THEN** `ProjectState.key_lock` is set to True
- **AND** `AudioEngine.set_key_lock(enabled=True)` is called
- **AND** project changed flag is marked

#### Scenario: Disable key lock mode
- **GIVEN** key lock mode is currently enabled
- **WHEN** `GlobalModesController.set_key_lock(enabled=False)` is called
- **THEN** `ProjectState.key_lock` is set to False
- **AND** `AudioEngine.set_key_lock(enabled=False)` is called
- **AND** project changed flag is marked

#### Scenario: Setting same key lock state is no-op
- **GIVEN** key lock mode is already enabled
- **WHEN** `GlobalModesController.set_key_lock(enabled=True)` is called
- **THEN** the call returns immediately without state changes
- **AND** project changed flag is not marked

### Requirement: GlobalModesController manages BPM lock mode
GlobalModesController SHALL provide methods to enable and disable BPM lock mode, which synchronizes pad tempos to a master BPM.

#### Scenario: Enable BPM lock mode
- **GIVEN** BPM lock mode is currently disabled
- **AND** selected pad is 0 with effective BPM 120.0
- **WHEN** `GlobalModesController.set_bpm_lock(enabled=True)` is called
- **THEN** `ProjectState.bpm_lock` is set to True
- **AND** `SessionState.bpm_lock_anchor_pad_id` is set to 0
- **AND** `SessionState.bpm_lock_anchor_bpm` is set to 120.0
- **AND** `AudioEngine.set_bpm_lock(enabled=True)` is called
- **AND** master BPM is recomputed via BPMController
- **AND** project changed flag is marked

#### Scenario: Disable BPM lock mode
- **GIVEN** BPM lock mode is currently enabled
- **AND** anchor pad ID is 0 and anchor BPM is 120.0
- **WHEN** `GlobalModesController.set_bpm_lock(enabled=False)` is called
- **THEN** `ProjectState.bpm_lock` is set to False
- **AND** `SessionState.bpm_lock_anchor_pad_id` is set to None
- **AND** `SessionState.bpm_lock_anchor_bpm` is set to None
- **AND** `AudioEngine.set_bpm_lock(enabled=False)` is called
- **AND** master BPM is recomputed via BPMController
- **AND** project changed flag is marked

#### Scenario: Setting same BPM lock state is no-op
- **GIVEN** BPM lock mode is already enabled
- **WHEN** `GlobalModesController.set_bpm_lock(enabled=True)` is called
- **THEN** the call returns immediately without state changes
- **AND** project changed flag is not marked

### Requirement: GlobalModesController integrates with BPMController
GlobalModesController SHALL depend on BPMController for effective BPM computation and master BPM recombination when managing BPM lock mode.

#### Scenario: BPM lock uses BPMController effective_bpm
- **GIVEN** BPMController is available
- **WHEN** `GlobalModesController.set_bpm_lock(enabled=True)` is called
- **THEN** BPMController.effective_bpm(selected_pad) is called to get anchor BPM
- **AND** the returned BPM value is used for anchor state

#### Scenario: BPM lock uses BPMController _recompute_master_bpm
- **GIVEN** BPMController is available
- **WHEN** `GlobalModesController.set_bpm_lock(enabled=True)` is called
- **THEN** BPMController._recompute_master_bpm() is called after anchor is set
- **AND** master BPM is updated based on anchor and speed

### Requirement: GlobalModesController integrates with TransportController
GlobalModesController SHALL be accessible through TransportController delegation, preserving the existing public API surface.

#### Scenario: TransportController delegates set_multi_loop
- **GIVEN** a TransportController instance
- **WHEN** `TransportController.set_multi_loop(enabled=True)` is called
- **THEN** the call is delegated to `TransportController._global_modes_controller.set_multi_loop(enabled=True)`

#### Scenario: TransportController delegates set_key_lock
- **GIVEN** a TransportController instance
- **WHEN** `TransportController.set_key_lock(enabled=True)` is called
- **THEN** the call is delegated to `TransportController._global_modes_controller.set_key_lock(enabled=True)`

#### Scenario: TransportController delegates set_bpm_lock
- **GIVEN** a TransportController instance
- **WHEN** `TransportController.set_bpm_lock(enabled=True)` is called
- **THEN** the call is delegated to `TransportController._global_modes_controller.set_bpm_lock(enabled=True)`

## CROSS-REFERENCE Requirements

This spec relates to the following capabilities:
- `multi-loop-mode`: Multi-loop mode state management and behavior
- `play-samples`: Lock mode changes affect playback (requirement 26)
- `bpm-controller`: BPMController provides effective_bpm and master BPM computation
