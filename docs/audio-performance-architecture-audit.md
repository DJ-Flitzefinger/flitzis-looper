# Audio Performance Architecture Audit

Date: 2026-05-25

Status: architecture audit and preparation plan. This document does not implement EQ, DSP
effects, plugin hosting, or runtime behavior changes.

## Scope

This audit reviews whether the current Gen3 Rust/Python audio architecture is ready to become a
professional live-performance looper foundation for playback, loops, pitch/BPM changes, Key Lock,
stems, master clock, quantization, Multi Loop, MIDI/keyboard control, future DSP/FX, and a future
3-band DJ isolator.

The short answer is: the ownership direction is correct, but the architecture is not ready for
professional DSP/FX work without preparation. Rust is already the right place for the realtime
audio engine, scheduler, transport, mixing, and future internal DSP nodes. The next work should
prepare realtime safety, command/parameter flow, state ownership, and clock semantics before the
EQ is replaced.

Explicit non-goals for this audit:

- no new EQ implementation,
- no new delay, reverb, phaser, flanger, filter, or other FX implementation,
- no plugin host,
- no big-bang engine rewrite,
- no regression of the currently working MIDI behavior,
- no attempt to run Python DSP in the audio path.

## High-Level Map

Major Rust audio modules:

- `rust/src/audio_engine/audio_stream.rs`: CPAL stream setup and the audio callback.
- `rust/src/audio_engine/mod.rs`: PyO3 `AudioEngine` API, loader/task orchestration, producer
  ownership, stem publication helpers, input-runtime lifecycle.
- `rust/src/messages.rs`: fixed message enums and shared buffer descriptors.
- `rust/src/audio_engine/mixer.rs`: `RtMixer`, fixed sample slots, voice slots, loop playback,
  stem mixing, speed/BPM-lock/key-lock routing, gain/EQ, metering, playheads.
- `rust/src/audio_engine/voice_slot.rs`: active voice state, per-voice DSP buffers, EQ state.
- `rust/src/audio_engine/stretch_processor.rs`: bounded current varispeed/master-tempo wrapper.
- `rust/src/audio_engine/eq3.rs`: current per-pad EQ DSP.
- `rust/src/audio_engine/transport.rs`: output-frame transport clock and musical phase helpers.
- `rust/src/audio_engine/scheduler.rs`: fixed-capacity absolute output-frame scheduler.
- `rust/src/audio_engine/input_mapping.rs`: Rust MIDI capture, timestamping, filtering, mapping
  snapshot resolution, and dispatch outside the CPAL callback.
- `rust/src/audio_engine/sample_loader.rs`, `analysis.rs`, `stem_cache.rs`: non-realtime decode,
  analysis, cache validation, and stem artifact handling.

Major Python/UI/controller modules:

- `src/flitzis_looper/app.py`: app entrypoint.
- `src/flitzis_looper/controller/app.py`: controller composition and project restore.
- `src/flitzis_looper/controller/loader.py`: load/unload and sample-state orchestration.
- `src/flitzis_looper/controller/transport/`: playback, BPM, global parameters, loop operations.
- `src/flitzis_looper/controller/stems.py`: stem cache/generation/publication orchestration.
- `src/flitzis_looper/controller/settings.py`: non-audio settings validation.
- `src/flitzis_looper/input_mapping/`: Learn, keyboard, mapping storage, Python-owned actions.
- `src/flitzis_looper/ui/render/`: pad grid, sidebars, bottom bar, settings surface.
- `src/flitzis_looper/models.py`: durable `ProjectState` and transient `SessionState`.
- `src/flitzis_looper/persistence.py` and controller persistence helpers: JSON persistence
  outside realtime operation.

Relevant durable planning/spec sources:

- `AGENTS.md`
- `docs/audio-engine.md`
- `docs/message-passing.md`
- `docs/time-stretch-and-pitch-shift.md`
- `docs/todos-legacy-migration.md`
- `openspec/specs/per-pad-eq3/spec.md`
- `openspec/specs/ring-buffer-messaging/spec.md`
- `openspec/specs/time-stretch-pitch-shift/spec.md`
- `openspec/specs/loop-region/spec.md`
- `openspec/specs/multi-loop-mode/spec.md`
- `openspec/changes/add-rust-transport-timeline/`
- `openspec/changes/add-phase-aware-playback-sync/`
- `openspec/changes/repair-key-lock-master-tempo/`
- `openspec/changes/add-low-jitter-input-mapping/`
- `openspec/changes/add-offline-stem-cache/`
- `openspec/changes/add-stem-performance-controls/`

Local continuation memory lives outside the repository in `../codex-meta/`.

## Current Architecture Summary

The current runtime has a useful split:

```text
Python UI/controller/persistence/offline prep
-> PyO3 AudioEngine methods
-> bounded Rust control ring buffer
-> CPAL callback-owned scheduler, transport, mixer
-> system audio output
```

Audio-to-control telemetry is also sent through a bounded ring buffer and polled by Python.
MIDI is now captured in Rust outside the audio callback and enters either the direct bounded audio
command path for simple playback actions or the Python controller path for controller-owned
actions. This is the right direction.

The current implementation is, however, a feature-accumulated engine rather than a complete
professional DSP/FX architecture. Several important concepts are present but not yet cleanly
separated:

- discrete commands and continuous parameter updates share one control ring,
- transport BPM and mixer BPM are separate concepts,
- durable Python state and live Rust state duplicate several authorities,
- the callback drains all available commands without a per-callback budget,
- buffer and prepared-stem handle lifetimes may still drop large allocations on the callback,
- current EQ is hardwired into the mixer rather than modeled as a DSP node,
- future parameter smoothing/coalescing is not defined as a first-class system.

## Rust Audio Engine And Hot Paths

The CPAL audio callback is created in `rust/src/audio_engine/audio_stream.rs`. It owns:

- the control-to-audio consumer ring,
- the audio-to-control producer ring,
- `RtMixer`,
- `TransportTimeline`,
- `TransportScheduler`,
- trigger quantization state,
- status cadence counters.

The callback currently:

1. drains all pending control messages with `while let Ok(message) = consumer_in.pop()`,
2. updates mixer/transport/scheduler state from those messages,
3. renders scheduled audio segments into the CPAL output buffer,
4. advances the transport by rendered output frames,
5. emits bounded status messages such as `SampleStarted`, `SampleStopped`, `PadPeak`, and
   `PadPlayhead`.

Positive findings:

- The callback does not acquire the Python GIL.
- The callback does not decode audio, read JSON, scan files, run Demucs, or do network work.
- MIDI ports and Learn state are outside the callback.
- Core mixer arrays and scheduler storage are fixed-capacity.
- Per-voice stretch buffers are constructed before callback rendering.
- Stem generation and cache validation are outside the callback.

Risks and gaps:

- The control-message drain is unbounded per callback. UI/MIDI/DSP parameter bursts can consume
  callback time before audio is rendered.
- The scheduler is fixed-capacity but insertion/drain operations shift arrays. This is bounded,
  but burst behavior should be measured and budgeted.
- `render_scheduled_audio` and `RtMixer::render` both clear output ranges. This is minor, but it
  is unnecessary hot-path work.
- `RtMixer::render` assumes the callback frame count stays within the internal
  `DEFAULT_BLOCK_SAMPLES` render buffers. If CPAL delivers a larger block than requested, the
  stretch output can be indexed past its internal capacity. The engine needs block splitting or a
  hard guarded render loop before professional use.
- `SampleBuffer` and `PreparedStemSet` are shared through `Arc` handles. Loading, unloading, or
  replacing them in the callback may drop the last strong reference on the callback thread. Large
  buffer deallocation belongs outside the realtime path.
- Audio-to-control messages are best-effort. Dropped `SampleStarted` or `SampleStopped` telemetry
  can desynchronize Python `SessionState` from live Rust state.
- Loop, stem-mask, EQ, and several parameter changes are immediate and unsmoothed, so clicks or
  discontinuities are possible during performance.

## Ringbus, Command, And Parameter Path

The current ringbus carries multiple categories:

- discrete commands: play, stop, stop all, exclusive play, pause/resume, unload,
- publication commands: loaded full-mix sample buffers, prepared stem buffers,
- mode updates: stem mode, stem mask, trigger quantization, BPM lock, Key Lock,
- continuous or frequently updated parameters: volume, speed, pad gain, pad EQ, master BPM,
  pad BPM, loop region.

The message types are explicit and typed, which is good. The problem is that event semantics are
not separated from parameter semantics. A professional performance engine should distinguish:

```text
Discrete events:
  trigger, stop, exclusive transition, unload, publish prepared data.

Continuous parameters:
  speed, pitch, gain, EQ/isolator, FX wet/dry, filter cutoff, feedback, stem level.
```

Current risks:

- Some Python-facing methods return ring-full errors; many parameter setters intentionally ignore
  push failures and return success. That is acceptable only if the system documents those setters
  as best-effort and coalesced. Today there is no general coalescing model.
- A burst of continuous updates can flood the same queue needed by discrete trigger/stop commands.
- Fast future DSP parameters such as filter cutoff or isolator bands need smoothing and a more
  explicit last-value-wins path.
- Direct Rust MIDI trigger dispatch currently sends multi-message sequences for some state, such
  as loop region plus play. If the queue fills between messages, the audio state can receive a
  partial transaction.

Target boundary:

```text
UI / Controller / MIDI / Keyboard
-> typed discrete commands or typed parameter updates
-> bounded realtime-safe bridge
-> Rust scheduler / mixer / DSP parameter state
```

The audio callback must not reach back into Python or UI state.

## Clock, Timing, And Scheduling

The `TransportTimeline` is the permanent output-frame clock. It advances by rendered audio frames
and can derive musical grid positions from sample rate, BPM, and downbeat anchor. The
`TransportScheduler` schedules commands by absolute output frame and supports in-buffer event
offsets.

Positive findings:

- Scheduling is based on output sample frames, not UI frame timing.
- Immediate and quantized starts share the scheduler execution path.
- Quantized starts currently preserve source-side loop start and choose only the output start
  frame from the global transport grid.
- MultiLoop-disabled exclusive starts are atomic in the scheduler when quantized.

Important issue:

- `ControlMessage::SetMasterBpm` currently updates mixer BPM-lock state but does not update
  transport master BPM. Quantization and tempo matching can therefore use different master BPM
  concepts unless an explicit transport-anchor operation happens. This is not good enough for a
  professional master-clock architecture.

Further gaps:

- There is no single documented authority for "performance master BPM" versus "BPM-lock tempo
  ratio" versus "transport grid BPM".
- Scheduled events are sample-frame based, but many Python-side edits are still stored and
  reasoned about in seconds.
- Scheduler-full and audio-telemetry-drop behavior need clearer UI/controller reconciliation.
- There is no documented policy for very rapid trigger bursts beyond bounded queue/scheduler
  rejection.

## BPM, Pitch, Key Lock, And Master Tempo

BPM currently appears in several places:

- detected BPM and manual BPM in Python project state,
- session master BPM in Python controller state,
- per-pad BPM in Rust mixer state,
- `RtMixer.master_bpm` for BPM-lock tempo ratio,
- `TransportTimeline.master_bpm` for quantization grid timing.

Pitch/speed and Key Lock are routed in Rust through `RtMixer` and `StretchProcessor`:

- BPM Lock off: global speed controls varispeed ratio.
- BPM Lock on with valid metadata: tempo ratio is `master_bpm / pad_bpm`.
- Key Lock off: varispeed changes speed and perceived pitch.
- Key Lock on: current bounded master-tempo wrapper attempts pitch compensation after varispeed.

Positive findings:

- The conceptual distinction between varispeed and Key Lock exists in Rust.
- Key Lock state is not handled in Python DSP.
- Per-voice DSP buffers are preallocated for the current bounds.
- Prepared stems and full mix share the same voice timing path before Key Lock processing.

Risks:

- The current Key Lock processor is a bounded pragmatic internal implementation, not a verified
  DJ-grade time-stretch/pitch engine.
- Live speed and BPM changes are smoothed for tempo ratio but not governed by a broader parameter
  smoothing layer.
- Transport BPM and mixer BPM are not unified.
- Simultaneous BPM lock, pitch changes, quantized starts, loop edits, and stem toggles need a
  deeper state-transition audit before new FX are layered in.

## Loop Editor And Loop Points

Python stores loop points as seconds in project state and clamps them through the controller.
Rust converts loop start/end seconds to frame positions using the output sample rate and stores
per-pad loop start/end frames in `RtMixer`. Playback starts at the effective loop start and wraps
at the loop end.

Positive findings:

- Rust has a frame-level loop region once the command reaches the mixer.
- Quantized triggers preserve loop starts.
- Loop state is published through typed messages rather than read from Python in the callback.

Risks:

- The persisted/editable representation is seconds while the playback representation is frames.
  This is workable, but source sample frame versus output timeline frame needs to be documented
  more explicitly.
- Changing loop points during playback immediately clamps or moves the playhead if needed. There
  is no crossfade, zero-crossing search, or click-suppression strategy.
- Loop changes are not clearly immediate versus quantized as a policy.
- Fast loop edits can interleave with playback/trigger/stem state through the shared control ring.

## Stems

Stem generation is offline/cache-based in Python through a replaceable backend boundary, with
Demucs as the first adapter. Cache validation and prepared-stem publication happen outside the
callback. The callback stores already prepared immutable stem buffers only for inactive, current
source-version pads.

Positive findings:

- Real-time stem separation is not in the audio callback.
- Stem cache work, Demucs, Torch, FFmpeg, disk I/O, and model handling stay outside the callback.
- Prepared stems match the full-mix sample shape before publication.
- The mixer renders stems through the same voice playhead, loop, BPM-lock, Key Lock, gain/EQ,
  metering, and playhead path as full-mix playback.
- Phase alignment is structurally sound when prepared stems are aligned to the loaded full mix.

Risks:

- Stem toggles are immediate mask changes with no smoothing or click-free transition.
- Prepared stem handle replacement may have the same callback-drop risk as full-mix buffers.
- There is no per-stem DSP/level architecture yet.
- The cached `instrumental.wav` artifact is not the `I` playback layer; this is intentional, but
  future code must not accidentally add it as a fifth stem.

## Multi Loop Mode

Multi Loop mode uses controller state to choose whether pad triggers call normal `play_sample` or
exclusive `play_sample_exclusive`. Rust can play multiple voices/pads together and BPM-lock them
through per-pad BPM metadata and mixer master BPM.

Positive findings:

- MultiLoop-disabled exclusive switching has an atomic scheduled command for quantized use.
- Multiple active pads share the Rust mixer, transport, scheduler, and status path.
- Normal quantized starts use the permanent global transport instead of redefining the clock from
  whichever pad starts.

Risks:

- Tempo-reference ownership remains partly Python/session and partly Rust/mixer/transport.
- BPM-lock master BPM and transport grid BPM can diverge.
- There is no complete per-deck or group state model.
- Pad/deck/global state transitions under simultaneous pitch, keylock, loop edits, stem toggles,
  and quantized starts need focused tests before adding future DSP chains.

## Python/Rust Boundary And Persistence

Python should remain responsible for:

- UI rendering and interaction,
- project persistence and JSON writes,
- Learn UX and mapping-file edits,
- offline sample/stem preparation orchestration,
- Demucs/backend process management,
- non-realtime settings validation and project dirty state.

Rust should own:

- audio callback, transport timeline, scheduler, mixing,
- source playhead and loop wrap,
- playback-rate and Key Lock processing,
- prepared-stem mixing,
- future internal DSP/FX chains and realtime parameter application,
- low-jitter MIDI capture/normalization outside the callback.

Current problems:

- ProjectState, SessionState, mixer state, and transport state duplicate pieces of pad/activity,
  BPM, loop, stem, and parameter truth.
- Telemetry drops can leave Python active/paused state stale.
- Several setter methods cross the bridge as best-effort without acknowledgement or coalescing.
- Persistence is correctly outside realtime operation, but state restoration sends many individual
  messages through the same queue used for live commands.

## MIDI And Keyboard Integration

The current MIDI issue is solved and should not be reopened as a latency bug. Architecturally,
MIDI is now much better placed:

- MIDI capture lives in Rust outside the CPAL callback.
- Supported events are timestamped near receipt with a monotonic timestamp.
- Normalization and filtering avoid JSON/UI work in the MIDI hot path.
- Direct audio-safe actions bridge into the existing bounded command path.
- Controller-owned actions are reported to Python for bounded non-realtime handling.

Remaining architecture points:

- The dispatcher can drop events under queue pressure, which is acceptable only if documented and
  surfaced where needed.
- Direct Rust-dispatched actions should avoid multi-message partial transactions for future
  trigger/loop/DSP operations.
- Runtime state snapshots from Python may be slightly stale until the next UI sync.
- Future MIDI CC/NRPN mappings to DSP parameters should use coalescing plus smoothing rather than
  enqueueing every hardware tick as an immediate callback update.

## Current EQ Implementation

The current EQ is not merely UI-level; it affects audio in Rust:

- UI/Python stores per-pad EQ in dB values.
- `AudioEngine.set_pad_eq(...)` sends `ControlMessage::SetPadEq`.
- `RtMixer` stores per-pad `Eq3Params`.
- `eq3.rs` applies current per-voice/channel EQ state during mixing.

Findings:

- Current UI range is dB-based, with `0.0 dB` neutral and `-60 dB` treated as kill.
- Current crossover constants in code are `380 Hz` and `2300 Hz`.
- Repository spec text expects a higher-quality crossover-based isolator direction, and the new
  target direction is a DJ-style internal Rust isolator with initial `250 Hz` and `4 kHz`
  crossovers and normalized `0.0..1.0` controls where `0.5` is neutral.
- The current implementation recomputes EQ coefficients in the callback when parameter messages
  arrive. It is not per-sample, but it is still not a good model for high-rate mapped controls.
- There is no parameter smoothing for EQ changes.
- The EQ is hardwired into mixer rendering rather than represented as a reusable DSP node.

Recommendation: do not patch this EQ into the future architecture. Keep it as the current
functional placeholder, then replace it later with an internal Rust 3-band DJ isolator DSP node
after the realtime-safety, command/parameter, state, and clock preparation phases.

## Critical Findings

Professional-readiness blockers before EQ/DSP replacement:

1. Bounded but unbudgeted callback command drain can create glitches under UI/MIDI/parameter
   bursts.
2. Large audio buffer handle replacement may deallocate in the callback if the callback owns the
   last `Arc`.
3. `DEFAULT_BLOCK_SAMPLES` assumptions can fail if the device callback delivers larger buffers.
4. Discrete commands and continuous parameters share one queue with mixed drop/error semantics.
5. No general coalescing or smoothing path exists for future fast DSP parameters.
6. Mixer master BPM and transport master BPM are separate authorities.
7. Python session state can desynchronize if audio-to-control telemetry is dropped.
8. Loop, stem, and EQ changes are immediate and can click.
9. Current EQ is not a durable DSP-chain foundation.
10. There is no per-pad/per-stem/deck/master DSP node architecture.

## Recommended Target Architecture

Recommended long-term structure:

```text
UI / Controller / MIDI / Keyboard
-> typed intents
-> command router
-> discrete event queue + coalesced parameter update path
-> Rust transport/scheduler/mixer
-> internal Rust DSP chains
-> output
```

Recommended audio processing model:

```text
Pad / Stem source buffers
-> source playback, loop wrap, pitch/rate, Key Lock
-> per-stem mix and optional future per-stem level/DSP
-> per-pad trim/gain
-> per-pad DSP chain
-> optional deck/group bus
-> optional deck/group DSP chain
-> master mix
-> master DSP chain
-> output
```

Initial target ownership:

- Rust owns realtime timing, scheduling, playback, source position, stem alignment, DSP chains,
  parameter smoothing, and audio rendering.
- Python owns UI, persistence, settings, mapping edit UX, offline/cache preparation, and
  background stem generation.
- MIDI and keyboard converge on the same typed action model after mapping. MIDI may keep its Rust
  low-jitter capture path outside the audio callback.
- Transport and BPM state must have one documented authority for grid timing and one documented
  relationship to BPM-lock tempo matching.
- Loop points should be represented in source frames in Rust and persisted/restored with a clear
  conversion contract.
- Prepared stems must stay aligned by frame count, sample rate, channel layout, source-version
  identity, and source-frame origin.

## DSP/FX Foundation Plan

Use internal Rust DSP modules. Do not add VST, LV2, CLAP, AU, plugin scanning, or dynamic plugin
hosting in this phase.

Initial DSP foundation scope:

- Define a small internal DSP node interface for fixed-channel block processing.
- Preallocate node state and scratch buffers outside realtime rendering.
- Apply realtime-safe parameter updates through typed parameter IDs/slots.
- Add parameter smoothing for gain-like and continuous controls.
- Handle sample-rate changes through explicit `prepare(...)` or rebuild outside hot rendering.
- Keep denormal/NaN guards local to DSP nodes.
- Add deterministic unit tests for neutral pass-through, bounded output, parameter clamping,
  smoothing behavior, sample-rate reconfiguration, and no callback allocation by construction.

Possible node shape:

```text
prepare(sample_rate_hz, max_block_frames, channels)
set_parameter(parameter_id, target_value)
process_block(input, output, frames)
reset()
```

The first real node should be the 3-band DJ isolator after the foundation is ready:

- bands: low below 250 Hz, mid 250 Hz to 4 kHz, high above 4 kHz,
- normalized controls: `0.0` full kill, `0.5` neutral, `1.0` limited boost,
- musical nonlinear kill curve,
- smooth limited boost curve, likely around +6 dB maximum,
- proper crossover/band-splitting design,
- transparent neutral path,
- smoothed parameter changes,
- no allocations, locks, logging, file I/O, JSON, plugin loading, or Python/GIL access in the
  callback.

## Staged Deep-Analysis Plan

Stop after each stage if context is getting large. Preserve findings in this document or a focused
follow-up document under `docs/`, and update `../codex-meta/handoff/next-step.md`.

### Analysis Stage 1: Repository And Architecture Map

Status: completed by this audit.

Questions:

- Where are the major modules?
- Where are Rust/Python/audio callback/bridge entry points?
- Where do pads, stems, pitch, Key Lock, loops, quantization, and EQ live?

Docs updated:

- this audit report,
- `docs/audio-engine.md`,
- `docs/message-passing.md`,
- local Codex meta handoff/current-state files.

### Analysis Stage 2: Realtime Safety And Buffer Lifetime

Goal: audit and prepare the callback hot path before feature DSP work.

Files:

- `rust/src/audio_engine/audio_stream.rs`
- `rust/src/audio_engine/mixer.rs`
- `rust/src/audio_engine/voice_slot.rs`
- `rust/src/audio_engine/stretch_processor.rs`
- `rust/src/messages.rs`

Questions:

- What is the maximum callback work under command bursts?
- Can buffer/stem `Arc` drops happen on the callback?
- What happens if CPAL delivers more than `DEFAULT_BLOCK_SAMPLES`?
- Which message handlers do nontrivial work in the callback?

Expected output:

- OpenSpec-friendly refactor proposal for callback budgets, deferred audio-buffer retirement, and
  block-splitting/guarded rendering.

### Analysis Stage 3: Command And Parameter Architecture

Files:

- `rust/src/messages.rs`
- `rust/src/audio_engine/mod.rs`
- `rust/src/audio_engine/audio_stream.rs`
- `rust/src/audio_engine/input_mapping.rs`
- `src/flitzis_looper/controller/`
- `src/flitzis_looper/input_mapping/`

Questions:

- Which messages are events and which are parameters?
- Which setters may be dropped?
- What needs coalescing?
- How should future DSP parameters reach Rust?

Expected output:

- discrete command path plus continuous parameter/coalescing design.

### Analysis Stage 4: Clock, BPM, Pitch, And Scheduler

Files:

- `rust/src/audio_engine/transport.rs`
- `rust/src/audio_engine/scheduler.rs`
- `rust/src/audio_engine/mixer.rs`
- `src/flitzis_looper/controller/transport/`
- `openspec/changes/add-rust-transport-timeline/`
- `openspec/changes/add-phase-aware-playback-sync/`

Questions:

- Which BPM is authoritative for transport?
- Which BPM is authoritative for tempo matching?
- How do pitch, BPM Lock, Key Lock, quantization, and Multi Loop interact?

Expected output:

- one documented clock/BPM ownership model and focused tests.

### Analysis Stage 5: Loop, Source Position, Key Lock, And Stems

Files:

- `rust/src/audio_engine/mixer.rs`
- `rust/src/audio_engine/stretch_processor.rs`
- `src/flitzis_looper/controller/transport/loop.py`
- `src/flitzis_looper/controller/stems.py`
- `openspec/changes/add-offline-stem-cache/`
- `openspec/changes/add-stem-performance-controls/`

Questions:

- Are loop points source-frame accurate?
- How should loop edits apply during playback?
- Are stems always frame-aligned under pitch/Key Lock?
- Where should future per-stem processing happen?

Expected output:

- loop/stem source-position model and click-free transition plan.

### Analysis Stage 6: Python State, Rust State, And Persistence Boundary

Files:

- `src/flitzis_looper/models.py`
- `src/flitzis_looper/controller/app.py`
- `src/flitzis_looper/controller/persistence.py`
- `src/flitzis_looper/controller/transport/`
- `rust/src/audio_engine/mod.rs`

Questions:

- Which state is durable intent?
- Which state is live audio truth?
- Which audio telemetry must be reliable or recoverable?
- What should be acknowledged versus best-effort?

Expected output:

- state ownership table and restoration/acknowledgement strategy.

### Analysis Stage 7: MIDI/Keyboard Under Future DSP Parameters

Files:

- `rust/src/audio_engine/input_mapping.rs`
- `src/flitzis_looper/input_mapping/`
- `src/flitzis_looper/ui/context.py`
- `openspec/changes/add-low-jitter-input-mapping/`

Questions:

- How should CC/NRPN map to future DSP parameters?
- Which events need coalescing before the audio side?
- How can direct Rust dispatch avoid partial transactions?

Expected output:

- MIDI/DSP parameter mapping policy without changing current working MIDI behavior.

### Analysis Stage 8: DSP/FX And EQ Replacement Architecture

Files:

- `rust/src/audio_engine/eq3.rs`
- `rust/src/audio_engine/mixer.rs`
- `src/flitzis_looper/ui/render/sidebar_left.py`
- `src/flitzis_looper/constants.py`
- `openspec/specs/per-pad-eq3/spec.md`

Questions:

- What is the minimal DSP node abstraction?
- How should EQ UI values map to internal normalized values?
- Where should per-pad/deck/master DSP chains live first?

Expected output:

- OpenSpec delta for DSP foundation and later isolator replacement.

### Analysis Stage 9: Consolidated Implementation Plan

Goal: consolidate earlier analysis into a small ordered set of behavior/refactor changes that
future chats can execute one by one.

Expected output:

- `docs/` and `../codex-meta/handoff/next-step.md` updated with the exact next task.

## Preparation And Refactoring Plan

Every behavior-changing phase needs an OpenSpec change or affected active delta update before or
alongside implementation, followed by official `openspec validate <change-id> --strict`.

### Phase 1: Architecture Audit And Documentation Cleanup

Status: completed by this documentation slice.

Goal: record the architecture map, risks, target direction, and next steps.

Scope:

- repository docs and local Codex meta only.

Non-goals:

- no runtime behavior change,
- no OpenSpec behavior delta,
- no EQ/DSP implementation.

Checks:

- `git diff --check`.

Acceptance:

- future chats can find the current phase and next step from durable files.

### Phase 2: Realtime-Safety Cleanup

Goal: remove known hot-path hazards before future DSP work.

Scope:

- budget command draining per callback or otherwise bound worst-case control handling,
- guard or split render blocks larger than internal DSP buffers,
- audit/defer large `Arc` buffer retirement outside the callback,
- reduce redundant hot-path clearing where safe.

Affected files:

- `audio_stream.rs`, `mixer.rs`, `voice_slot.rs`, `stretch_processor.rs`, `messages.rs`,
  focused Rust tests, docs/OpenSpec.

Non-goals:

- no new EQ,
- no new FX,
- no change to MIDI semantics.

Risks:

- changing callback scheduling can alter edge timing,
- deferred retirement needs careful ownership.

Tests/checks:

- Rust unit tests for oversized block handling, command-burst budget behavior, and buffer
  retirement policy,
- full uv-managed Rust/Python validation if behavior changes.

Acceptance:

- no unbounded command drain,
- no callback panic for larger device blocks,
- no known large-buffer deallocation on callback.

Rollback:

- keep each safety change isolated so it can be reverted without touching feature behavior.

Ordering:

- must happen before DSP/EQ implementation.

### Phase 3: Command/Ringbus/Parameter Path Cleanup

Goal: separate event commands from continuous parameters and define overload behavior.

Scope:

- classify all current messages,
- introduce coalescing or last-value-wins parameter publication for fast controls,
- make discrete trigger/stop semantics reliable under parameter floods,
- make multi-message direct MIDI actions atomic where needed.

Non-goals:

- no new DSP algorithm,
- no plugin host.

Affected files:

- `messages.rs`, `audio_engine/mod.rs`, `audio_stream.rs`, `input_mapping.rs`, Python transport
  and input-mapping controllers, docs/OpenSpec/tests.

Tests/checks:

- queue-full behavior tests,
- continuous parameter coalescing tests,
- MIDI direct-dispatch transaction tests.

Acceptance:

- future DSP parameters have a documented and tested path.

Ordering:

- before DSP foundation and EQ replacement.

### Phase 4: State Ownership Cleanup

Goal: define durable Python intent versus live Rust audio truth.

Scope:

- state ownership table for BPM, pitch, Key Lock, loop points, stems, pad activity, and DSP params,
- decide which audio-to-control events require recovery or reconciliation,
- reduce duplicated master BPM authority.

Non-goals:

- no new performer controls unless needed for the ownership model.

Tests/checks:

- restore/reload tests,
- telemetry-drop reconciliation tests where practical,
- focused controller/native API tests.

Acceptance:

- no unclear duplicated authority for performance-critical state.

Ordering:

- before clock stabilization and DSP replacement.

### Phase 5: Clock/Scheduler Stabilization

Goal: unify transport grid timing, BPM-lock tempo matching, quantized starts, and Multi Loop
behavior.

Scope:

- make the relationship between transport BPM and mixer/BPM-lock master BPM explicit,
- ensure scheduling and state changes use sample-frame time,
- add tests for simultaneous quantized starts, stops, loop edits, pitch changes, and BPM changes.

Non-goals:

- no time-slip/warp feature unless separately specified.

Acceptance:

- one documented master-clock model,
- no hidden active-pad clock side effects,
- quantization behavior remains predictable under Multi Loop.

Ordering:

- before DSP foundation.

### Phase 6: DSP/FX Foundation

Goal: add the internal Rust DSP-chain framework without adding a visible new effect.

Scope:

- DSP node trait/structs,
- per-pad initial chain slot,
- parameter IDs and smoothing,
- sample-rate prepare/reset handling,
- tests for neutral pass-through and parameter smoothing.

Non-goals:

- no isolator replacement yet,
- no delay/reverb/phaser/flanger/filter implementation,
- no plugin host.

Acceptance:

- a no-op or test-only node can be hosted without changing audio output,
- realtime safety constraints are documented and covered by tests/design review.

Ordering:

- directly before EQ replacement.

### Phase 7: Replace Current EQ With 3-Band DJ Isolator

Goal: replace the current hardwired EQ with a professional internal Rust DSP node.

Scope:

- normalized `0.0..1.0` internal controls with `0.5` neutral,
- LOW below 250 Hz, MID 250 Hz to 4 kHz, HIGH above 4 kHz as initial defaults,
- nonlinear musical cut curve and smooth limited boost,
- proper crossover/band-splitting strategy,
- smoothed parameter updates,
- UI mapping and middle-click reset preservation.

Non-goals:

- no external plugin host,
- no other FX.

Tests/checks:

- Rust DSP tests for neutral transparency, kill behavior, bounded boost, smoothing, NaN/denormal
  handling, sample-rate changes,
- Python UI/control mapping tests,
- full validation.

Acceptance:

- current EQ is replaced, not patched, and lives in the DSP foundation.

Ordering:

- after Phases 2 through 6.

### Phase 8: Future FX Modules

Goal: add performance FX incrementally after the DSP foundation and isolator are stable.

Scope examples:

- delay,
- filters,
- phaser/flanger,
- reverb,
- master or deck/group processing.

Non-goals:

- no plugin host unless a much later explicit product decision changes the architecture.

Acceptance:

- each FX module is its own OpenSpec-friendly change with tests and realtime-safety review.

## Next Recommended Step

The next recommended step is Analysis Stage 2 / Phase 2:

```text
Audit and prepare the Rust audio callback realtime-safety boundary:
bounded command drain, oversized callback blocks, and buffer/stem handle retirement.
```

Do not implement the new EQ or any other DSP effect before that step is complete, unless a future
user request explicitly supersedes this plan with an OpenSpec-backed change.
