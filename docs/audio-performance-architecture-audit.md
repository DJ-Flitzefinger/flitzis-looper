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

This plan is not a freeze on Python-to-Rust migration. Targeted migrations are allowed and
preferred when they reduce duplicated live-audio authority, improve timing correctness, lower
latency, or make the realtime boundary safer. The constraint is scope and realtime safety: each
migration should move one bounded responsibility at a time, keep Python responsible for UI,
persistence, and offline orchestration, and keep allocation, blocking work, file I/O, Python/GIL
access, logging, plugin loading, and heavy processing out of the callback and hot path.

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
- `docs/audio-state-ownership.md`
- `docs/audio-loop-source-stem-alignment.md`
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
- `openspec/changes/clarify-loop-source-stem-alignment/`

Local continuation memory lives outside the repository in `../codex-meta/`.

## Current Architecture Summary

The current runtime has a useful split:

```text
Python UI/controller/persistence/offline prep
-> PyO3 AudioEngine methods
-> bounded Rust command queue + bounded Rust parameter queue
-> CPAL callback-owned scheduler, transport, mixer
-> system audio output
```

Audio-to-control telemetry is also sent through a bounded ring buffer and polled by Python.
MIDI is now captured in Rust outside the audio callback and enters either the direct bounded audio
command path for simple playback actions or the Python controller path for controller-owned
actions. This is the right direction.

The current implementation is, however, a feature-accumulated engine rather than a complete
professional DSP/FX architecture. Several preparation concepts have been cleaned up, and several
still need cleanup before DSP/FX work:

- accepted performance master BPM is now bridged to transport-grid timing and mixer BPM-lock
  tempo matching,
- durable Python state and live Rust state duplicate several authorities,
- command draining is now bounded per callback and fast scalar parameters now use a separate
  bounded queue with callback-side coalescing,
- buffer and prepared-stem handle retirement now leaves the callback through a bounded
  non-audio retirement worker,
- durable Python intent, transient Python session projections, and live Rust audio state are now
  documented in `docs/audio-state-ownership.md`,
- audio-to-control telemetry dispatch is controller-owned rather than UI-context-owned,
- current EQ is hardwired into the mixer rather than modeled as a DSP node,
- future parameter smoothing is not yet defined as a first-class DSP system.

## Rust Audio Engine And Hot Paths

The CPAL audio callback is created in `rust/src/audio_engine/audio_stream.rs`. It owns:

- the ordered command control consumer ring,
- the fast parameter control consumer ring,
- the audio-to-control producer ring,
- `RtMixer`,
- `TransportTimeline`,
- `TransportScheduler`,
- trigger quantization state,
- status cadence counters.

The callback currently:

1. drains at most `MAX_CONTROL_MESSAGES_PER_CALLBACK` ordered command messages per invocation,
   currently `64`, and leaves additional command messages queued for later callbacks,
2. drains at most `MAX_PARAMETER_MESSAGES_PER_CALLBACK` parameter messages per invocation,
   currently `64`, and coalesces repeated drained updates by parameter identity,
3. updates mixer/transport/scheduler state from those messages,
4. renders scheduled audio segments into the CPAL output buffer,
5. advances the transport by rendered output frames,
6. emits bounded status messages such as `SampleStarted`, `SampleStopped`, `PadPeak`, and
   `PadPlayhead`.

Positive findings:

- The callback does not acquire the Python GIL.
- The callback does not decode audio, read JSON, scan files, run Demucs, or do network work.
- MIDI ports and Learn state are outside the callback.
- Core mixer arrays and scheduler storage are fixed-capacity.
- Per-voice stretch buffers are constructed before callback rendering.
- Stem generation and cache validation are outside the callback.
- Oversized render slices are split into chunks that fit the existing preallocated stretch-buffer
  assumptions.
- Sample and prepared-stem handles removed by the callback are moved to a bounded non-audio
  retirement worker before large `Arc` deallocation can occur.

Risks and gaps:

- The callback work budget is now split between ordered commands and coalesced scalar parameters.
  Future DSP parameters still need audio-side smoothing before they are applied to sample
  processing.
- The scheduler is fixed-capacity but insertion/drain operations shift arrays. This is bounded,
  but burst behavior should be measured and budgeted.
- `render_scheduled_audio` and `RtMixer::render` both clear output ranges. This is minor, but it
  is unnecessary hot-path work.
- The buffer-retirement path is fixed-size and preallocated. Sustained producer overload can defer
  handle-retiring control messages at the queue head until retirement capacity is available.
- Audio-to-control messages are best-effort. Dropped `SampleStarted` or `SampleStopped` telemetry
  can desynchronize Python `SessionState` from live Rust state.
- Loop, stem-mask, EQ, and several parameter changes are immediate and unsmoothed, so clicks or
  discontinuities are possible during performance.

## Ringbus, Command, And Parameter Path

The Stage 3 command/parameter preparation is recorded in
`openspec/changes/prepare-command-parameter-path/`. The current bridge now has two bounded
control-to-audio paths:

- discrete commands: play, stop, stop all, exclusive play, pause/resume, unload,
- publication commands: loaded full-mix sample buffers, prepared stem buffers,
- mode updates: stem mode, stem mask, trigger quantization, BPM lock, Key Lock,
- continuous or frequently updated parameters: volume, speed, pad gain, pad EQ, master BPM,
  pad BPM.

Loop-region updates remain ordered commands in this slice because existing playback paths rely on
loop state being applied before a paired trigger.

The message types are explicit and typed. Event semantics and fast parameter semantics are now
separated at the queue boundary:

```text
Discrete events:
  trigger, stop, exclusive transition, unload, publish prepared data.

Continuous parameters:
  speed, pitch, gain, EQ/isolator, FX wet/dry, filter cutoff, feedback, stem level.
```

Resolved in Stage 3:

- Fast scalar parameters use a separate bounded parameter queue, so parameter bursts do not occupy
  ordered command slots needed by trigger and stop commands.
- Drained parameter messages are coalesced by parameter identity during one callback invocation,
  and only the latest drained value is applied for each identity.
- Direct Rust MIDI trigger dispatch checks command queue capacity for the whole loop-region plus
  play sequence and sends none of it if the whole transaction cannot fit.

Remaining risks:

- Parameter ring-full behavior remains best-effort for high-rate controls; the newest update may
  be dropped when the parameter queue is full.
- Fast future DSP parameters such as filter cutoff or isolator bands still need audio-side
  smoothing after the coalesced target value reaches Rust DSP state.

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

Stage 5 result:

- `ControlParameterMessage::SetMasterBpm` now updates both mixer BPM-lock state and transport
  master BPM. The transport preserves its current bar phase at the callback's current output frame,
  so tempo changes do not reset the output-frame clock or implicitly sync to a pad.

Further gaps:

- Future clock work still needs stress coverage around rapid mixed command/parameter bursts.
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
- a shared accepted Rust performance master BPM applied to both `RtMixer.master_bpm` for BPM-lock
  tempo ratio and `TransportTimeline.master_bpm` for quantization grid timing.

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
- Simultaneous pitch changes, quantized starts, loop edits, and stem toggles need a
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

- Python session still projects the BPM-lock anchor and displayed master BPM, while Rust owns the
  accepted live tempo used for audio.
- There is no complete per-deck or group state model.
- Pad/deck/global state transitions under simultaneous pitch, keylock, loop edits, stem toggles,
  and quantized starts need focused tests before adding future DSP chains.

## Python/Rust Boundary And Persistence

The Stage 4 ownership cleanup is recorded in `docs/audio-state-ownership.md` and
`openspec/changes/clarify-state-ownership-boundary/`.

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

## Targeted Python-To-Rust Migration Policy

The planning should not discourage carefully scoped Rust ownership transfers. When a function
currently represented in Python participates in live audio timing, scheduler decisions, playback
rate, loop/source-frame conversion, parameter application, or low-jitter control dispatch, moving
that responsibility toward Rust is a valid architecture improvement if it is the smallest safe
step.

Migration rules:

- Move one authority boundary at a time; avoid big-bang rewrites.
- Update or create an OpenSpec change before or alongside behavior changes.
- Keep Python as the owner of durable performer intent, project persistence, UI state, settings,
  mapping edit UX, and offline/background preparation unless a focused design says otherwise.
- Keep Rust as the owner of live audio truth, using typed fixed-size commands, parameter updates,
  and telemetry across the Python/Rust boundary.
- Any new Rust state used by the callback must be bounded, prevalidated, and realtime safe.
- Heavy work needed by the migration must run outside the callback or behind a bounded worker/ring
  boundary.
- Add focused Rust and/or Python tests for the transferred authority and run official OpenSpec
  validation for affected active changes.

Stage 5 used a small OpenSpec-backed Rust-side bridge to unify transport grid BPM and BPM-lock
tempo matching. Accepted master-BPM parameter updates are now the shared Rust live tempo, and
pad-derived phase anchoring remains explicit. This stage did not implement new EQ, DSP effects, or
plugin hosting.

Current problems:

- Stage 5 resolved the duplicated transport-grid versus BPM-lock master BPM authority. Remaining
  clock work should focus on mixed state-transition stress cases rather than choosing another
  master-BPM owner.
- Telemetry drops can leave Python active/paused state stale until later telemetry or an explicit
  controller action reconciles the projection.
- Several setter methods cross the bridge as best-effort without acknowledgement or coalescing.
- Persistence is correctly outside realtime operation, but state restoration sends many individual
  messages through the same queue used for live commands.

Resolved in Stage 4:

- `ProjectState` is documented as durable performer intent.
- `SessionState` is documented as a transient, rebuildable control/UI projection.
- Rust audio-thread state is documented as live truth for active voices, source playheads,
  transport, scheduler, loaded buffers, prepared stems, and future smoothed DSP state.
- UI rendering may request runtime polling, but audio telemetry type dispatch now lives in
  `AppController.poll_runtime_events()`.

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

Stage 7 result:

- `docs/input-mapping-dsp-parameter-policy.md` records the mapping policy for future DSP
  parameters without changing current MIDI behavior.
- Keyboard and MIDI Note mappings may keep bounded set-value semantics for explicit targets.
- MIDI CC and NRPN mappings should produce stable relative-step action keys and bounded
  controller-owned target changes before any accepted target reaches Rust.
- Accepted continuous DSP targets should use the bounded parameter path, callback-side coalescing,
  and Rust-owned smoothing before sample processing.
- Direct Rust dispatch remains limited to discrete audio-safe command transactions; future DSP
  mappings must not carry plugin handles, callback-local pointers, Python objects, file paths, or
  unbounded metadata.

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

1. Python session state can desynchronize if audio-to-control telemetry is dropped.
2. Loop, stem, and EQ changes are immediate and can click.
3. Future DSP parameters still need audio-side smoothing after the Stage 3 coalesced parameter
   bridge.
4. Current EQ is not a durable DSP-chain foundation.
5. There is no per-pad/per-stem/deck/master DSP node architecture.

Resolved preparation blockers:

- callback command drain is bounded,
- oversized render blocks are split to preserve preallocated stretch-buffer bounds,
- large sample/stem handle retirement leaves the callback through a bounded worker,
- fast scalar parameter updates no longer share the ordered command queue,
- direct Rust MIDI loop-region plus trigger dispatch is all-or-nothing,
- accepted master-BPM parameter updates bridge transport-grid timing and BPM-lock tempo matching
  while preserving transport bar phase,
- source-frame position, output-frame time, loop-region interpretation, prepared-stem alignment,
  and the click-free transition follow-up plan are documented and covered by focused mixer tests.

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

Status: completed by `openspec/changes/prepare-realtime-callback-safety/`.

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

Result:

- OpenSpec change `prepare-realtime-callback-safety` records the bounded callback drain,
  oversized block handling, and deferred buffer-retirement behavior.
- The callback processes at most `64` control messages per invocation.
- Oversized mixer render slices are split to preserve the existing fixed stretch-buffer bounds.
- Sample and prepared-stem handles removed or rejected in the callback are retired through a
  bounded non-audio worker before large deallocation can occur.
- Focused Rust tests cover command-burst budgeting, oversized render chunking, scheduled event
  offsets, retirement queue behavior, and mixer unload/reject paths.

### Analysis Stage 3: Command And Parameter Architecture

Status: completed by `openspec/changes/prepare-command-parameter-path/`.

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

Output:

- ordered command path plus separate bounded continuous parameter path,
- callback-side last-value-wins coalescing for drained scalar parameters,
- all-or-nothing direct Rust MIDI dispatch for existing loop-region plus trigger sequences.

### Analysis Stage 4: Python State, Rust State, And Persistence Boundary

Status: completed by `openspec/changes/clarify-state-ownership-boundary/`.

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

Result:

- `docs/audio-state-ownership.md` defines durable Python intent, transient Python projections,
  live Rust audio state, restore ordering, and telemetry reconciliation expectations.
- Audio runtime telemetry dispatch moved from the UI context to controller-owned runtime polling.

### Analysis Stage 5: Clock, BPM, Pitch, And Scheduler

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
- if documentation alone is not enough, one small OpenSpec-backed Rust ownership migration or
  bridge that removes the duplicated transport-grid versus BPM-lock authority without broad
  rewrites.

### Analysis Stage 6: Loop, Source Position, And Stems

Status: completed by `docs/audio-loop-source-stem-alignment.md` and
`openspec/changes/clarify-loop-source-stem-alignment/`.

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

Result:

- output-frame transport/scheduler time and per-voice source-frame playback position are named as
  separate runtime concepts,
- loop regions are documented as Rust-owned half-open source-frame ranges after Python publishes
  durable editable seconds,
- live loop edits preserve an in-range source playhead and clamp an out-of-range playhead to the
  new loop start,
- prepared stems are documented as sharing full-mix source-frame origin, frame count, sample rate,
  channel layout, and source-version identity,
- stem mode/mask changes preserve the voice playhead and are sequenced before future per-stem DSP,
- click-free transition work is deferred to a bounded Rust transition helper before DSP foundation
  and EQ replacement.

### Analysis Stage 7: MIDI/Keyboard Under Future DSP Parameters

Status: completed by `docs/input-mapping-dsp-parameter-policy.md` and the Stage 7
`add-low-jitter-input-mapping` clarification.

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

Result:

- future DSP parameter mappings are documented as stable action keys plus bounded controller-owned
  target derivation,
- accepted continuous targets are documented to use the bounded parameter path and Rust-side
  smoothing,
- direct Rust MIDI dispatch remains command-only for existing discrete audio-safe transactions,
- stale runtime snapshots are documented as inappropriate for future live DSP truth.

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

Status: completed by `prepare-realtime-callback-safety`.

Goal: remove known hot-path hazards before future DSP work.

Implemented scope:

- budget command draining per callback with `MAX_CONTROL_MESSAGES_PER_CALLBACK = 64`,
- split render blocks larger than the existing internal stretch-buffer capacity,
- defer sample/prepared-stem `Arc` retirement outside the callback through a bounded worker.

Affected files:

- `audio_stream.rs`, `mixer.rs`, `voice_slot.rs`, `buffer_retirement.rs`, focused Rust tests,
  docs/OpenSpec.

Non-goals:

- no new EQ,
- no new FX,
- no change to MIDI semantics.

Risks:

- changing callback scheduling can alter edge timing,
- deferred retirement needs careful ownership.

Tests/checks completed:

- Rust unit tests for oversized block handling, command-burst budget behavior, and buffer
  retirement policy,
- official strict OpenSpec validation,
- full uv-managed Rust/Python validation for the behavior change.

Acceptance result:

- no unbounded command drain remains in the callback,
- larger device blocks are rendered in safe chunks rather than indexing past stretch buffers,
- known sample/prepared-stem handle replacement and rejection paths defer large deallocation
  outside the callback.

Rollback:

- keep each safety change isolated so it can be reverted without touching feature behavior.

Ordering:

- must happen before DSP/EQ implementation.

### Phase 3: Command/Ringbus/Parameter Path Cleanup

Status: completed by `prepare-command-parameter-path`.

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

Acceptance result:

- fast scalar parameters have a documented and tested bounded path,
- parameter bursts cannot occupy ordered trigger/stop command queue capacity,
- future DSP parameters have a documented coalesced path and still need DSP-side smoothing.

Ordering:

- before DSP foundation and EQ replacement.

### Phase 4: State Ownership Cleanup

Status: completed by `clarify-state-ownership-boundary`.

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

- durable Python intent versus live Rust audio truth is documented, and telemetry dispatch is
  controller-owned.

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

Stage 7 has completed the MIDI/keyboard future DSP-parameter mapping clarification.

The next recommended step is click-free transition preparation before DSP foundation:

```text
Prepare a bounded Rust transition helper for click-free stem mode/mask transitions, without
implementing a visible effect, EQ replacement, plugin hosting, or broad DSP foundation.
```

Do not implement the new EQ or any other DSP effect before click-free transition preparation and
the DSP foundation stages are complete, unless a future user request explicitly supersedes this
plan with an OpenSpec-backed change.
