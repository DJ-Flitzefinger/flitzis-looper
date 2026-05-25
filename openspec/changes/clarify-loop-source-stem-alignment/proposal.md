# Change: Clarify loop source-frame and stem-alignment model

## Why
The Gen3 architecture now has bounded callback work, separate command and parameter paths,
documented state ownership, and a shared Rust master-BPM bridge. Before DSP/FX foundation work can
start safely, loop positions, output-frame scheduling, source-frame playback, and prepared-stem
alignment need one explicit model.

## What Changes
- Define output-frame time as the scheduler/transport timeline and source-frame position as the
  per-voice read position.
- Clarify that loop regions are half-open source-frame ranges after Python's editable seconds are
  published to Rust.
- Clarify live loop-edit behavior: edits apply immediately, preserve the current source frame when
  it is still inside the new region, and clamp to the loop start when it is outside.
- Clarify that prepared stems must share full-mix sample rate, channel layout, frame count, and
  source-frame origin before publication.
- Clarify that full-mix/prepared-stem mode and mask changes preserve the voice playhead and share
  the same loop, BPM-lock, Key Lock, gain/EQ, metering, and telemetry path.
- Record the click-free transition plan without implementing a new DSP effect or new smoothing
  layer in this change.

## Impact
- Affected specs: `loop-source-stem-alignment`.
- Affected docs: `docs/audio-loop-source-stem-alignment.md`,
  `docs/audio-performance-architecture-audit.md`, `docs/audio-engine.md`.
- Affected tests: focused Rust mixer tests for live loop edits and stem mask changes preserving
  source-frame position.

## Non-Goals
- No new EQ, DSP/FX node, isolator, filter, delay, reverb, phaser, flanger, or plugin host.
- No real-time stem separation, neural inference, disk I/O, logging, blocking, heap allocation, or
  Python/GIL access in the audio callback.
- No change to current trigger quantization, explicit transport phase anchoring, or MIDI behavior.
- No broad Python-to-Rust rewrite and no durable project schema migration.
