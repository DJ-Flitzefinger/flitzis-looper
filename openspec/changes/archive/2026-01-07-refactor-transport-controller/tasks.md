## 1. Extract BPMController
- [x] 1.1 Create `_BpmController` private class in transport.py
- [x] 1.2 Extract methods from lines 292-403:
  - [x] 1.2.1 Implement `set_manual_bpm(sample_id, bpm)` - public method
  - [x] 1.2.2 Implement `clear_manual_bpm(sample_id)` - public method
  - [x] 1.2.3 Implement `tap_bpm(sample_id)` - public method
  - [x] 1.2.4 Implement `effective_bpm(sample_id)` - public method
  - [x] 1.2.5 Implement `_recompute_master_bpm()` - private helper
  - [x] 1.2.6 Implement `_on_pad_bpm_changed(sample_id)` - private helper
- [x] 1.3 Wire dependencies: ProjectState (manual_bpm), SessionState (tap_bpm_timestamps, bpm_lock_anchor_*), AudioEngine (set_pad_bpm, set_master_bpm)
- [x] 1.4 Validate with existing tests that use these methods

## 2. Extract GlobalModesController
- [x] 2.1 Create `_GlobalModesController` private class in transport.py
- [x] 2.2 Extract methods from lines 259-290:
  - [x] 2.2.1 Implement `set_multi_loop(enabled)` - public method
  - [x] 2.2.2 Implement `set_key_lock(enabled)` - public method
  - [x] 2.2.3 Implement `set_bpm_lock(enabled)` - public method
- [x] 2.3 Wire dependencies: ProjectState (multi_loop, key_lock, bpm_lock, selected_pad), SessionState (bpm_lock_anchor_*), AudioEngine (set_key_lock, set_bpm_lock)
- [x] 2.4 Integrate with BPMController for effective_bpm and _recompute_master_bpm
- [x] 2.5 Validate with existing tests that use these methods

## 3. Extract PlaybackOrchestrator
- [x] 3.1 Create `_PlaybackOrchestrator` private class in transport.py
- [x] 3.2 Extract methods from lines 63-98:
  - [x] 3.2.1 Implement `trigger_pad(sample_id)` - public method
  - [x] 3.2.2 Implement `stop_pad(sample_id)` - public method
  - [x] 3.2.3 Implement `stop_all_pads()` - public method
- [x] 3.3 Wire dependencies: ProjectState (multi_loop, sample_paths), SessionState (active_sample_ids), AudioEngine (play_sample, stop_sample, stop_all)
- [x] 3.4 Integrate with _PadLoopController for effective region and apply region
- [x] 3.5 Validate with existing tests that use these methods

## 4. Update TransportController delegation
- [x] 4.1 Instantiate all three subcontrollers in `TransportController.__init__`
- [x] 4.2 Add delegating methods for BPMController public APIs (set_manual_bpm, clear_manual_bpm, tap_bpm, effective_bpm)
- [x] 4.3 Add delegating methods for GlobalModesController public APIs (set_multi_loop, set_key_lock, set_bpm_lock)
- [x] 4.4 Add delegating methods for PlaybackOrchestrator public APIs (trigger_pad, stop_pad, stop_all_pads)
- [x] 4.5 Remove extracted methods from main TransportController class
- [x] 4.6 Verify TransportController public API surface is preserved (19 public methods)

## 5. Update _ApplyProjectState integration
- [x] 5.1 Modify `_apply_pad_bpm_settings` to call BPMController._on_pad_bpm_changed
- [x] 5.2 Modify `_apply_bpm_lock_settings` to call GlobalModesController.set_bpm_lock
- [x] 5.3 Verify state application works correctly after refactoring

## 6. Update existing subcontroller integration
- [x] 6.1 Update `_PadPlaybackController` to use `_PlaybackOrchestrator` if needed
- [x] 6.2 Update `_PadLoopController` to use `_BpmController` for effective_bpm
- [x] 6.3 Verify no circular dependencies between subcontrollers

## 7. Run full test suite
- [x] 7.1 Execute `uv run pytest` to verify behavior preservation (127 tests passed)
- [x] 7.2 Fix any test failures (added `_on_pad_bpm_changed` delegating method for facade.py)

## 8. Run linting and type checking
- [x] 8.1 Execute `uv run ruff check src` (passed, only unrelated PLR0914 in waveform_editor.py)
- [x] 8.2 Execute `uv run mypy src` (passed, no issues)
- [x] 8.3 Verify no `PLR0904` violations in controller module (passed)

## Notes
- Tasks 1-3 can be done in parallel (each extracts independent method groups)
- Task 4 depends on 1-3 completing
- Task 5 depends on 4 completing
- Task 6 depends on 4 completing
- Tasks 7-8 must be sequential and final
