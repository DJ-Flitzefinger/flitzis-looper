## 1. Implementation
- [ ] 1.1 Remove all pad context-menu UI code (rendering, ids, labels, helpers)
- [ ] 1.2 Handle middle mouse click on a pad by selecting that pad (without triggering or stopping playback)
- [ ] 1.3 Ensure middle-click on already-selected pad is a no-op
- [ ] 1.4 Confirm pad selection highlighting and left sidebar content update correctly

## 2. Spec Alignment
- [ ] 2.1 Ensure implementation matches `openspec/changes/update-middle-click-to-select-pad/specs/performance-pad-interactions/spec.md`
- [ ] 2.2 Ensure implementation matches `openspec/changes/update-middle-click-to-select-pad/specs/performance-ui/spec.md`

## 3. Validation
- [ ] 3.1 Run `pytest`
- [ ] 3.2 Run `ruff check .`
- [ ] 3.3 Run `openspec validate update-middle-click-to-select-pad --strict`
