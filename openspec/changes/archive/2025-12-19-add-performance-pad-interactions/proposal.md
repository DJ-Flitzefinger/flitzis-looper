# Change: Add performance pad interactions

## Why
The performance grid is only useful once pads can be interacted with quickly (trigger/retrigger, stop, context menu). The legacy app relies on left-click trigger, right-click stop, and middle-click context actions for rapid performance workflows.

## What Changes
- Wire pad mouse interactions:
  - Left-click: trigger / retrigger a pad.
  - Right-click: stop a pad.
  - Middle-click: open a per-pad context menu.
- Expand sample slot ID range from `0..32` to `0..36` to match the 6×6 grid.
- Add a stop API to the Rust-backed `AudioEngine` so a pad can be stopped deterministically.

## Impact
- Affected specs: `play-samples`, `load-audio-files`, `performance-pad-interactions` (new capability)
- Affected code: Rust message protocol and audio engine (`rust/src/messages.rs`, `rust/src/audio_engine.rs`), Python UI/app (`src/flitzis_looper/ui.py`, `src/flitzis_looper/app.py`)
- Dependency: Intended to follow `add-performance-ui-grid` so the pad widgets exist.
