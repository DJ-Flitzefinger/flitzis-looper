# Change: Restrict sidebar manual BPM entry

## Why

The selected-pad sidebar manual BPM field currently uses ImGui float input behavior, which can let
non-performer-facing characters enter the edit flow and does not clamp typed manual BPM values to
the requested performer range.

## What Changes

- Replace the selected-pad sidebar BPM field with the same text-entry filtering model used by the
  right-side BPM display entry.
- Accept only `0` through `9`, `.`, and `,` while typing, with `,` normalized to `.`.
- Ignore other typed characters without changing the field contents.
- Clamp committed manual BPM values below 0.5 BPM to 0.5 BPM and values above 400 BPM to 400 BPM.
- Preserve clearing the field as the way to remove the manual BPM override.

## Non-Goals

- No change to Tap BPM timing, tap averaging, or detected BPM analysis.
- No change to the Rust audio callback, ring-buffer format, DSP algorithm, or audio-side tempo
  representation.
- No disk I/O, Python/GIL access from the audio callback, blocking audio-thread work, logging in
  the audio callback, neural inference, or new real-time allocation behavior.

## Impact

- Affected specs: `performance-ui`
- Affected code: Python selected-pad sidebar UI and focused Python UI tests.
