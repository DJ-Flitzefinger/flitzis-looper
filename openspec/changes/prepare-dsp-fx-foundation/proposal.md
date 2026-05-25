# Change: Prepare DSP/FX foundation

## Why

The architecture audit found that the current per-pad EQ is still hardwired into the mixer. Future
professional EQ and FX work needs an internal Rust DSP foundation first, otherwise the next EQ
change would deepen the current one-off path instead of creating a reusable realtime-safe
architecture.

This change defines the OpenSpec-backed foundation task before any visible EQ or effect work.

## What Changes

- Define the intended internal Rust DSP/FX foundation scope.
- Establish a neutral first per-pad chain host before the later isolator replacement.
- Define typed fixed-size DSP parameter identity and Rust-owned smoothing expectations.
- Keep the current EQ, UI controls, persistence, and audio output unchanged during the foundation
  slice.
- Keep plugin hosting, external plugin scanning, and Python DSP out of scope.

## Impact

- Affected specs: `dsp-fx-foundation`.
- Affected docs: `docs/dsp-fx-foundation-plan.md`,
  `docs/audio-performance-architecture-audit.md`, `docs/audio-engine.md`,
  `docs/message-passing.md`.
- Expected later code scope: a narrow Rust DSP module, fixed-size per-pad chain state, smoothing
  helpers, focused Rust tests, and only minimal mixer integration needed to prove neutral
  pass-through.

## Non-Goals

- No new EQ implementation.
- No visible DSP/FX effect.
- No VST, LV2, CLAP, AU, or other plugin-hosting infrastructure.
- No real-time stem separation.
- No live loop-edit crossfade.
- No broad rewrite or big-bang Python-to-Rust port.
- No disk I/O, JSON access, Python/GIL access, UI calls, logging, blocking waits, neural
  inference, plugin loading/scanning, heavy allocation, or unbounded work in the audio callback.
