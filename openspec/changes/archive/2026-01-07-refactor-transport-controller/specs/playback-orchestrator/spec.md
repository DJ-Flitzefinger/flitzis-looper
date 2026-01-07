# playback-orchestrator Specification

## Purpose
Extract playback orchestration from TransportController into a focused PlaybackOrchestrator subcomponent that manages pad triggering, stopping, and multi-loop semantics.

## ADDED Requirements

### Requirement: PlaybackOrchestrator manages pad triggering
PlaybackOrchestrator SHALL provide methods to trigger pad playback with proper multi-loop semantics and loop region application.

#### Scenario: Trigger pad with multi-loop disabled
- **GIVEN** multi-loop mode is disabled
- **AND** pad 1 is active
- **AND** pad 2 has loaded audio with loop region [0.0, 4.0]
- **WHEN** `PlaybackOrchestrator.trigger_pad(2)` is called
- **THEN** pad 1 is stopped via `AudioEngine.stop_sample(1)`
- **AND** `AudioEngine.play_sample(2, 1.0)` is called
- **AND** `SessionState.active_sample_ids` contains only {2}
- **AND** `AudioEngine.set_pad_loop_region(2, 0.0, 4.0)` is called

#### Scenario: Trigger pad with multi-loop enabled
- **GIVEN** multi-loop mode is enabled
- **AND** pad 1 is active
- **AND** pad 2 has loaded audio with loop region [0.0, 4.0]
- **WHEN** `PlaybackOrchestrator.trigger_pad(2)` is called
- **THEN** pad 1 is stopped via `AudioEngine.stop_sample(2)` (stop self before retrigger)
- **AND** `AudioEngine.play_sample(2, 1.0)` is called
- **AND** `SessionState.active_sample_ids` contains {1, 2}
- **AND** `AudioEngine.set_pad_loop_region(2, 0.0, 4.0)` is called

#### Scenario: Trigger pad with no loaded audio is no-op
- **GIVEN** pad 2 has no loaded audio (sample_paths[2] is None)
- **WHEN** `PlaybackOrchestrator.trigger_pad(2)` is called
- **THEN** the call returns immediately without state changes
- **AND** no audio engine methods are called

### Requirement: PlaybackOrchestrator manages pad stopping
PlaybackOrchestrator SHALL provide methods to stop individual pad playback.

#### Scenario: Stop active pad
- **GIVEN** pad 1 is active (in SessionState.active_sample_ids)
- **WHEN** `PlaybackOrchestrator.stop_pad(1)` is called
- **THEN** `AudioEngine.stop_sample(1)` is called
- **AND** `SessionState.active_sample_ids` does not contain 1

#### Scenario: Stop inactive pad is no-op
- **GIVEN** pad 1 is not active (not in SessionState.active_sample_ids)
- **WHEN** `PlaybackOrchestrator.stop_pad(1)` is called
- **THEN** the call returns immediately without state changes
- **AND** no audio engine methods are called

### Requirement: PlaybackOrchestrator manages stopping all pads
PlaybackOrchestrator SHALL provide a method to stop all currently active pads.

#### Scenario: Stop all pads when multiple are active
- **GIVEN** pads {1, 2, 3} are active
- **WHEN** `PlaybackOrchestrator.stop_all_pads()` is called
- **THEN** `AudioEngine.stop_all()` is called
- **AND** `SessionState.active_sample_ids` is cleared (empty set)

#### Scenario: Stop all pads when none are active
- **GIVEN** no pads are active
- **WHEN** `PlaybackOrchestrator.stop_all_pads()` is called
- **THEN** `AudioEngine.stop_all()` is called
- **AND** `SessionState.active_sample_ids` remains empty

### Requirement: PlaybackOrchestrator uses LoopController for loop regions
PlaybackOrchestrator SHALL depend on LoopController to get effective loop regions before triggering playback.

#### Scenario: Trigger pad uses effective loop region
- **GIVEN** LoopController provides effective_region method
- **AND** pad 2 has loop region [0.5, 4.5] with auto-loop enabled
- **WHEN** `PlaybackOrchestrator.trigger_pad(2)` is called
- **THEN** `LoopController.effective_region(2)` is called
- **AND** the returned region is applied via `AudioEngine.set_pad_loop_region()`
- **AND** the region accounts for quantization and beat snapping if applicable

### Requirement: PlaybackOrchestrator respects multi-loop semantics
PlaybackOrchestrator SHALL implement multi-loop mode semantics as defined in the multi-loop-mode spec.

#### Scenario: Multi-loop disabled stops all other pads before trigger
- **GIVEN** multi-loop mode is disabled (from ProjectState.multi_loop)
- **AND** pads {1, 2, 3} are active
- **WHEN** `PlaybackOrchestrator.trigger_pad(4)` is called
- **THEN** `PlaybackOrchestrator.stop_all_pads()` is called implicitly
- **AND** all pads {1, 2, 3} are stopped
- **AND** pad 4 becomes the only active pad

#### Scenario: Multi-loop enabled stops only triggered pad before retrigger
- **GIVEN** multi-loop mode is enabled (from ProjectState.multi_loop)
- **AND** pads {1, 2, 3} are active
- **AND** pad 2 is being retriggered
- **WHEN** `PlaybackOrchestrator.trigger_pad(2)` is called
- **THEN** only pad 2 is stopped via `PlaybackOrchestrator.stop_pad(2)`
- **AND** pads {1, 3} remain active
- **AND** pad 2 is triggered again

### Requirement: PlaybackOrchestrator integrates with TransportController
PlaybackOrchestrator SHALL be accessible through TransportController delegation, preserving the existing public API surface.

#### Scenario: TransportController delegates trigger_pad
- **GIVEN** a TransportController instance
- **WHEN** `TransportController.trigger_pad(2)` is called
- **THEN** the call is delegated to `TransportController._playback_orchestrator.trigger_pad(2)`

#### Scenario: TransportController delegates stop_pad
- **GIVEN** a TransportController instance
- **WHEN** `TransportController.stop_pad(1)` is called
- **THEN** the call is delegated to `TransportController._playback_orchestrator.stop_pad(1)`

#### Scenario: TransportController delegates stop_all_pads
- **GIVEN** a TransportController instance
- **WHEN** `TransportController.stop_all_pads()` is called
- **THEN** the call is delegated to `TransportController._playback_orchestrator.stop_all_pads()`

## CROSS-REFERENCE Requirements

This spec relates to the following capabilities:
- `multi-loop-mode`: Multi-loop mode state management and behavior
- `play-samples`: Trigger, stop, and stop-all playback APIs
