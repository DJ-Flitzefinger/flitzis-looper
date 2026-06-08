# Change: Refine Gain and stem pad indicators

## Why

Two small performance UI details currently slow down live use. The per-pad
Gain/Trim control uses relative drag movement, so the mouse can drift away from
the visible handle instead of directly placing it. The pad grid also shows a
compact `BLK` stem badge for playing pads, even though the selected-pad sidebar
already explains why stem generation is blocked.

## What Changes

- Make left mouse interaction on the selected-pad Gain/Trim control set the
  target from the pointer's absolute position on the control, including an
  immediate jump on click and direct tracking while dragging.
- Preserve the existing asymmetric normalized Gain/Trim mapping: negative range
  on `0.0..0.5`, neutral at `0.5`, and positive range on `0.5..1.0`.
- Keep right mouse fine-drag and fine-step behavior, middle-click reset, and
  controller clamping unchanged.
- Stop rendering the compact pad-grid blocked stem badge; the selected-pad
  sidebar remains the detailed blocked-status surface.

## Non-Goals

- No change to the per-pad Gain/Trim dB range, default, persistence format,
  audio-engine publication, smoothing, or signal-path placement.
- No change to stem generation gating, blocked reasons, cache validation, or
  sidebar stem controls.
- No disk I/O, Python/GIL access from the audio callback, blocking audio-thread
  work, logging in the audio callback, neural inference, or new real-time
  allocation behavior.

## Impact

- Affected specs: `per-pad-gain`, `performance-ui`
- Affected code: Python UI render helpers and focused Python tests.
