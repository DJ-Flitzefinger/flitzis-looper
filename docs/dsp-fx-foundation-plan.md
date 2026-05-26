# DSP/FX Foundation Plan

Date: 2026-05-26

Status: Stage 8 architecture planning, the first neutral Rust foundation slice, the first
3-band DJ isolator replacement slice, and the focused low/high kill tuning follow-up are
complete. This document does not authorize unrelated DSP/FX effects, plugin hosting, real-time
stem separation, live loop-edit crossfades, or broad rewrites.

## Purpose

This document defines the internal Rust DSP/FX foundation and records the first safe per-pad EQ
replacement slice. It complements:

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

## Current EQ

The old hardwired mixer EQ path has been replaced as live audio authority:

- `rust/src/audio_engine/dsp.rs` now hosts the fixed-size per-pad DJ isolator node inside the
  internal DSP chain,
- `rust/src/audio_engine/mixer.rs` routes existing `set_pad_eq(...)` dB triplets to typed
  normalized per-pad DSP parameter identities,
- Python continues to persist per-pad low/mid/high dB intent and uses the same UI, restore, and
  input-mapping action semantics,
- live Rust targets are normalized `0.0..1.0`, smoothed on the audio side, and rendered before
  pad gain/master volume metering,
- the deleted `eq3.rs` path and per-voice `Eq3State` are no longer active as a second hardwired
  EQ processing stage.

## Current Processing Order

The long-term internal audio path should be:

```text
full-mix or prepared-stem source selection
-> source-frame loop wrap and voice playhead
-> playback-rate and Key Lock processing
-> optional future per-stem processing
-> per-pad DSP chain with DJ isolator node
-> per-pad gain, voice velocity, master volume, metering, telemetry
-> optional later deck/group/master chains
```

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

## First Isolator Replacement Slice

The first isolator implementation slice now replaces the old live EQ authority:

- a fixed-size DJ isolator node lives in each per-pad DSP chain,
- low, mid, and high targets use typed per-pad DSP parameter identities,
- existing Python-facing dB values are converted to normalized Rust targets with `0.5` neutral,
  minimum kill, and `+6 dB` maximum boost,
- inactive/project-restore updates snap current DSP state to the restored targets, while active
  playback target changes are smoothed before sample processing,
- the tuned isolator uses fixed-size Linkwitz-Riley-style splits for non-equal band gains and an
  equal-gain dry path for exact neutral transparency, all-band kill silence, and uniform all-band
  `+6 dB` boost,
- focused Rust DSP and mixer tests cover neutral transparency, full-kill behavior, bounded boost,
  smoothing, finite output, sample-rate preparation, and the absence of double-processing through
  the removed hardwired EQ path.

## Focused Isolator Review

The first review/audition slice kept runtime DSP unchanged and checked the implemented isolator
against deterministic representative sine tones. The Rust DSP-chain ownership, normalized
parameter path, smoothing, and all-band `+6 dB` boost cap remain the right architecture direction.
The all-band boost review measures about `1.995x` RMS at `1 kHz`, and the mid-kill path strongly
suppresses a `1 kHz` band-center tone.

The same review found that the initial low/high split was not archive-ready as final DJ isolator
behavior: low kill around `60 Hz` left substantial residual RMS, and high kill around `8 kHz`
could exceed neutral RMS instead of suppressing the high band.

The focused tuning follow-up keeps the same per-pad DSP-chain ownership and replaces the residual
`dry - low - high` reconstruction with fixed-size Linkwitz-Riley-style band splitting for
non-equal gains. It preserves exact dry output when all three band gains are equal, so neutral
transparency, all-band kill silence, and uniform all-band `+6 dB` boost stay bounded and
deterministic. Focused tests now require representative `60 Hz` low-kill and `8 kHz` high-kill
suppression while preserving other-band audibility. No unrelated FX, plugin hosting,
deck/group/master chains, live loop-edit crossfades, real-time stem separation, or UI redesign
was added.

## Parameter Model

DSP parameters should use stable typed identities, not callback-local pointers or UI object
references. An identity should be able to name:

- scope: initially per-pad, later optional per-stem/deck/master,
- pad or bus index where applicable,
- node slot or stable node kind,
- parameter kind or slot,
- normalized target value where the UI uses normalized controls.

High-rate controller input should continue to derive bounded targets outside the callback, send
accepted targets through the bounded parameter path, and smooth on the Rust side before sample
processing.

## DJ Isolator Replacement

The 3-band DJ isolator replacement lives at
`openspec/changes/replace-hardwired-eq-with-dj-isolator/`. Initial isolator targets:

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

- No unrelated visible filter, delay, reverb, phaser, flanger, or other effect in this slice.
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

For the first isolator implementation:

- run focused Rust DSP and mixer tests for neutral transparency, full kill, bounded boost,
  smoothing, finite output, sample-rate preparation, and no double hardwired EQ processing,
- run focused Python controller/UI/input-mapping tests if compatibility glue changes,
- run the broader uv-managed validation sequence because the change replaces live audio behavior.

For the focused low/high kill tuning follow-up:

- run official strict OpenSpec validation for `replace-hardwired-eq-with-dj-isolator`,
- run focused Rust DSP tests for representative band-center suppression and equal-gain behavior,
- run focused mixer tests for the existing no-double-processing bridge,
- run the broader uv-managed validation sequence because live audio DSP behavior changes.
