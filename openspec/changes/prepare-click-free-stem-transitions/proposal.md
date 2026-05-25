# Change: Prepare click-free stem transitions

## Why

Stage 6 made the loop/source-frame/stem alignment model explicit, but stem mode and stem mask
changes still switch the rendered source immediately. That can create discontinuities during
performance even though prepared stems are already source-frame aligned with the full mix.

This change prepares the smallest bounded Rust-side transition layer before DSP/FX foundation
work. It applies only to already accepted full-mix and prepared-stem source selections.

## What Changes

- Add bounded Rust-owned transition state for accepted stem mix-mode and enabled-mask changes.
- Crossfade from the previous stem source selection to the new selection over a fixed short ramp.
- Keep the ramp on the existing source-frame reader path before Key Lock, gain/EQ, metering, and
  telemetry.
- Preserve active voice source-frame continuity and loop wrapping during transitions.
- Keep realtime constraints unchanged: no disk I/O, Python/GIL access, logging, blocking waits,
  neural inference, plugin loading, or heavy allocation in the audio callback.

## Impact

- Affected specs: `loop-source-stem-alignment`.
- Affected docs: `docs/audio-loop-source-stem-alignment.md`,
  `docs/audio-performance-architecture-audit.md`, `docs/audio-engine.md`.
- Affected code: `rust/src/audio_engine/mixer.rs` and focused Rust mixer tests.

## Non-Goals

- No EQ replacement.
- No new visible DSP/FX effect.
- No DSP-chain foundation.
- No VST, LV2, CLAP, AU, or other plugin-hosting infrastructure.
- No real-time stem separation.
- No live loop-edit crossfade or loop-boundary transition policy.
- No disk I/O, JSON, Python/GIL access, logging, blocking waits, neural inference, or heavy
  allocation in the audio callback.
