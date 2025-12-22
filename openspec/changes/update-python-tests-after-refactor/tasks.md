# Update Python Tests After Refactor

This task list outlines the work needed to update the test suite after the FlitzisLooperApp → LooperController refactor.

## 1. Remove obsolete tests

- [ ] Delete `src/tests/flitzis_looper/test_app.py` (all 134 lines - 13 tests)
- [ ] Test references removed `FlitzisLooperApp` class

## 2. Update conftest.py fixtures

- [ ] Replace `audio_engine_mock` fixture that patches "flitzis_looper.app.AudioEngine" → "flitzis_looper.controller.AudioEngine"
- [ ] Replace `app` fixture returning `FlitzisLooperApp()` → `LooperController()`
- [ ] Replace `state` fixture returning `app.state` → separate `project_state` and `session_state` fixtures
- [ ] Add new fixtures:
  - [ ] `@pytest.fixture def controller() -> LooperController`
  - [ ] `@pytest.fixture def project_state() -> ProjectState`
  - [ ] `@pytest.fixture def session_state() -> SessionState`
  - [ ] `@pytest.fixture def ui_context(controller) -> UiContext`

## 3. Update test_state.py

- [ ] Update or remove `test_multi_loop_defaults_disabled` (old AppState has been split)
- [ ] Update or remove `test_speed_defaults_to_one` (now tests ProjectState.speed)
- [ ] Split into separate tests for ProjectState vs SessionState defaults
- [ ] Add tests for new state fields (selected_pad, selected_bank, sidebar states)

## 4. Create test_controller.py

- [ ] Test initialization (`test_controller_initializes_states`, `test_controller_initializes_audio_engine`)
- [ ] Test sample management (load_sample, unload_sample, is_sample_loaded) - 5 tests
- [ ] Test playback control (trigger_pad, stop_pad, stop_all_pads) - 5 tests
- [ ] Test audio parameters (set_volume, set_speed, reset_speed with clamping) - 6 tests
- [ ] Test mode toggles (set_multi_loop, set_key_lock, set_bpm_lock) - 3 tests
- [ ] Test error handling (invalid_sample_id, non_finite_values, trigger_unloaded) - 3 tests
- [ ] **Total: ~22 new tests**

## 5. Create test_models.py

- [ ] Test ProjectState defaults and validation - 6 tests
- [ ] Test SessionState defaults and validation - 5 tests
- [ ] Test model serialization/deserialization (Pydantic) - 2 tests
- [ ] **Total: ~13 new tests**

## 6. Create test_ui_context.py

- [ ] Test UiState computed properties (pad_label, is_pad_loaded, is_pad_active, etc.) - 8 tests
- [ ] Test AudioActions delegation to controller - 12 tests
- [ ] Test UiActions state mutations - 8 tests
- [ ] Test UiContext initialization and access - 2 tests
- [ ] **Total: ~30 new tests**

## 7. Create test_readonly_state_proxy.py

- [ ] Test ReadOnlyStateProxy read access - 2 tests
- [ ] Test ReadOnlyStateProxy write protection - 3 tests
- [ ] Test ReadOnlyStateProxy integration with UiState - 2 tests
- [ ] **Total: ~7 new tests**

## 8. Validation

- [ ] Verify all tests pass: `uv run pytest src/tests/flitzis_looper/ -v`
- [ ] Verify no broken imports or old class references
- [ ] Verify test coverage for all new classes and methods
- [ ] Run linting: `uv run ruff check src/tests/`
- [ ] Run type checking: `uv run mypy src/tests/`