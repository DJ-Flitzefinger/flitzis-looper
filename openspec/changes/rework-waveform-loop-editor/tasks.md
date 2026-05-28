## 1. Specification

- [x] 1.1 Add OpenSpec deltas for loop defaults, `ALL`, bar stepping, middle-click seek,
  transport controls, in-frame-only editor presentation, toolbar close, and `Adjust Loop`
  toggle-close behavior.
- [x] 1.2 Run official strict OpenSpec validation for `rework-waveform-loop-editor`.

## 2. Loop Region Model And Controller

- [x] 2.1 Initialize newly loaded tracks with auto-loop enabled, `8.0` bars, and loop start `0.0`.
- [x] 2.2 Allow persisted `pad_loop_bars` values to represent finite numeric half-bar/integer
  values while preserving compatibility with existing integer project files.
- [x] 2.3 Add controller-owned maximum-bar validation from loop start, sample duration, and
  effective BPM; reject out-of-range bar changes as no-ops.
- [x] 2.4 Replace Reset behavior with `ALL`: explicit full-track start/end, auto-loop disabled,
  immediate Rust loop-region application, and focused regression tests.

## 3. Audio Seek Path

- [x] 3.1 Add a bounded Rust control message and PyO3 API for selected-pad source-position seek.
- [x] 3.2 Preserve realtime safety: no disk I/O, JSON, Python/GIL, UI calls, blocking locks,
  logging, neural inference, plugin loading/scanning, unbounded loops, heavy allocation, or
  long-running callback work.
- [x] 3.3 Implement before-loop, inside-loop, and after-loop explicit seek semantics without
  changing loop markers or broadening live loop-edit behavior.
- [x] 3.4 Add focused Rust and Python controller tests for seek API calls, playhead projection, and
  loop wrapping after explicit seeks.

## 4. Waveform Editor UI

- [x] 4.1 Keep empty-plot left-click loop-start placement, make it retrigger the selected pad from
  the new effective loop start, and keep middle-click as the separate playback seek shortcut.
- [x] 4.2 Add middle mouse-down seek and reconcile it with middle-drag pan without repeated seeks.
- [x] 4.3 Rename Reset to `ALL` and route it to the full-track controller action.
- [x] 4.4 Implement left-click power-of-two bar stepping and right-click exact `1.0` bar stepping
  with helper tests.
- [x] 4.5 Rework Play/Pause mouse-down and right-button hold behavior with selected-pad-only tests.
- [x] 4.6 Remove the separate waveform editor window, title bar, maximize/restore state, and
  floating/in-frame mode toggle.
- [x] 4.7 Render the waveform editor only in the center surface and add a right-aligned toolbar
  close button with focused helper coverage.

## 5. Selected-Pad Sidebar

- [x] 5.1 Make `Adjust Loop` close the editor when it is already open for the same pad.
- [x] 5.2 Make `Adjust Loop` switch the editor when a different loaded pad is selected.
- [x] 5.3 Add focused UI action tests for open, same-pad close, and different-pad switch.

## 6. Validation

- [x] 6.1 Run focused Python tests for loop/model/loader/UI action behavior.
- [x] 6.2 Run Rust seek and mixer tests through `uv run cargo test --manifest-path rust/Cargo.toml`.
- [x] 6.3 Run full validation for the finished behavior slice when feasible: `uv sync`,
  `uv run maturin develop`, `uv run cargo check --manifest-path rust/Cargo.toml`,
  `uv run cargo test --manifest-path rust/Cargo.toml`, `uv run pytest`,
  `uv run ruff check src`, and `uv run mypy src`.
- [x] 6.4 Re-run focused UI/model/input tests, `ruff`, `mypy`, strict OpenSpec validation, and
  `git diff --check` after the in-frame-only cleanup.

## Blockers And Non-Goals

- Any audio-callback disk I/O, JSON, Python/GIL access, UI calls, blocking locks, logging, neural
  inference, plugin scanning/loading, unbounded loops, heavy allocation, or long-running work is a
  blocker.
- Live loop-edit crossfade, plugin/DSP infrastructure, broad UI redesign, and stem-separation
  behavior are outside this change.
