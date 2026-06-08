# Change: Rework pad Gain as dB Trim

## Why

The current per-pad Gain behaves as a `0..100%` volume scalar. It defaults to 100%, cannot boost,
is not centered around 0 dB, and does not match professional pad/channel Gain or Trim behavior.
The selected-pad UI also does not provide a dedicated dB readout, level meter, or clip hold near
the Gain control.

## What Changes

- Replace per-pad Gain semantics with a dB-based pad/channel Trim control.
- Default all pads to `0.0 dB`, with a bounded `-12.0 dB..+12.0 dB` range.
- Migrate legacy saved `pad_gain` linear or percent values so old unity loads as `0.0 dB`.
- Route dB trim targets through the existing bounded Rust parameter path.
- Convert dB targets to linear gain in Rust and smooth active changes before sample multiplication.
- Place the Gain trim stage before per-pad EQ/DSP and before trigger/master volume.
- Move the user-facing Gain value out of pad controls and render it below the Gain control.
- Add a horizontal gain-area meter with green/yellow level zones and a separate clip hold
  indication.
- Remove the redundant vertical right-edge level meter from performance pad buttons while keeping
  the loaded-pad BPM/key metadata overlay.

## Impact

- Affected specs: `per-pad-gain`, `per-pad-metering`, `performance-ui`, `pad-button`,
  `ui-state`, `project-persistence`, `per-pad-eq3`.
- Affected code: Python models/controllers/input mapping/UI and Rust mixer/messages/PyO3 API.
- Validation: strict OpenSpec validation, focused Python/Rust tests, and uv-managed Rust/Python
  checks for the touched paths.

## Non-Goals

- No new master-volume behavior.
- No compressor, limiter, or automatic gain prevention.
- No deck/group/master DSP chain.
- No plugin hosting or external plugin scanning.
- No realtime stem separation.
- No disk I/O, JSON access, Python/GIL access, UI calls, logging, blocking waits, neural
  inference, plugin loading/scanning, heavy allocation, or unbounded work in the audio callback.
