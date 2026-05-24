# Change: Add phase-aware playback sync

## Why
Gen3 now has the building blocks for musical timing: a Rust-owned transport clock, a
fixed-capacity scheduler, quantized trigger routing, atomic exclusive playback transitions,
and bounded per-pad timing anchors derived from beatgrid/downbeat analysis.

The remaining gap is controlled phase behavior. Quantized starts land on the Rust transport grid
and must continue to start from the pad's effective loop start. BPM lock tempo-matches pads by BPM
ratio, but transport downbeat alignment must be an explicit sync operation rather than a side
effect of whichever pad happens to be playing.

This change defines the next narrow Gen3 behavior contract before implementation: keep normal
quantized starts loop-start based, keep bounded phase helpers available for explicit sync, and
avoid adding heavy work to the audio callback.

## What Changes
- Define that normal quantized pad starts and retriggers preserve the effective loop-start source frame.
- Define bounded Rust phase helpers without applying them implicitly to normal trigger source frames.
- Define how an explicit sync request can anchor the Rust transport downbeat to a selected playing
  pad using a fixed-size control request.
- Preserve immediate-trigger behavior when trigger quantization is disabled.
- Preserve scheduler-full rejection semantics and atomic MultiLoop-disabled transitions.
- Keep full beat-grid vectors, beat detection, disk I/O, Python/GIL access, allocation, and
  logging out of the audio callback.

## Impact
- Affected specs: `transport-timeline`, `play-samples`, `multi-loop-mode`,
  `time-stretch-pitch-shift`, `ring-buffer-messaging`.
- Affected docs: `docs/audio-engine.md`, `docs/message-passing.md`,
  `docs/todos-legacy-migration.md`.
- Later affected code: Rust transport, mixer, scheduler/audio-stream execution path,
  message enum, and focused Rust tests. Python/controller changes are limited to keeping
  BPM-lock tempo matching separate from explicit transport phase anchoring.

## Non-Goals
- No production Rust or Python implementation in this planning slice.
- No trigger-quantization UI controls in this change.
- No continuous time-slip, beat warping, or drift-correction loop for already playing pads.
- No replacement of the existing scheduler or `rtrb` ring-buffer architecture.
- No full beat-grid vector publication to the audio callback.
- No real-time stem separation, neural network inference, or stem cache implementation.
- No disk I/O, logging, blocking waits/locks, heap allocation, or Python/GIL access in the
  audio callback.
