# Refactor TransportController Design

## Architectural Overview

The current `TransportController` class has 593 lines and ~40 public methods, violating Ruff's `PLR0904` public-method threshold (20 methods). This refactor extracts three focused subcontrollers:

1. **BPMController** - Manages BPM overrides, tap detection, master BPM computation
2. **GlobalModesController** - Manages global playback modes (multi-loop, key lock, BPM lock)
3. **PlaybackOrchestrator** - Manages pad playback triggering and stopping

## Subcontroller Responsibilities

### BPMController
**Purpose**: Centralize all BPM-related logic including manual overrides, tap detection, and master BPM synchronization.

**Public Methods**:
- `set_manual_bpm(sample_id, bpm)` - Set manual BPM override
- `clear_manual_bpm(sample_id)` - Clear manual BPM override
- `tap_bpm(sample_id)` - Register tap event, compute and return BPM
- `effective_bpm(sample_id)` - Get effective BPM (manual or detected)

**Private Methods**:
- `_recompute_master_bpm()` - Compute master BPM when BPM lock is active
- `_on_pad_bpm_changed(sample_id)` - Handle BPM change side effects

**Dependencies**:
- ProjectState.manual_bpm (read/write)
- ProjectState.sample_analysis (read-only)
- SessionState.tap_bpm_timestamps (read/write)
- SessionState.bpm_lock_anchor_* (read/write)
- AudioEngine.set_pad_bpm(), set_master_bpm()
- TransportController parent (for delegation)

**State Flow**:
- User sets manual BPM → manual_bpm[sample_id] updated → _on_pad_bpm_changed() → audio updated
- User taps BPM → timestamps tracked → BPM computed → manual_bpm[sample_id] updated → audio updated
- BPM lock anchor changes → anchor_pad_id/bpm updated → _recompute_master_bpm() → master_bpm updated → audio updated

### GlobalModesController
**Purpose**: Manage global playback modes that affect all pads.

**Public Methods**:
- `set_multi_loop(enabled)` - Enable/disable multi-loop mode
- `set_key_lock(enabled)` - Enable/disable key lock mode
- `set_bpm_lock(enabled)` - Enable/disable BPM lock mode

**Dependencies**:
- ProjectState.multi_loop, key_lock, bpm_lock, selected_pad (read/write)
- SessionState.bpm_lock_anchor_* (read/write)
- AudioEngine.set_key_lock(), set_bpm_lock()
- BPMController.effective_bpm() (read-only)
- BPMController._recompute_master_bpm() (invoke)

**State Flow**:
- User toggles multi-loop → ProjectState updated → no audio sync needed
- User toggles key lock → ProjectState updated → audio updated
- User toggles BPM lock → ProjectState updated → anchor set → master BPM recomputed → audio updated

### PlaybackOrchestrator
**Purpose**: Coordinate pad playback triggering and stopping with multi-loop semantics.

**Public Methods**:
- `trigger_pad(sample_id)` - Trigger or retrigger pad playback
- `stop_pad(sample_id)` - Stop specific pad
- `stop_all_pads()` - Stop all pads

**Dependencies**:
- ProjectState.multi_loop, sample_paths (read-only)
- SessionState.active_sample_ids (read/write)
- AudioEngine.play_sample(), stop_sample(), stop_all()
- _PadLoopController.effective_region() (read-only)
- TransportController parent (for delegation)

**State Flow**:
- User triggers pad → check multi_loop → stop others if needed → get loop region → audio.play_sample() → active_sample_ids updated
- User stops pad → audio.stop_sample() → active_sample_ids updated
- User stops all → audio.stop_all() → active_sample_ids cleared

## Integration Points

### TransportController Delegation
All public methods from subcontrollers will be exposed on `TransportController` via delegation:
```python
def set_manual_bpm(self, sample_id: int, bpm: float) -> None:
    self._bpm_controller.set_manual_bpm(sample_id, bpm)
```

This preserves the existing public API surface while moving implementation to focused classes.

### _ApplyProjectState Integration
The state applier will use subcontrollers to apply initial state:
```python
def _apply_pad_bpm_settings(self) -> None:
    for sample_id in range(len(self._project.sample_paths)):
        if self._project.manual_bpm[sample_id] or self._project.sample_analysis[sample_id]:
            self._bpm_controller._on_pad_bpm_changed(sample_id)

def _apply_bpm_lock_settings(self) -> None:
    self._global_modes.set_bpm_lock(enabled=self._project.bpm_lock)
```

### Existing Subcontroller Integration
- `_PadPlaybackController` already exists and can stay as-is (provides different semantics)
- `_PadLoopController` uses `effective_bpm()` which will move to BPMController, requiring cross-controller call

## Dependency Graph

```
TransportController (entry point)
├── _BpmController
│   └── AudioEngine (set_pad_bpm, set_master_bpm)
├── _GlobalModesController
│   ├── _BpmController (effective_bpm, _recompute_master_bpm)
│   └── AudioEngine (set_key_lock, set_bpm_lock)
├── _PlaybackOrchestrator
│   ├── _PadLoopController (effective_region)
│   └── AudioEngine (play_sample, stop_sample, stop_all)
├── _PadPlaybackController (existing)
└── _PadLoopController (existing)
```

Note: `_GlobalModesController` depends on `_BpmController` for BPM lock functionality, creating a slight coupling. This is acceptable as they are both internal implementation details of `TransportController`.

## Trade-offs

### Benefits
- Reduced complexity in main class
- Improved testability (subcontrollers can be unit tested in isolation)
- Better organization (related functionality grouped)
- Easier to understand each responsibility area

### Costs
- More files/classes to navigate
- Delegation overhead (minimal, just method forwarding)
- Cross-subcontroller dependencies (BPMController ↔ GlobalModesController)

## Alternatives Considered

### Option 1: Create separate top-level controllers
**Rejected**: Would require changing all call sites and import paths, breaking UI integration.

### Option 2: Mixin classes
**Rejected**: Mixins would create implicit method resolution and harder-to-follow control flow.

### Option 3: Keep all methods in main class
**Rejected**: Violates Ruff `PLR0904` and reduces maintainability.

## Testing Strategy

Each subcontroller should be testable via:
- Unit tests on individual methods (e.g., tap_bpm calculation)
- Integration tests with mocked AudioEngine
- Full system tests through TransportController public API

The refactoring should preserve all existing test behavior; no new tests are required beyond what validates the refactoring preserves behavior.
