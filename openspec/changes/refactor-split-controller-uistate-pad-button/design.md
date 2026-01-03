## Context
- `src/flitzis_looper/controller.py` is ~619 LOC and currently contains:
  - async loader event application (`poll_loader_events` + handlers)
  - analysis triggering and analysis state updates
  - metering peak decay + message polling
  - playback/transport + multi-loop behavior
  - global controls (volume/speed/key lock/bpm lock)
  - computed helpers (effective BPM/key)

- `src/flitzis_looper/ui/context.py` is ~327 LOC but has Ruff `PLR0904` for both:
  - `UiState` (public computed-state helpers)
  - `AudioActions` (UI-callable mutations)

- `src/flitzis_looper/ui/render/performance_view.py:_pad_button` has grown into a single function handling:
  - load/analyze status label formatting
  - progress bar drawing
  - peak meter drawing
  - input handling (left/right/middle)
  - overlays (pad number, bpm/key)

## Goals / Non-Goals
- Goals:
  - Remove the listed Ruff violations by decomposition (not by ignoring rules).
  - Preserve runtime behavior and external UI semantics per existing specs.
  - Keep `from flitzis_looper.controller import LooperController` working.
  - Keep patchability/testability (tests currently patch `flitzis_looper.controller.AudioEngine` and `flitzis_looper.controller.time.monotonic`).

- Non-Goals:
  - Changing UI visuals or interaction semantics.
  - Changing pad tag naming / item IDs beyond what is needed for refactor.
  - Refactoring Rust audio engine or message protocol.

## Decisions
### 1) Convert controller module to package
Create `src/flitzis_looper/controller/` and move controller responsibilities into separate modules.

The package `flitzis_looper.controller` will re-export the main entrypoint (`LooperController`) so `from flitzis_looper.controller import LooperController` remains valid.

Important: we will NOT preserve the existing flat-method `LooperController` API. Call sites in the repo (including UI context classes and tests) will be updated to use the new grouped controller surface. No compatibility layer will be provided.

Key constraint: existing tests patch `flitzis_looper.controller.AudioEngine` and `flitzis_looper.controller.time.monotonic`. After splitting, we will update patch points as needed (and keep them stable within this repo going forward).

### 2) Decompose LooperController by responsibility
Proposed internal module boundaries:
- `controller/facade.py`: small `LooperController` that wires dependencies and delegates.
- `controller/loader.py`: async loading state, loader event handlers, and analysis task state.
- `controller/transport.py`: trigger/stop/multi-loop, volume/speed, key-lock/bpm-lock, manual bpm/key helpers.
- `controller/metering.py`: pad-peak decay and audio message polling.
- `controller/validation.py`: shared validation helpers (`_ensure_finite`, BPM normalization, ID validation policy).

This is intended to keep each class below Ruff’s `PLR0904` threshold.

### 3) Replace blind exception handling at the audio boundary
`analyze_sample_async` currently catches `Exception` to update UI state on error. To keep scope minimal, the refactor will catch only `RuntimeError` at this boundary (satisfying Ruff `BLE001`) and keep the existing UI-visible error reporting behavior.

We will not introduce an exception-normalization adapter as part of this change.

### 4) Refactor UiState / actions into smaller sub-objects
To resolve `PLR0904` in `UiState` and `AudioActions`, shift from “flat bag-of-methods” to grouped accessors:
- `UiState` becomes a container exposing:
  - `state.pads` (pad selectors like label/loading/analysis/peak)
  - `state.banks` (bank selection helpers)
  - `state.global_` (global computed values like effective BPM)

Similarly `AudioActions` becomes a container exposing:
- `audio.pads` (trigger/stop/load/unload/analyze/tap)
- `audio.global_` (volume/speed/key lock/bpm lock)
- `audio.poll` (poll loader/audio messages)

This reduces public method counts per class without losing clarity.

If `src/flitzis_looper/ui/context.py` exceeds ~500 LOC after refactoring, convert it into `src/flitzis_looper/ui/context/` package and re-export `UiContext` from `ui/context/__init__.py`.

### 5) Split `_pad_button` into focused helpers
Refactor `_pad_button` into helpers that align with spec-driven UI concerns:
- compute pad state (loaded/loading/analyzing/active)
- format label text
- draw button + progress overlay
- draw peak meter
- handle hover/click interactions (press tracking, trigger/stop)
- draw overlays (pad number, bpm/key)

Keep the current behavior and IDs stable unless a spec requires changes.

## Risks / Trade-offs
- Import/package conversion for `flitzis_looper.controller` could break patch paths if not managed. Mitigation: preserve patch points or update tests as part of the same change.
- Refactoring UI state/actions may require updating multiple render sites. Mitigation: do this in small, verifiable steps and keep naming consistent.

## Migration Plan
1. Create controller package and move code behind a facade.
2. Update all in-repo imports and test patch points.
3. Refactor `UiState` and actions into grouped sub-objects; update render call sites.
4. Split `_pad_button` into helper functions.
5. Validate with Ruff and run Python tests.

## Open Questions
- None (decisions confirmed): the refactor will reshape the controller API (no compatibility flat-method layer) and will catch only `RuntimeError` at the `AudioEngine.analyze_sample_async` boundary.