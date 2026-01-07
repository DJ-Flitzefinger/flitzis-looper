## 1. Extract BPMController
- [ ] 1.1 Create `_BpmController` private class in transport.py
- [ ] 1.2 Extract methods from lines 292-403:
  - [ ] 1.2.1 Implement `set_manual_bpm(sample_id, bpm)` - public method
  - [ ] 1.2.2 Implement `clear_manual_bpm(sample_id)` - public method
  - [ ] 1.2.3 Implement `tap_bpm(sample_id)` - public method
  - [ ] 1.2.4 Implement `effective_bpm(sample_id)` - public method
  - [ ] 1.2.5 Implement `_recompute_master_bpm()` - private helper
  - [ ] 1.2.6 Implement `_on_pad_bpm_changed(sample_id)` - private helper
- [ ] 1.3 Wire dependencies: ProjectState (manual_bpm), SessionState (tap_bpm_timestamps, bpm_lock_anchor_*), AudioEngine (set_pad_bpm, set_master_bpm)
- [ ] 1.4 Validate with existing tests that use these methods

## 2. Extract GlobalModesController
- [ ] 2.1 Create `_GlobalModesController` private class in transport.py
- [ ] 2.2 Extract methods from lines 259-290:
  - [ ] 2.2.1 Implement `set_multi_loop(enabled)` - public method
  - [ ] 2.2.2 Implement `set_key_lock(enabled)` - public method
  - [ ] 2.2.3 Implement `set_bpm_lock(enabled)` - public method
- [ ] 2.3 Wire dependencies: ProjectState (multi_loop, key_lock, bpm_lock, selected_pad), SessionState (bpm_lock_anchor_*), AudioEngine (set_key_lock, set_bpm_lock)
- [ ] 2.4 Integrate with BPMController for effective_bpm and _recompute_master_bpm
- [ ] 2.5 Validate with existing tests that use these methods

## 3. Extract PlaybackOrchestrator
- [ ] 3.1 Create `_PlaybackOrchestrator` private class in transport.py
- [ ] 3.2 Extract methods from lines 63-98:
  - [ ] 3.2.1 Implement `trigger_pad(sample_id)` - public method
  - [ ] 3.2.2 Implement `stop_pad(sample_id)` - public method
  - [ ] 3.2.3 Implement `stop_all_pads()` - public method
- [ ] 3.3 Wire dependencies: ProjectState (multi_loop, sample_paths), SessionState (active_sample_ids), AudioEngine (play_sample, stop_sample, stop_all)
- [ ] 3.4 Integrate with _PadLoopController for effective region and apply region
- [ ] 3.5 Validate with existing tests that use these methods

## 4. Update TransportController delegation
- [ ] 4.1 Instantiate all three subcontrollers in `TransportController.__init__`
- [ ] 4.2 Add delegating methods for BPMController public APIs (set_manual_bpm, clear_manual_bpm, tap_bpm, effective_bpm)
- [ ] 4.3 Add delegating methods for GlobalModesController public APIs (set_multi_loop, set_key_lock, set_bpm_lock)
- [ ] 4.4 Add delegating methods for PlaybackOrchestrator public APIs (trigger_pad, stop_pad, stop_all_pads)
- [ ] 4.5 Remove extracted methods from main TransportController class
- [ ] 4.6 Verify TransportController public API surface is preserved

## 5. Update _ApplyProjectState integration
- [ ] 5.1 Modify `_apply_pad_bpm_settings` to call BPMController._on_pad_bpm_changed
- [ ] 5.2 Modify `_apply_bpm_lock_settings` to call GlobalModesController.set_bpm_lock
- [ ] 5.3 Verify state application works correctly after refactoring

## 6. Update existing subcontroller integration
- [ ] 6.1 Update `_PadPlaybackController` to use `_PlaybackOrchestrator` if needed
- [ ] 6.2 Update `_PadLoopController` to use `_BpmController` for effective_bpm
- [ ] 6.3 Verify no circular dependencies between subcontrollers

## 7. Run full test suite
- [ ] 7.1 Execute `uv run pytest` to verify behavior preservation
- [ ] 7.2 Fix any test failures (should be minimal, mostly delegation issues)

## 8. Run linting and type checking
- [ ] 8.1 Execute `uv run ruff check src`
- [ ] 8.2 Execute `uv run mypy src`
- [ ] 8.3 Verify no `PLR0904` violations in controller module

## Notes
- Tasks 1-3 can be done in parallel (each extracts independent method groups)
- Task 4 depends on 1-3 completing
- Task 5 depends on 4 completing
- Task 6 depends on 4 completing
- Tasks 7-8 must be sequential and final
