# Change: Add phase-aware playback sync

## Why
Gen3 now has the building blocks for musical timing: a Rust-owned transport clock, a
fixed-capacity scheduler, quantized trigger routing, atomic exclusive playback transitions,
and bounded per-pad timing anchors derived from beatgrid/downbeat analysis.

The remaining gap is phase. Quantized starts currently land on the Rust transport grid, but
the pad still starts at its normal loop start. BPM lock currently tempo-matches pads by BPM
ratio, but it does not establish a shared downbeat/bar phase. That means loops can be tempo
matched while still sounding offset from one another.

This change defines the next narrow Gen3 behavior contract before implementation: use the
Rust transport timeline plus per-pad timing anchors to align quantized starts and BPM-lock
transport phase without adding heavy work to the audio callback.

## What Changes
- Define phase-aware quantized pad start and retrigger behavior.
- Define how Rust computes an initial pad sample frame from transport phase, pad BPM, active
  loop region, and the bounded per-pad phase anchor.
- Define how BPM lock can anchor the Rust transport downbeat to a selected playing pad using
  a fixed-size control request.
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
  message enum, and focused Rust tests. Python/controller changes are limited to publishing
  a fixed-size BPM-lock phase-anchor request when the anchor pad changes.

## Non-Goals
- No production Rust or Python implementation in this planning slice.
- No trigger-quantization UI controls in this change.
- No continuous time-slip, beat warping, or drift-correction loop for already playing pads.
- No replacement of the existing scheduler or `rtrb` ring-buffer architecture.
- No full beat-grid vector publication to the audio callback.
- No real-time stem separation, neural network inference, or stem cache implementation.
- No disk I/O, logging, blocking waits/locks, heap allocation, or Python/GIL access in the
  audio callback.
