## Why

Input failures, content movement and rejected controller publications can leave
the UI crashed or displaying intent/error/loop geometry belonging to a different
Native state or content owner. Mouse-only exception handling does not cover the
equivalent keyboard/MIDI paths.

## What Changes

- Keep expected keyboard/MIDI admission failures visible and nonfatal while
  preserving original captures and validated input timestamps.
- Preserve the previous saved loop/quantization state when synchronous Native
  admission fails; a later explicit identical request must remain possible.
- Move acknowledged loop geometry with its original content in the existing Pair
  mirror and reject stale diagnostic fallback to old numeric pad slots.
- Validate fixed pad-table dimensions at the persistence boundary.

## Non-goals

No broad exception suppression, unguarded audio fallback, cross-frame recapture,
invented ACK, data reset or changes to the sole realtime audio/scheduler path.
