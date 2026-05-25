# Change: Prepare realtime callback safety

## Why

The Gen3 audio callback already avoids Python, disk I/O, neural inference, and blocking work, but
the architecture audit found three concrete hot-path hazards that should be removed before DSP/FX
foundation work:

- the callback drains all pending control messages without a per-callback work budget,
- larger-than-requested device callback blocks can exceed the current fixed stretch-buffer
  assumptions,
- replacing, unloading, rejecting, or stopping sample/stem buffers can drop the final `Arc` in the
  callback.

These are safety and preparation changes, not user-facing EQ or FX work.

## What Changes

- Bound the number of control messages processed by one audio callback invocation.
- Leave overflow control messages queued for later callbacks instead of draining indefinitely.
- Split oversized render segments into callback-safe chunks that fit the existing preallocated
  per-voice stretch buffers.
- Defer retired sample and prepared-stem handles to a non-audio worker so large buffer
  deallocation does not occur on the callback thread.
- Preserve existing immediate and quantized playback behavior for normal command rates.
- Preserve the current MIDI integration path; MIDI remains outside the audio callback and reaches
  audio only through the existing bounded control path.

## Impact

- Affected specs: `ring-buffer-messaging`, `play-samples`, `load-audio-files`.
- Affected docs: `docs/audio-performance-architecture-audit.md`, `docs/audio-engine.md`,
  `docs/message-passing.md`.
- Affected code: Rust audio stream callback, mixer/voice buffer lifetime paths, focused Rust
  tests.

## Non-Goals

- No new EQ implementation.
- No new DSP/FX implementation.
- No VST, LV2, CLAP, AU, or other plugin-hosting infrastructure.
- No MIDI latency or jitter rework.
- No Python DSP, GIL access, disk I/O, logging, blocking waits, neural inference, or heavy
  allocation in the audio callback.
