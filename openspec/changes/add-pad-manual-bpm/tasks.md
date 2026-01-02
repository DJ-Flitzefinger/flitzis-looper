## 1. Spec-Driven Implementation
- [x] 1.1 Add per-pad manual BPM storage to `ProjectState` (e.g., `manual_bpm: list[float | None]`).
- [x] 1.2 Add runtime tap state (timestamps) in `SessionState` or UI-local state, scoped to the selected pad.
- [x] 1.3 Add controller actions:
  - [x] 1.3.1 Set manual BPM for a pad
  - [x] 1.3.2 Clear manual BPM for a pad
  - [x] 1.3.3 Register Tap BPM on mouse down and compute BPM from recent taps
- [x] 1.4 Add a computed "effective BPM" helper (manual BPM when set, otherwise detected BPM).

## 2. UI Changes
- [x] 2.1 Extend `sidebar_left` to include a BPM numeric input for the selected pad.
- [x] 2.2 Add a Tap BPM button/control in the left sidebar.
- [x] 2.3 Ensure Tap BPM registers on mouse down (not release).
- [x] 2.4 Update pad overlay BPM display to use effective BPM (manual overrides detected).

## 3. Validation
- [x] 3.1 Add/extend Python unit tests for:
  - [x] 3.1.1 Manual BPM set/clear behavior
  - [x] 3.1.2 Tap BPM averaging window behavior (5 taps)
  - [x] 3.1.3 Effective BPM selection (manual vs detected)
- [x] 3.2 Run `uv run ruff check src`.
- [x] 3.3 Run `uv run mypy src`.
- [x] 3.4 Run `uv run pytest`.
