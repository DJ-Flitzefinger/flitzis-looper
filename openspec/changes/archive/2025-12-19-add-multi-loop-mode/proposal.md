# Change: Add multi-loop mode (polyphonic looping)

## Why
The current application has no explicit "MultiLoop" control like the legacy UI, and pad triggering uses standard button-click semantics (typically firing on mouse release). For performance parity, a performer needs:

- A fast toggle between "one-at-a-time" playback and true polyphonic looping.
- Immediate pad onset on **mouse down** for tighter timing control.
- Clear visual feedback about which pads are currently active.

## What Changes
- Add a **MultiLoop** toggle control to the performance view (legacy-style), positioned below the bank buttons.
- Default MultiLoop mode to **disabled** on startup.
- When **MultiLoop is enabled**, triggering a pad starts/restarts that pad without stopping other active pads.
- When **MultiLoop is disabled**, triggering a pad stops any other active pads first (legacy “one-at-a-time” behavior).
- Change pad triggering and right-click stopping so actions fire on **mouse down** (not mouse release).
- Visually indicate which pads are currently active (playing).
- Expose a `AudioEngine.stop_all()` API to enable efficient “stop others” behavior without issuing many per-pad stop calls.

## Impact
- Affected specs: `performance-ui`, `performance-pad-interactions`, `play-samples`, `multi-loop-mode` (new)
- Affected code (expected): `src/flitzis_looper/ui.py`, `src/flitzis_looper/app.py`, `rust/src/audio_engine.rs`, `rust/src/messages.rs`, `src/flitzis_looper_rs/__init__.pyi`, tests under `src/tests/` and `rust/src/`
- User-visible behavior: MultiLoop button appears; pads can layer loops when enabled; “one-at-a-time” when disabled; pad onset happens on mouse down; active pads are highlighted.
