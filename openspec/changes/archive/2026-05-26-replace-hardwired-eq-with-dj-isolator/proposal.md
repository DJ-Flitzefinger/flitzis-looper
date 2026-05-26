# Change: Replace hardwired EQ with DJ isolator DSP node

## Why

The neutral Rust DSP foundation now exists, but the performer-facing per-pad EQ still lives as a
hardwired mixer path. Replacing it by patching the current mixer-specific EQ storage would deepen
the old architecture instead of using the new DSP foundation.

This change defines the OpenSpec-backed replacement target before implementation: the current
per-pad EQ behavior should move into a professional 3-band DJ isolator node hosted by the Rust
per-pad DSP chain, with realtime-safe smoothing and no plugin-hosting dependency.

## What Changes

- Replace the current hardwired per-pad EQ runtime authority with an internal Rust DSP-chain
  isolator node.
- Use normalized accepted live controls in `0.0..1.0`, where `0.5` is neutral.
- Define initial DJ isolator bands as low below `250 Hz`, mid from `250 Hz` to `4 kHz`, and high
  above `4 kHz`.
- Preserve existing performer workflows: selected-pad low/mid/high controls, middle-click neutral
  reset, durable project intent, input mappings, and restore behavior.
- Require Rust-side smoothing before sample processing and avoid double-processing through both
  the old hardwired EQ and the new DSP node.

## Impact

- Affected spec: `per-pad-eq3`.
- Affected docs: `docs/dsp-fx-foundation-plan.md`,
  `docs/audio-performance-architecture-audit.md`, and local handoff notes.
- Expected later code scope: `rust/src/audio_engine/dsp.rs`, `rust/src/audio_engine/mixer.rs`,
  `rust/src/audio_engine/eq3.rs` or its successor boundary,
  `rust/src/audio_engine/voice_slot.rs`, `rust/src/messages.rs`,
  Python controller/UI mapping glue as needed, and focused Rust/Python tests.

## Non-Goals

- No implementation in this planning slice.
- No new visible effect beyond replacing the existing per-pad EQ behavior.
- No delay, reverb, phaser, flanger, filter module, deck/group/master chain, or live loop-edit
  crossfade.
- No VST, LV2, CLAP, AU, external plugin host, plugin scanning, or dynamic plugin loading.
- No real-time stem separation.
- No broad Python-to-Rust port or UI rewrite.
- No disk I/O, JSON access, Python/GIL access, UI calls, logging, blocking waits, neural
  inference, plugin loading/scanning, heavy allocation, or unbounded work in the audio callback.
