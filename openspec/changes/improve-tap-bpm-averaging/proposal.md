# Change: Improve Tap BPM averaging

## Why

The current Tap BPM workflow reacts too strongly to recent tap jitter because it keeps only a
small fixed window of tap timestamps and waits until the third tap before updating the displayed
manual BPM. For manual song tempo detection, the performer needs a measurement that starts only
when Tap BPM is explicitly used, updates as soon as the second tap provides an interval, and gets
steadier as the performer continues tapping.

## What Changes

- Start a Tap BPM measurement only when the performer explicitly activates Tap BPM.
- Compute BPM by fitting one constant interval across all accepted taps in the current measurement
  series.
- Update the manual BPM after the second tap and after every later tap in the same series.
- Display pad BPM with two decimal places so the immediate Tap BPM result is visible at the
  precision the controller stores.
- Reset the current measurement series when the performer pauses for longer than 3 seconds before
  tapping again.
- Preserve the existing per-pad target behavior and manual-BPM override semantics.

## Non-Goals

- No automatic song BPM detection changes.
- No background Tap BPM measurement while audio is merely playing.
- No Rust audio callback changes, disk I/O, Python/GIL access from the audio callback, blocking
  audio-thread work, logging in the audio callback, or new real-time allocation behavior.

## Impact

- Affected specs: `pad-manual-bpm`
- Affected code: Python transport BPM controller and focused Python tests.
