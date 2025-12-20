## 1. Implementation
- [x] 1.1 Add a helper to derive a pad label from a loaded sample path (basename only)
- [x] 1.2 Set initial pad labels from current loaded/unloaded state when building the grid
- [x] 1.3 Update the affected pad label after load/unload operations
- [x] 1.4 Update active pad theme colors to use legacy green `#2ecc71` (RGBA `46, 204, 113, 255`) and active text `#000000`

## 2. Tests
- [x] 2.1 Add a small unit test that verifies pad label formatting does not include directory paths
- [x] 2.2 Run `uv run pytest`

## 3. Tooling / QA
- [x] 3.1 Run `uv run ruff check src`
- [ ] 3.2 Manual smoke: `python -m flitzis_looper`; load/unload a file; trigger/stop; confirm filename label + green active state
