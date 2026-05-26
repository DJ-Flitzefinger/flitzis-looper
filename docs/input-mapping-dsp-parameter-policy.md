# Input Mapping DSP Parameter Policy

Date: 2026-05-26

Status: maintained input-mapping policy for current and future Rust DSP parameters. The per-pad
DJ isolator now uses the bounded Rust DSP parameter path; this document does not authorize a new
DSP/FX node, plugin host, direct MIDI-to-callback path, or new MIDI behavior.

## Purpose

This document defines how keyboard and MIDI mappings should approach current and future DSP
parameters after the Gen3 low-jitter input work and the first DSP-chain foundation. It complements
`docs/audio-performance-architecture-audit.md`, `docs/message-passing.md`, and
`docs/audio-state-ownership.md`.

## Current Boundary

Current mapped input has two paths:

- Direct Rust dispatch for audio-safe discrete actions: pad trigger, pad stop, and stop all.
- Python/controller dispatch for actions that require durable project intent, UI/session state,
  Learn behavior, stem/cache orchestration, or current parameter interpretation.

The Rust MIDI layer may capture, timestamp, normalize, and resolve mappings outside the CPAL audio
callback. It must not become a direct callback path. The audio callback only observes bounded
command messages or bounded parameter messages later.

## DSP Parameter Rules

Mapped DSP parameters should use these rules:

1. Keyboard and MIDI Note mappings may save bounded set-value actions when the UI target value is
   explicit and finite.
2. MIDI CC and NRPN increment/decrement mappings should save relative-step actions for performer
   knobs and encoders, not raw hardware ticks.
3. Controller-owned relative steps should clamp and normalize targets outside the audio callback.
4. Accepted continuous DSP parameter targets should cross to Rust through the bounded parameter
   path, not the ordered command path.
5. The audio side should coalesce by parameter identity and then smooth from the old DSP parameter
   state to the new target before sample processing.
6. Mapping files should store stable action keys and durable intent, not raw DSP node pointers,
   callback-local addresses, plugin handles, or device-specific runtime state.
7. Direct Rust dispatch should remain limited to small discrete audio-safe command transactions.
   Future DSP parameters must not be added to that direct dispatch list unless the target is a
   fixed-size, bounded parameter update that still passes through the established parameter queue.

## Stale Snapshot Policy

Rust direct dispatch currently uses a small Python-published runtime snapshot for loaded pad state,
loop regions, and Multi Loop mode. That snapshot can be slightly stale until the next frame sync.
This is acceptable for current trigger gating because direct dispatch remains all-or-nothing and
falls back by rejecting unsafe or incomplete command sequences.

DSP mappings should not depend on this snapshot for live DSP truth. Rust owns live DSP parameter
state after accepted parameter updates, while Python owns persisted performer intent and
mapping-edit UX. If a mapped DSP action needs context, the controller should derive the target
value outside the callback and publish a typed parameter update to Rust.

## Non-Goals

- No new EQ, isolator, filter, delay, reverb, phaser, flanger, or other FX.
- No plugin hosting, plugin scanning, or plugin parameter mapping.
- No direct MIDI-to-audio-callback execution.
- No Python/GIL access, JSON access, logging, blocking waits, file I/O, neural inference, or heavy
  allocation in the audio callback.
