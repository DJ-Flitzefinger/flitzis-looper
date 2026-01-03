## 1. Implementation
- [ ] 1.1 Inventory `LooperController` call sites and test patch points
- [ ] 1.2 Create `src/flitzis_looper/controller/` package and define the new grouped controller API surface
- [ ] 1.3 Migrate in-repo call sites to the new controller API (no compatibility flat-method layer)
- [ ] 1.4 Split controller responsibilities into loader/analysis, transport, and metering modules
- [ ] 1.5 Replace blind `except Exception` with `except RuntimeError` for analysis triggering
- [ ] 1.6 Rewrite `UiState` into grouped selector objects; update render call sites
- [ ] 1.7 Rewrite audio/UI action classes to match the new controller API; update call sites
- [ ] 1.8 Split `performance_view._pad_button` into helper functions; keep UI behavior stable

## 2. Validation
- [ ] 2.1 Run Ruff and confirm listed violations are gone
- [ ] 2.2 Run Python tests (`pytest`) and fix any regressions caused by import/patch path changes
- [ ] 2.3 (If configured) run `mypy` for the touched modules

## 3. Safety / Rollback
- [ ] 3.1 Ensure `from flitzis_looper.controller import LooperController` still works
- [ ] 3.2 Ensure patch points used in tests remain valid (or update tests accordingly)
