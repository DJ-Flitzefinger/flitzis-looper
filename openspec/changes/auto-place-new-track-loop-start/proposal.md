# Align new-track loops and grids to the initial signal boundary

## Why

Commit 2c87137 skips leading silence but places the loop 5 ms before detected
activity while retaining an unrelated grid origin. The user requests the last
near-zero frame before activity and a matching editable grid seed, with counting
relative to the chosen loop and one regular beat of editor space before line 1.

## What changes

- Replace fixed pre-roll with the predecessor of the first crossing of a fixed
  +/-0.01 full-scale deadzone, widened after the user's screenshot-based test.
- Initialize new-track loop and independent persisted scalar grid base together.
- Keep the manual grid offset separate; preserve legacy and saved/manual intent.
- Display loop-relative beat positions; line 0 is the invisible left view edge.
- Preserve BPM/raw analysis and the remaining B2b1-to-B8/K1 development plan.

## Non-goals and realtime constraints

No musical-downbeat certification, variable map implementation, live SYNC change,
Beat This cutover, PCM trimming/padding, extra inference, callback scanning,
allocation, locking, disk I/O or GIL access. ALL and physical marker behavior
retain their contracts. The tolerance remains a first value for user testing.
