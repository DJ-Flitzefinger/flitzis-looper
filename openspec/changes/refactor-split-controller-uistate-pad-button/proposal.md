# Change: Refactor Python controller/UI modules to reduce Ruff complexity

## Why
Recent iterations added features quickly, but left several Ruff issues that now block further refactoring and make the code harder to maintain:

- `src/flitzis_looper/controller.py`: `PLR0904` (too many public methods), `BLE001` (blind `except Exception`)
- `src/flitzis_looper/ui/context.py`: `PLR0904` in `UiState` and `AudioActions`
- `src/flitzis_looper/ui/render/performance_view.py`: `_pad_button` exceeds complexity thresholds (`C901`, `PLR0912`, `PLR0914`, `PLR0915`)

These are symptoms of the same underlying issue: single classes/functions are acting as “god objects” and accumulating responsibilities.

## What Changes
- **Split `LooperController` into a package module**: create `src/flitzis_looper/controller/` and move the controller implementation into multiple focused modules.
- **Refactor controller API into sub-controllers** (e.g. loader/analysis, transport/playback, metering) to keep each class under Ruff’s public-method thresholds.
- **Replace blind exception handling**: remove `except Exception` in controller analysis triggering and catch only `RuntimeError` at that boundary (keep scope minimal).
- **Refactor `UiState` and action classes** into smaller “selector”/“actions” components so each class stays under the method threshold.
- **Split `_pad_button` into helper functions** (label formatting, progress overlay, peak meter drawing, input handling, overlays) while preserving UI behavior.

## Impact
- **Affected specs**: behavior SHOULD remain consistent with `performance-ui`, `performance-pad-interactions`, `async-sample-loading`, and `per-pad-metering`. This change proposal adds internal-quality requirements for controller/state/render structure.
- **Affected code**:
  - `src/flitzis_looper/controller.py` → `src/flitzis_looper/controller/` (package)
  - `src/flitzis_looper/ui/context.py` (refactor into smaller classes; consider package split only if it grows beyond ~500 LOC)
  - `src/flitzis_looper/ui/render/performance_view.py` (split `_pad_button`)
- **Breaking changes**: **Yes (internal)** — the `LooperController` API will be reshaped into grouped sub-objects and in-repo call sites will be updated accordingly. No compatibility/legacy flat-method API will be provided. The import path `from flitzis_looper.controller import LooperController` SHALL remain valid.
- **Tests/tooling**: Update Python tests that patch `flitzis_looper.controller.*` symbols if module boundaries move.

## Benefits
- Ruff clean for the listed violations.
- Clearer separation of concerns and easier unit-level testing.
- Safer evolution of controller/UI surface without continued complexity growth.
