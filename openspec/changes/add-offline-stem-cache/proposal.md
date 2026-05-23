# Change: Add offline stem cache and prepared stem-buffer mixing

## Why
Gen3 transport and phase-aware playback now give the audio engine a real-time-safe timing
foundation. The next legacy parity gap is stems: performers eventually need generated stem
sets and synchronized stem playback controls, but real-time stem separation would violate
the project's audio callback constraints.

This change defines the first stem contract before implementation: stem generation is
offline/background work, cached per pad/sample version, and only already prepared immutable
stem buffers may be published for future Rust mixing.

## What Changes
- Define a stem cache model for vocals, melody, bass, drums, and instrumental buffers.
- Require stem generation to run outside the audio callback as a background/offline task.
- Require generation and stem replacement to be blocked or deferred for pads that are
  currently playing.
- Define prepared stem buffers as immutable, sample-rate/channel/length-aligned audio data
  that can be published to Rust by handle.
- Define future stem mixing as bounded audio-thread state that shares the same voice
  playhead, loop region, transport timing, speed, BPM-lock, and key-lock behavior as the
  full mix.
- Keep the existing fixed-size ring-buffer message-passing architecture.
- Keep real-time stem separation, neural network inference, disk I/O, logging, blocking,
  heap allocation, and Python/GIL access out of the audio callback.

## Impact
- Affected specs: `stem-cache` (new), `background-tasks`, `ring-buffer-messaging`,
  `play-samples`.
- Affected docs: `docs/audio-engine.md`, `docs/message-passing.md`,
  `docs/todos-legacy-migration.md`.
- Later affected code: controller/background-task orchestration, project stem cache metadata,
  Rust message enum, Rust mixer stem-buffer storage, and focused Rust/Python tests.

## Non-Goals
- No production Rust or Python implementation in this planning slice.
- No neural stem separation model selection or dependency installation in this change.
- No real-time stem separation.
- No stem generation, disk I/O, decoding, neural inference, logging, blocking, heap
  allocation, or Python/GIL access in the audio callback.
- No stem UI controls, waveform UI changes, or performance stem toggles in this change.
- No replacement of the existing `rtrb` message-passing architecture.
- No continuous stem phase correction separate from the existing voice playhead and loop
  timing model.
