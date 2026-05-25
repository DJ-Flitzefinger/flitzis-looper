# Change: Refine pitch BPM control

## Why

The right-side Pitch control currently exposes the underlying speed multiplier step directly.
Performers read and think about the control through the BPM display above it, so plus/minus clicks
and mouse movement need to land on musical BPM values instead of multiplier-percent increments.

The BPM display also needs a quick manual entry path for exact tempo targets without going through
the selected-pad sidebar.

## What Changes

- Adjust the global Pitch control in displayed-BPM units when an effective BPM reference is
  available.
- Make left-click plus and minus actions move the displayed BPM by 0.1 BPM per click.
- Make right-click plus and minus actions move the displayed BPM by 1.0 BPM per click.
- Snap mouse/slider Pitch movement to a 0.1 BPM grid.
- Add a visual center indicator beside the Pitch control at the 1.00x/default speed position.
- Allow double-clicking the BPM display to type a target BPM with two decimal places.
- Sanitize the BPM entry so only digits, `.` and `,` are accepted, with `,` interpreted as `.`.

## Non-Goals

- No change to the Rust audio callback, ring-buffer format, DSP algorithm, or persisted speed
  range.
- No disk I/O, Python/GIL access from the audio callback, blocking audio-thread work, logging in
  the audio callback, neural inference, or new real-time allocation behavior.
- No change to per-pad manual BPM entry in the selected-pad sidebar.
- No change to audio-side speed representation; Rust still receives the bounded speed multiplier.

## Impact

- Affected specs: `performance-ui`
- Affected code: Python performance UI, transport/global parameter helpers, and focused Python
  tests.
