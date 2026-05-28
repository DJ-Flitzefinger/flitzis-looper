## 1. Specification
- [x] 1.1 Create the OpenSpec proposal, design, tasks, and spec deltas for per-pad Key Lock.
- [x] 1.2 Run `openspec validate add-per-pad-key-lock --strict`.

## 2. Python State And Persistence
- [x] 2.1 Add `ProjectState.pad_key_lock` with one boolean per pad and all-False defaults.
- [x] 2.2 Validate `pad_key_lock` length and item types through the model.
- [x] 2.3 Add model and persistence tests for defaults, round-trip behavior, and invalid length.

## 3. Controller And UI Boundary
- [x] 3.1 Add controller APIs for setting and toggling one pad's Key Lock state.
- [x] 3.2 Change global `set_key_lock` so it overwrites every per-pad Key Lock value.
- [x] 3.3 Expose selected-pad Key Lock selectors and actions through `UiContext`.
- [x] 3.4 Add focused tests for global overwrite and independent per-pad toggles.

## 4. Rust Audio Engine
- [x] 4.1 Add bounded per-pad Key Lock state in the mixer/audio engine.
- [x] 4.2 Keep global `SetKeyLock(bool)` behavior as an all-pad overwrite.
- [x] 4.3 Add a fixed-size per-pad Key Lock control message plus PyO3/API support.
- [x] 4.4 Use the active voice's pad id to choose Key Lock processing during rendering.
- [x] 4.5 Add Rust tests for mixed per-pad states and global overwrite behavior.

## 5. Performance UI
- [x] 5.1 Render the selected-pad `KEY LOCK` button at the bottom of the left sidebar under Stem Mix / stem controls.
- [x] 5.2 Use the same on/off visual language as the global Key Lock button.
- [x] 5.3 Wire the button so it toggles only the selected pad.
- [x] 5.4 Add focused UI/context tests where feasible without pixel automation.

## 6. Validation
- [x] 6.1 Run focused Python tests for state, persistence, controller, and UI context changes.
- [x] 6.2 Run `uv run cargo check --manifest-path rust/Cargo.toml`.
- [x] 6.3 Run focused Rust tests for touched audio paths.
- [x] 6.4 Run broad validation from `codex-meta/workflow.md` before marking the feature complete.

Note: `uv run mypy src` still reports pre-existing typing issues in
`src/tests/flitzis_looper/ui/test_sidebar_left.py` and
`src/tests/flitzis_looper/ui/test_file_dialog.py`; app-package mypy, Rust tests,
Python tests, lint, format checks, OpenSpec validation, and `git diff --check`
passed.
