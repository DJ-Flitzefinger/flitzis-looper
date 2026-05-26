# Audio Engine

Flitzis Looper uses a Rust audio engine for realtime playback and a Python
control/UI layer for orchestration. Python interacts with a single PyO3
`AudioEngine` object from the native `flitzis_looper_audio` module. The CPAL
audio callback renders without acquiring the Python GIL.

## Architecture Status

The Gen3 preparation stages for realtime callback safety, command/parameter
separation, state ownership, clock/scheduler ownership, loop/source/stem
alignment, input mapping, DSP-chain foundation, and the per-pad DJ isolator
replacement are complete. The current architecture is therefore ready for the
next focused OpenSpec-backed target, but no new FX module, plugin host,
deck/group/master chain, realtime stem separation, live loop-edit crossfade, or
broad rewrite is implied by this document.

The protected boundary is the CPAL audio callback and realtime hot path. Rust
modules outside that boundary may still be changed when a focused OpenSpec
change justifies the work and preserves realtime constraints.

## Runtime Split

- Python owns UI rendering, `ProjectState` persistence, settings, input-mapping
  edit UX, background sample/stem orchestration, and durable performer intent.
- Rust owns live audio truth: transport timeline, scheduler, mixer state, voice
  playheads, loop wrapping, playback-rate application, Key Lock processing,
  prepared-stem rendering, realtime parameter application, per-pad DSP,
  metering, and audio-to-control telemetry.

## Signal Path

```text
Python UI / controllers / persistence / background workers
-> PyO3 AudioEngine API
-> bounded Rust command ring + bounded Rust parameter ring
-> CPAL audio callback
-> bounded command drain and parameter coalescing
-> TransportTimeline + TransportScheduler
-> RtMixer voice slots
-> full-mix or prepared-stem source selection
-> source-frame loop wrap and voice playhead
-> playback-rate / BPM Lock / Key Lock processing
-> per-pad Rust DSP chain
-> per-pad gain, trigger velocity, master volume
-> metering and audio-to-control telemetry
-> system audio output
```

The callback does not perform disk I/O, JSON access, Python/GIL work, UI work,
plugin loading, neural inference, logging, blocking waits, or unbounded work.

## Module Structure

```text
rust/src/
|-- lib.rs                         # PyO3 module export
|-- messages.rs                    # fixed-size messages and shared descriptors
`-- audio_engine/
    |-- mod.rs                     # AudioEngine API and background orchestration
    |-- audio_stream.rs            # CPAL callback, drains, scheduler integration
    |-- buffer_retirement.rs       # non-audio retirement of large handles
    |-- constants.rs               # banks, grid size, slot count, ranges
    |-- dsp.rs                     # per-pad DSP chain and DJ isolator node
    |-- input_mapping.rs           # Rust MIDI capture/dispatch outside callback
    |-- mixer.rs                   # RtMixer, voices, loops, stems, gain, DSP
    |-- scheduler.rs               # fixed-capacity output-frame scheduler
    |-- transport.rs               # output-frame timeline and musical grid phase
    |-- voice_slot.rs              # voice state and per-voice stretch buffers
    |-- stretch_processor.rs       # bounded varispeed/master-tempo wrapper
    |-- sample_loader.rs           # non-realtime decode/cache/resample
    |-- analysis.rs                # non-realtime BPM/key/beat-grid analysis
    |-- stem_cache.rs              # prepared-stem validation/loading
    |-- progress.rs
    |-- channels.rs
    `-- errors.rs
```

## Command And Parameter Path

Control-to-audio communication is split into two bounded SPSC rings:

- Ordered command ring: play, stop, pause/resume, exclusive play, unload,
  sample publication, prepared-stem publication, loop regions, stem mode/mask,
  trigger quantization, Key Lock/BPM Lock mode, and other ordered state changes.
- Fast parameter ring: high-rate scalar updates such as volume, speed, master
  BPM, per-pad BPM, per-pad gain, and per-pad EQ/DSP targets.

The callback drains at most `MAX_CONTROL_MESSAGES_PER_CALLBACK` ordered
commands per invocation and at most `MAX_PARAMETER_MESSAGES_PER_CALLBACK`
parameter messages per invocation. Both current budgets are `64`. Drained
parameter messages are coalesced by identity before they are applied, so the
latest drained target wins for each parameter in that callback.

## Transport And Scheduling

`TransportTimeline` is the Rust-owned output-frame clock. It advances by the
number of rendered output frames and derives musical phase from sample rate,
master BPM, and downbeat anchor.

`TransportScheduler` stores bounded scheduled events by absolute output frame.
Immediate and quantized play/stop paths both use scheduler execution. Quantized
triggers choose when the pad becomes audible in output time; they do not seek
inside the source loop. Explicit pad-derived phase sync uses
`AudioEngine.anchor_transport_phase_from_pad(id)`.

Accepted master-BPM parameter updates apply to both transport-grid timing and
BPM-lock tempo matching while preserving current transport bar phase.

## Playback, Loops, And Stems

The runtime keeps output-frame scheduling separate from source-frame playback:

- Output frames belong to transport and scheduler timing.
- Source frames belong to loaded full-mix buffers, prepared stems, loop regions,
  and active voice playheads.

Python persists loop intent in seconds. Rust converts accepted loop regions to
half-open source-frame ranges and owns live playhead wrapping. Live loop edits
apply immediately: an in-range playhead is preserved; an out-of-range playhead
is clamped to the new loop start. There is no live loop-edit crossfade yet.

Stems are generated offline in Python background work. Rust accepts prepared
immutable stem buffers only after non-realtime validation and renders them
through the same source-frame playhead, loop, BPM Lock, Key Lock, per-pad DSP,
gain, metering, and telemetry path as full-mix playback. Active full-mix/stem
mode and stem-mask changes use bounded Rust-owned transition state with a short
128 source-frame crossfade.

## Speed, BPM Lock, And Key Lock

Speed and BPM Lock resolve to one Rust mixer tempo ratio per active voice:

- BPM Lock off: the ratio is the global speed multiplier.
- BPM Lock on with valid metadata: the ratio is `master_bpm / pad_bpm`.
- Pads without valid BPM metadata fall back to the global speed multiplier.

Key Lock selects how that ratio is rendered:

- Key Lock off uses varispeed semantics, so pitch follows playback speed.
- Key Lock on uses the bounded master-tempo wrapper in
  `stretch_processor.rs`, preserving source playhead timing while applying
  pitch compensation.

Key Lock settings are bounded scalar values persisted in `ProjectState` and
published to Rust as fixed-size control messages. Voice-local buffers are
constructed before callback rendering and reused during processing.

## Per-Pad DSP And EQ

`dsp.rs` provides the current fixed-size per-pad DSP chain. The first live node
is the per-pad 3-band DJ isolator. The public Python/UI EQ API remains
compatible with dB-oriented project intent, while Rust converts accepted live
targets to normalized DSP parameters:

- `0.0`: band kill
- `0.5`: neutral
- `1.0`: limited boost

The old standalone mixer EQ path is not active as a second processing stage.
Future DSP/FX work should extend the Rust DSP-chain path through focused
OpenSpec changes.

## Loading, Analysis, And Persistence

`AudioEngine.load_sample_async(id, path, run_analysis=True)` decodes, resamples,
caches the source under `samples/`, optionally analyzes BPM/key/beat grid, and
publishes a shared immutable sample handle to the audio thread. This work runs
outside the callback.

Project persistence stores durable intent in `samples/flitzis_looper.config.json`.
The controller restores project state, applies bounded audio settings, validates
stem cache metadata, schedules cached sample loads, and publishes input mapping
runtime state.

## Current Python API Surface

The native `AudioEngine` exposes lifecycle, sample, stem, playback, transport,
parameter, input mapping, loader/event polling, and waveform-render-data APIs.
The type stub in `src/flitzis_looper_audio/__init__.pyi` is the current compact
reference for the Python surface.

## Not Implemented Yet

- Audio device selection/configuration beyond the default CPAL output device.
- Broader channel-layout support beyond the current mono/stereo mapping.
- Realtime stem separation.
- Plugin hosting or external plugin scanning.
- Live loop-edit crossfade/zero-crossing transition policy.
- Deck/group/master DSP chains.
- A richer reliable audio-state snapshot/acknowledgement stream beyond current
  best-effort telemetry.

## Related Specs And Docs

- `openspec/specs/minimal-audio-engine/spec.md`
- `openspec/specs/ring-buffer-messaging/spec.md`
- `openspec/specs/load-audio-files/spec.md`
- `openspec/specs/play-samples/spec.md`
- `openspec/specs/per-pad-eq3/spec.md`
- `openspec/changes/add-rust-transport-timeline/`
- `openspec/changes/add-phase-aware-playback-sync/`
- `openspec/changes/add-offline-stem-cache/`
- `openspec/changes/add-stem-performance-controls/`
- `openspec/changes/add-low-jitter-input-mapping/`
- `openspec/changes/clarify-state-ownership-boundary/`
- `openspec/changes/prepare-dsp-fx-foundation/`
- `openspec/changes/archive/2026-05-26-replace-hardwired-eq-with-dj-isolator/`
