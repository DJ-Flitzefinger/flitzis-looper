# Change: Add Rust transport timeline and quantized scheduler

## Why
Gen3 needs pad starts, restarts, loop timing, and future sync features to be driven by one
Rust-owned musical clock. The current audio callback has only a local frame counter for
status emission, and `PlaySample` messages are handled as soon as the callback drains them.
That preserves responsiveness, but it cannot provide sample-frame-accurate quantization,
bar/downbeat alignment, or phase-aware BPM lock.

This change defines the first Gen3 audio-timing contract before feature code is written.
The corrected contract makes the Rust masterclock permanent and independent of active pads:
the first-started, oldest active, or currently playing pad does not become the clock.

## What Changes
- Add a permanent Rust-owned global transport timeline advanced by rendered output sample frames.
- Track transport master BPM, beat phase, bar phase, and a downbeat-aligned transport anchor in Rust.
- Treat accepted performance master-BPM parameter updates as the shared Rust tempo for both
  transport-grid timing and BPM-lock tempo matching, preserving current transport bar phase.
- Add a fixed-capacity scheduler that executes events at absolute output-frame positions.
- Route quantized pad triggers through the scheduler while preserving existing immediate trigger
  behavior when quantization is disabled and preserving effective loop-start source frames when
  quantization is enabled.
- Define scheduler-full failure behavior without blocking, allocation, panic, or partial
  stop/start transitions.
- Define how beatgrid and downbeat metadata is prepared outside the audio callback and published
  to Rust as bounded per-pad timing metadata without redefining the permanent transport.
- Keep the existing fixed-size ring-buffer message-passing architecture.
- Document explicit real-time non-goals: no real-time stem separation, no neural network
  inference in the audio callback, and no disk I/O in the audio callback.
- Document the later Gen3 stems direction: offline/cached stems only, and only for pads that
  are not currently playing.

## Impact
- Affected specs: `transport-timeline` (new), `play-samples`, `multi-loop-mode`,
  `ring-buffer-messaging`, `audio-analysis`, `performance-ui`.
- Affected docs: `docs/architecture.md`.
- Later affected code: Rust audio stream, mixer, message enum, new transport/scheduler
  modules, and Python transport/UI controls when quantization is exposed.

## Non-Goals
- No production Rust or Python feature implementation in this planning step.
- No replacement of the existing `rtrb` message-passing architecture.
- No real-time stem separation.
- No neural network inference in the audio callback.
- No disk I/O in the audio callback.
- No stem generation or stem cache implementation in this change.
