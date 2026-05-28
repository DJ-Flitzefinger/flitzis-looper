# Change: Refine gain, master, and START/STOP controls

## Why

The performance controls need faster stage-safe gestures for live use:
per-pad Gain/Trim needs a deeper negative cut without moving its neutral
12 o'clock position, Master Volume needs a direct right-click mute position,
and the bottom bar needs a dedicated START/STOP control for stopping and
restarting the current live loop set.

## What Changes

- Extend the negative side of per-pad Gain/Trim to `-60.0 dB` while keeping
  `0.0 dB` at the normalized center and `+12.0 dB` at the positive end.
- Make right-clicking the Master Volume slider set the persisted Master Volume
  to `0.0`.
- Add a bottom-right START/STOP button immediately left of the Settings toggle.
- Make left mouse button down on START/STOP start or restart the current target
  loop set from the beginning of each loop.
- Make right mouse button down on START/STOP stop the current playing loop set
  immediately and never start playback.
- Move the momentary output mute to the mouse-wheel button on START/STOP without
  moving or persisting the Master Volume slider value.

## Non-Goals

- No new plugin hosting, stem behavior, sample loading, persistence format beyond
  existing gain bounds, or waveform-editor transport behavior.
- No disk I/O, Python/GIL access from the audio callback, blocking audio-thread
  work, logging in the audio callback, neural inference, or new real-time
  allocation behavior.
- No change to the positive Gain/Trim range, `0.0 dB` default, or center
  position.
- No change to the Master Volume persisted range.
- No change to pad-grid left/right mouse semantics; the START/STOP button reuses
  the same mouse-down timing pattern at global scope.

## Impact

- Affected specs: `per-pad-gain`, `performance-ui`
- Affected code: Python UI/controller/session state, Python/Rust gain bounds,
  focused Python/Rust tests, and architecture docs.
