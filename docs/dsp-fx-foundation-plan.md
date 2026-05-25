# DSP/FX Foundation Plan

Date: 2026-05-26

Status: Stage 8 architecture planning plus the first neutral Rust foundation slice. This document
does not implement a new EQ, visible DSP/FX effect, plugin host, real-time stem separation, live
loop-edit crossfade, or broad rewrite.

## Purpose

This document defines the smallest safe internal Rust DSP/FX foundation step before the current
hardwired per-pad EQ is replaced. It complements:

- `docs/audio-performance-architecture-audit.md`,
- `docs/audio-state-ownership.md`,
- `docs/audio-loop-source-stem-alignment.md`,
- `docs/input-mapping-dsp-parameter-policy.md`,
- `openspec/changes/prepare-dsp-fx-foundation/`.

The current architecture already has the correct ownership direction: Rust owns live audio truth,
transport, scheduling, mixing, playback-rate application, and future DSP state; Python owns UI,
durable project intent, persistence, Settings, mapping edit UX, and offline/background
orchestration.

## Current Constraints

The CPAL audio callback and realtime hot path remain protected. DSP/FX work must not add disk I/O,
JSON reads/writes, Python/GIL access, UI calls, blocking locks, logging, neural inference, plugin
loading/scanning, unbounded loops, heavy allocation, or long-running work to the callback.

Existing preparation stages provide the boundary for this plan:

- ordered commands and continuous parameters use separate bounded queues,
- continuous parameters are coalesced by identity before audio-state application,
- Rust owns accepted live master BPM for both transport-grid timing and BPM-lock matching,
- source-frame playback and output-frame scheduling are documented separately,
- accepted active stem source-selection changes use bounded Rust transition state,
- future mapped DSP controls must resolve to bounded controller-owned targets before entering the
  Rust parameter path.

## Existing EQ

The current EQ path is useful but not the final architecture:

- `rust/src/audio_engine/eq3.rs` implements the current 3-band per-pad EQ coefficients and state,
- `rust/src/audio_engine/mixer.rs` stores `pad_eq` directly in `RtMixer`,
- `rust/src/audio_engine/voice_slot.rs` stores per-channel `Eq3State`,
- Python persists per-pad low/mid/high dB intent and sends `set_pad_eq(...)` through the
  continuous parameter path,
- the mixer currently applies EQ after source selection, loop wrapping, playback-rate/Key Lock
  processing, and before pad gain/master volume metering.

Stage 8 does not patch this EQ. The foundation should make room for a later replacement instead
of deepening the hardwired EQ path.

## Target Processing Order

The long-term internal audio path should be:

```text
full-mix or prepared-stem source selection
-> source-frame loop wrap and voice playhead
-> playback-rate and Key Lock processing
-> optional future per-stem processing
-> neutral first per-pad DSP chain
-> later per-pad isolator EQ node
-> per-pad gain, voice velocity, master volume, metering, telemetry
-> optional later deck/group/master chains
```

For the first implementation slice, the DSP chain must be neutral and must not replace the
current EQ. A safe first render order is therefore:

```text
source/stem -> playback-rate/Key Lock -> neutral per-pad DSP chain -> existing EQ -> gain/meter
```

This lets tests prove that the chain host can process audio without changing output before any
performer-facing DSP behavior is added.

## First Implementation Slice

The first executable task added an internal Rust DSP foundation with no visible effect:

- `rust/src/audio_engine/dsp.rs` defines the narrow module boundary,
- fixed-size internal parameter identifiers and node/chain state are represented by typed Rust
  enums/structs with no strings, pointers, plugin handles, Python objects, or dynamic metadata,
- a neutral no-op node is hosted by a per-pad chain,
- node state is stored in fixed-size per-pad mixer-owned chain state and prepared during mixer
  construction, not allocated during callback rendering,
- Rust-owned smoothing primitives exist for future normalized continuous DSP parameters,
- public Python UI/API behavior is unchanged,
- existing per-pad EQ controls and DSP output remain unchanged,
- focused Rust tests cover neutral pass-through, smoothing target progression, parameter
  clamping/rejection, reset/prepare behavior, and bounded fixed-size state.

The initial foundation should not add a visible filter, delay, reverb, phaser, flanger, isolator,
stem effect, deck/group/master chain, plugin host, or new UI control.

## Parameter Model

Future DSP parameters should use stable typed identities, not callback-local pointers or UI object
references. A future identity should be able to name:

- scope: initially per-pad, later optional per-stem/deck/master,
- pad or bus index where applicable,
- node slot or stable node kind,
- parameter kind or slot,
- normalized target value where the UI uses normalized controls.

High-rate controller input should continue to derive bounded targets outside the callback, send
accepted targets through the bounded parameter path, and smooth on the Rust side before sample
processing.

## Later EQ Replacement

The later 3-band DJ isolator should be a separate OpenSpec-backed behavior change after the
foundation exists. That change should replace the current hardwired EQ path rather than patch it.
That dedicated planning change now lives at
`openspec/changes/replace-hardwired-eq-with-dj-isolator/`.

Initial isolator targets:

- normalized internal controls in `0.0..1.0` with `0.5` neutral,
- low band below about `250 Hz`,
- mid band from about `250 Hz` to `4 kHz`,
- high band above about `4 kHz`,
- full-kill cut behavior at minimum,
- limited smooth boost, likely around `+6 dB` maximum,
- parameter smoothing before sample processing,
- transparent neutral path within floating-point tolerance,
- no plugin hosting or external plugin dependency.

Python can continue to own durable performer intent and UI gesture semantics, including
middle-click reset and mapping edit UX. Rust should own accepted live normalized DSP parameter
state and smoothing.

## Non-Goals

- No new EQ implementation in Stage 8.
- No visible filter, delay, reverb, phaser, flanger, or other effect in Stage 8.
- No VST, LV2, CLAP, AU, or other plugin-hosting infrastructure.
- No real-time stem separation.
- No live loop-edit crossfade policy.
- No broad Python-to-Rust port.
- No Python DSP or callback access to Python/GIL state.

## Validation Plan

For the Stage 8 planning slice:

- run official strict OpenSpec validation for `prepare-dsp-fx-foundation`,
- run `git diff --check`.

For the neutral foundation implementation:

- run focused Rust DSP/mixer tests through
  `uv --no-cache run cargo test --manifest-path rust/Cargo.toml`,
- run `uv run cargo check --manifest-path rust/Cargo.toml`,
- run Python tests only if UI/controller/API behavior changes,
- run the broader uv-managed sequence if behavior, bridge contracts, or shared audio state change.

For the dedicated isolator planning slice:

- run official strict OpenSpec validation for `replace-hardwired-eq-with-dj-isolator`,
- run `git diff --check`.

For the later isolator implementation:

- run focused Rust DSP and mixer tests for neutral transparency, full kill, bounded boost,
  smoothing, finite output, sample-rate preparation, and no double hardwired EQ processing,
- run focused Python controller/UI/input-mapping tests if compatibility glue changes,
- run the broader uv-managed validation sequence because the change replaces live audio behavior.
