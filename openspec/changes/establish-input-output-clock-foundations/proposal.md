## Why

Native MIDI records input time but loses it before playback dispatch; keyboard and mouse
launches have no captured time. The scheduler currently uses callback processing time and
ignores the device callback/playback timestamps, so later phase-aware launch work needs a
shared, validated timing foundation first.

## What Changes

- Share one Rust monotonic engine epoch across native MIDI, UI capture and output-clock mapping.
- Retain optional captured input time through normal/exclusive launches, bounded command and
  scheduler messages, and failed-direct MIDI fallback. A global launch batch uses one input time.
- Publish bounded, coherent output-clock observations and expose estimated device-time mapping
  and nearest-grid target diagnostics, including ties toward the future and unavailable mapping.
- Keep current immediate/future-boundary loop-start playback active during this foundation slice.
  The design records the intended synchronized launch contract for subsequent manual-grid and
  audible-DSP slices; activating it requires their validation.
- Correct OpenSpec configuration rule keys so the real spec-driven CLI applies the intended rules.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `input-mapping`: shared clock provenance and capture-to-command preservation for all launch paths.
- `transport-timeline`: bounded output-device clock observations and captured-input target diagnostics
  without changing current launch execution.
- `ring-buffer-messaging`: retain bounded timestamp fields across commands, scheduled events and
  queue-failure fallback without adding realtime synchronization.

## Impact

Rust stream/input/command/scheduler modules, focused launch-timing math, PyO3 API/stub, Python
input and UI action facades, playback controllers, tests and maintained architecture/UI docs.
There are no new runtime dependencies, persisted timing fields or performer controls.

## Non-goals

This slice does not activate nearest-boundary playback, source catch-up, signed grid phase mapping,
master bootstrap changes, DSP pre-roll or latency compensation. It does not change Rubber Band,
stem generation, loop markers, plugin support or variable-tempo behavior.

## Realtime constraints

MIDI capture remains outside the audio callback. The callback uses fixed-size observations,
bounded atomics/message work and preallocated scheduler state, with no locks, GIL, allocation,
logging, disk access or background inference. Device-time estimates are diagnostics, not a claim
of measured audible sample accuracy.
