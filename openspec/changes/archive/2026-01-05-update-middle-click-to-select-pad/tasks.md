## 1. Implementation
- [x] 1.1 Remove all pad context-menu UI code (rendering, ids, labels, helpers)
- [x] 1.2 Handle middle mouse click on a pad by selecting that pad (without triggering or stopping playback)
- [x] 1.3 Ensure middle-click on already-selected pad is a no-op
- [x] 1.4 Confirm pad selection highlighting and left sidebar content update correctly

## 2. Spec Alignment
- [x] 2.1 Ensure implementation matches `openspec/changes/update-middle-click-to-select-pad/specs/performance-pad-interactions/spec.md`
- [x] 2.2 Ensure implementation matches `openspec/changes/update-middle-click-to-select-pad/specs/performance-ui/spec.md`

## 3. Validation
- [x] 3.1 Run `pytest` (currently fails with 2 failures: `TestModeToggles.test_bpm_lock_anchors_master_bpm_to_selected_pad`, `test_project_state_defaults`)
- [x] 3.2 Run `ruff check .`
- [x] 3.3 Run `openspec validate update-middle-click-to-select-pad --strict`
