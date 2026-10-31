# Architecture

Flitzis Looper is a desktop sample looper with six banks of 36 PADs. Python owns
the application and saved performer intent; Rust owns current audio sources,
sample-frame time and rendering. It supports scalar constant-tempo playback,
quantized launches, independent PAD controls, prepared stems and offline analysis.
The same source and ownership rules apply to GUI and MIDI.

This is the current system overview. Focused references explain the implementation
without reproducing its development chronology:

| Reference | Responsibility |
| --- | --- |
| [Runtime and transport](architecture/runtime-and-transport.md) | Callback, clock, scheduler, ordinary transport, rates, rendering and feedback. |
| [Media and PCM](architecture/media-and-pcm.md) | Material/content/source identity, import, complete disk PCM, finite residency, sharing and last readers. |
| [State, input and UI](architecture/state-input-and-ui.md) | Controllers, GUI/MIDI intent, persistence, arrangements, restore and shutdown. |
| [BPM analysis](architecture/bpm-analysis.md) | Productive Beat This, complete-source admission, constant-tempo selection, uncertainty and manual/Tap correction. |
| [Key analysis](architecture/key-analysis.md) | Complete-file CQT/KeyNet, independent outcomes, memory and source/correction epochs. |
| [Pitch and Key Lock](architecture/pitch-and-key-lock.md) | Piano/SET/Retrigger, nullable inheritance, Rubber Band preparation, actual source history and limits. |
| [Stems](architecture/stems.md) | Shared offline jobs, immutable disk selection, component residency, own live adoption and final readers. |

Current [OpenSpec contracts](../openspec/specs/) define supported behavior. Source
defines implementation facts; historical tests establish only their stated scope.
The [documentation audit](documentation-audit.md), [frozen provenance manifest](audit/source-disposition.json)
and [canonical evidence index](../openspec/changes/archive/2026-10-30-evidence-index.md)
retain original reports, failures and unpassed gates. Archived changes are
history, not an execution queue or proof of acceptance.

## Runtime split and source layout

| Owner | Entry points and responsibilities |
| --- | --- |
| Python application | [__main__.py](../src/flitzis_looper/__main__.py) calls [ui/run.py](../src/flitzis_looper/ui/run.py). [AppController](../src/flitzis_looper/controller/app.py) constructs controllers, loads intent, polls work and coordinates close. |
| Python state and controls | [models.py](../src/flitzis_looper/models.py), [controller/](../src/flitzis_looper/controller/), [input_mapping/](../src/flitzis_looper/input_mapping/) and [ui/](../src/flitzis_looper/ui/) own validated intent, persistence, input editing, dialogs, presentation and offline orchestration. |
| Native boundary | [flitzis_looper_audio](../src/flitzis_looper_audio/) wraps the PyO3 module exported by [lib.rs](../rust/crates/looper/src/lib.rs); [__init__.pyi](../src/flitzis_looper_audio/__init__.pyi) describes the Python API. |
| Rust control and workers | [audio_engine/mod.rs](../rust/crates/looper/src/audio_engine/mod.rs) coordinates non-realtime loading, analysis, preparation, publication and retirement. Rust outside the callback can perform bounded disk work and preparation. |
| Rust audio | [audio_stream.rs](../rust/crates/looper/src/audio_engine/audio_stream.rs), [transport.rs](../rust/crates/looper/src/audio_engine/transport.rs), [scheduler.rs](../rust/crates/looper/src/audio_engine/scheduler.rs) and [mixer.rs](../rust/crates/looper/src/audio_engine/mixer.rs) own due-frame effects, voices, sources and DSP. |
| Offline analysis | [analysis crate](../rust/crates/analysis/), [app_analysis.rs](../rust/crates/looper/src/audio_engine/app_analysis.rs), [app_tempo.rs](../rust/crates/looper/src/audio_engine/app_tempo.rs) and Python [analysis/](../src/flitzis_looper/analysis/) implement distinct productive and diagnostic paths outside realtime. |

Both Python packages are required. The compiled extension and native dependencies
are build products, not a second Python application.

## Control and signal flow

```text
GUI / captured MIDI or keyboard action / restored performer intent
  -> controller or guarded native input route
  -> validate current content + source + requested revision
  -> prepare immutable data/history off the audio thread when needed
  -> bounded command / parameter publication
  -> callback rechecks ownership and capacity
  -> absolute-frame scheduler / current mixer effect
  -> operation-specific acknowledgement + observations
  -> controller updates the matching projection and persistence intent

sealed source -> complete disk PCM -> admitted resident view / prepared readers
  -> fractional source trajectory + shared source read plan
  -> full mix or prepared stem selection
  -> rate / pitch / Key Lock processing
  -> smoothed PAD gain -> PAD DSP/isolator -> trigger velocity
  -> PAD metering and sum -> master volume / momentary mute
  -> master metering / recording capture -> CPAL output
```

Input admission and saved settings describe a request, not proof that audio
changed. Loading, timing refresh, ordinary pause, pitch, stem adoption and
arrangements each have their own current-source acceptance/observation path.
Meters and playheads are best-effort projections; their absence cannot authorize
replacing an owner or deleting media.

## Time, source and ownership invariants

- `TransportTimeline` advances in rendered output frames, including silence.
  `TransportScheduler` orders fixed-capacity absolute-frame events. UI polling
  observes work; it does not advance the musical clock.
- Scalar periods, rates and source progression retain binary64 precision.
  Physical PCM bounds are integer frames. A rounded endpoint does not redefine
  fractional musical duration. Quantization chooses launch time; it does not
  add source phase following.
- A PAD number is a slot. Content lifetime, placement, source generation,
  material identity and preparation request have different meanings. Sharing
  immutable PCM does not share mutable voice state or current acceptance.
- Publication is guarded again at effect time. Stale/cancelled work, exhausted
  capacity or failed preparation must not mutate a replacement source or certify
  success from enqueue alone.
- Saved assignments, workers, queued effects, active/paused voices and history
  readers can keep resources alive. Physical retirement waits for the last
  reader and checked identity; large ownership is released outside realtime.

The callback must not perform file or JSON I/O, Python/GIL access, UI calls,
logging, inference, plugin scanning/loading, blocking locks, heavy allocation,
unbounded loops or long-running work. New non-realtime Rust modules do not
change that boundary.

## Preparation, publication and retirement

Import captures immutable input and builds complete decoder/playback artifacts
on disk. Ordinary loading admits the needed finite PCM range rather than making
full-track RAM residency the default. Complete-source analysis/export and stream
producers borrow their own checked readers. Pitch/history and stems prepare
against the same source coordinates and current requested state.

The control owner retains the original request through preparation and enqueue.
The callback checks its source, lifetime, revisions, timing and available feedback/
retirement capacity before the effect. A matching acknowledgement settles that
operation; newer unrelated metadata does not erase a real reader. Conversely,
an old ticket or journal is no authority for a replacement source. See
[publication and failure handling](architecture/media-and-pcm.md#publication-cancellation-and-retirement).

## State, persistence and recovery

`ProjectState` stores performer intent and immutable material references;
`SessionState` is recreated and projects UI/runtime observations. Config writes
are revision-fenced, timing-verified and atomically replaced. Current Native
ownership, voices and ACKs are never restored by deserializing JSON.

Startup protects arrangement/recovery custody before ordinary restore, loads
the current source, and obtains fresh operation-specific acceptance. Copy, Pair
and Bank have separate journal/settlement paths. Close keeps polling until
recording, holds, deletion and arrangements permit teardown. See
[persistence and restore](architecture/state-input-and-ui.md#persistence-and-restore).

## Related subsystems and dependencies

The specialist modules describe current behavior; setup and diagnostic references
retain their narrower contracts. Historical reports are evidence and do not
override current source or activate old programs:

- [Constant-BPM controls](constant-bpm-guide.md),
  [accepted constant timing](accepted-constant-timing.md) and
  [scalar coordinates](scalar-source-coordinates.md): source-bound scalar timing.
  Productive automatic BPM uses configured BeatThis policy
  `neural-stable-regions-v4`; scientific QM/diagnostic interfaces remain separate
  from live authority.
- [Key analysis](architecture/key-analysis.md): automatic classification is independent of
  pitch and BPM admission. The complete-source file frontend replaces the old
  full-vector memory model. A model result is not musical ground truth.
- [Pitch and Key Lock](architecture/pitch-and-key-lock.md): prepared Rubber Band processing and
  timing limits. Global inheritance preserves explicit per-PAD overrides.
- [Stems](architecture/stems.md) and [model setup](stem-generation-setup.md):
  offline generation, saved selection, residency and effective Native adoption
  are separate states. Heavy generation now waits while playback is active;
  adoption has its own state-specific permits.
- [Development](development.md), [Rust guide](../rust/README.md) and
  [Portable Windows](portable-windows.md): toolchains, workers/models and
  distribution. Source and generic self-contained Portable installations do
  not require personal developer bindings.

Dear ImGui Bundle renders UI; PyO3 exposes native control; CPAL drives output;
native decoders/converters and Rubber Band support media/rendering. Offline
model environments are prepared separately from the callback. Runtime/model
availability checks do not establish successful inference or audio.

The opt-in standalone native BeatThis trial is separate from these productive
workers. Its original complete-source admission, pure Rust frontend and local
ORT sessions preserve the shared clock/source/OwnACK boundaries. A bounded real
CPU termination is evidenced; native CUDA/freshCPU, whole quality, complete
producer and cross-platform distribution remain open. Diagnostic exit receipts
cannot replace the original owned supervisor, actual reader settlement or
productive provenance. No Torch closure or installed runtime is removed.

The standalone dynamic experiment now admits explicit manifest/runtime files,
exact version/API and process OS/architecture before configuring ORT. Its checked
file handles outlive all CLI sessions/output work; Windows pins deny writes and
deletes, while other platforms still require an immutable owned directory.
This process-local bootstrap is deliberately outside the App/core session API.
It does not attest provider/CRT/driver images or a complete producer, and a
runtime-only probe does not establish model/device execution. CPU-only dynamic
loading is a separate opt-in feature. Remaining native parity and combined final
gates continue as STEP24 and STEP25 respectively. STEP23 adds an optional separate
SHA-authenticated declared ORT component list and retains every checked file handle
through the same last-reader boundary; loader/dependency closure remains UNKNOWN.
STEP22 ends the standalone
serialized-graph lifetime after the copying session loads and the original PCM
lifetime after full synchronous resampling. Saved source frames/duration and all
prepared/output/runtime pins remain; deallocation alone is not a memory-peak or
CUDA-success measurement.

## Limits and evidence

The productive optimization target is 2/4/6 simultaneous PADs. Synthetic
performance stress is limited to 10; more than 10 can cause performance problems
or glitches. Internal 32-voice capacity/ownership checks are not a device or
7–10-PAD performance guarantee. Disk integrity, memory admission, queues, worker
availability and supported finite reader domains can refuse an operation.

Current scope excludes realtime stem separation, plugin hosting, arbitrary
deck/group/master DSP chains, a new live loop-edit crossfade policy and a full
Rust port. Retired SYNC/source-map/phase-following designs are not live runtime
dependencies; the [documentation map](README.md) holds the recovery locator.
Device/loopback, sustained listening, musical-reference and clean-Windows gates
remain as limited in original evidence. A docs rewrite passes none of them.
The complete [original architecture](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/dbc80d3c117dcec50996feef2e8c59f1649388a5/docs/architecture.md)
preserves the superseded narrative; current navigation and actual section
dispositions are recorded separately in the audit.
