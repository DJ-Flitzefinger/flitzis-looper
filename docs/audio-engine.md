# Audio Engine

Flitzis Looper uses a Rust-based audio engine to meet real-time constraints while keeping the Python side focused on orchestration.
Python interacts with a single `AudioEngine` object; the CPAL audio callback renders audio without acquiring the Python GIL.

```text
+---------------------+
|   Flitzis Looper    |
|      (Python)       |
+----------+----------+
           |
           v
+---------------------+
|    AudioEngine      |
| (Rust, cpal, PyO3)  |
+----------+----------+
           |
           v
+---------------------+
|  System Audio API   |
| (ALSA/PulseAudio)   |
+---------------------+
```

## High-level flow

1. Python creates an `AudioEngine` instance and calls `run()`.
2. `run()` initializes a CPAL output stream and two ring buffers (control→audio, audio→control).
3. Python schedules audio loading into sample slots using `load_sample_async(id, path)` and polls `poll_loader_events()`.
4. Python triggers playback using `play_sample(id, velocity)`.
5. The CPAL callback drains pending control messages, updates bounded per-pad timing metadata, routes play/stop commands through the Rust scheduler, mixes active voices into the output buffer, and advances the Rust transport timeline by rendered output frames.
6. Optional: Python can poll `receive_msg()` for messages emitted by the audio thread (e.g., `Pong`).

## Module Structure

The audio engine is organized into modular components following the single responsibility principle:

```
audio_engine/
├── mod.rs              # Main orchestration, re-exports, public API
├── constants.rs        # Configuration constants (NUM_BANKS, GRID_SIZE, etc.)
├── errors.rs           # Error types (SampleLoadError)
├── voice.rs            # Voice struct and lifecycle management
├── mixer.rs            # RtMixer implementation with real-time rendering
├── sample_loader.rs    # Audio file decoding and channel mapping
└── audio_stream.rs     # CPAL stream management and callback setup
```

Each module is `pub(crate)` with only `mod.rs` exposing the public API, ensuring clear encapsulation and reducing coupling between components.

The Gen3 transport helper lives in `rust/src/audio_engine/transport.rs`; the fixed-capacity
absolute-frame scheduler helper lives in `rust/src/audio_engine/scheduler.rs` and is owned by the
CPAL callback.

## Main components

- Python (control layer)
  - Owns the `AudioEngine` instance and calls its methods.
  - Schedules potentially blocking work (disk I/O, decoding) via the Rust engine.

- Rust (real-time audio layer)
  - `rust/src/audio_engine/mod.rs`: Main orchestration and Python-facing API.
  - `rust/src/audio_engine/constants.rs`: Configuration constants and limits.
  - `rust/src/audio_engine/errors.rs`: Audio-specific error types.
  - `rust/src/audio_engine/voice.rs`: Voice management and lifecycle.
  - `rust/src/audio_engine/mixer.rs`: Real-time mixer implementation.
  - `rust/src/audio_engine/transport.rs`: Audio-thread-owned output-frame clock and musical phase helpers.
  - `rust/src/audio_engine/scheduler.rs`: Fixed-capacity absolute output-frame scheduler for playback events.
  - `rust/src/audio_engine/sample_loader.rs`: Audio file decoding and channel mapping.
  - `rust/src/audio_engine/audio_stream.rs`: CPAL stream management and callback.
  - `rust/src/messages.rs`: Fixed-size message types shared between threads.
  - Dependencies:
    - `cpal` for the audio callback/stream.
    - `rtrb` for SPSC ring buffers.
    - `symphonia` for decoding audio files.

## Threading and real-time constraints

The project deliberately separates non-real-time work from the real-time audio callback:

- The audio callback MUST avoid blocking, disk I/O, heap allocations, logging, and Python/GIL interaction.
- Inter-thread communication uses fixed-capacity ring buffers (1024 messages).
- Sample playback is designed for predictable performance:
  - `MAX_SAMPLE_SLOTS` fixed sample slots addressed by `id` (`0..n`).
  - `MAX_VOICES` fixed polyphony; additional triggers are dropped deterministically.
- Sample decoding happens outside the callback. Decoded sample data is published to the audio thread via a shared handle (no full-buffer copies just for cross-thread transfer).

## Gen3 transport timeline plan

The first Gen3 behavior change is specified in
`openspec/changes/add-rust-transport-timeline/`. The initial transport timeline module now
exists and is owned by the audio callback. The fixed-capacity scheduler module now exists
with deterministic unit tests, and the audio callback owns a scheduler for current-frame
playback integration.

Implemented first slice:

- `TransportTimeline` stores the absolute output-frame clock and output sample rate.
- The CPAL callback advances the transport by the number of rendered output frames.
- Rust validates optional master BPM and derives beat/bar phase for 4/4 timing.
- Deterministic Rust unit tests cover frame advancement, BPM conversion, phase, grid
  boundary calculations, and invalid BPM fallback.
- `FixedCapacityScheduler` stores accepted events in bounded array storage, orders by
  absolute output frame, preserves same-frame insertion order, drains late events at the
  current callback start frame, and rejects new events when full without eviction.
- Scheduler tests cover named capacity, target-frame ordering, same-frame stable ordering,
  late events, in-buffer target execution frames, and full-capacity rejection.
- The CPAL callback owns `TransportScheduler` and routes `PlaySample`, `StopSample`, and
  `StopAll` through current-frame scheduled commands while preserving the public immediate
  `play_sample` behavior.
- Scheduled events inside an output buffer split rendering at the target frame so starts and
  stops occur at the intended sample-frame offset.
- `AudioEngine.set_trigger_quantization("immediate" | "next_beat" | "next_bar")` publishes a
  fixed-size trigger-quantization mode update to the audio thread. The default remains
  immediate.
- `AudioEngine.set_pad_timing_metadata(id, phase_anchor_s)` publishes bounded per-pad
  beatgrid/downbeat timing metadata prepared outside the audio callback. The Python controller
  derives `phase_anchor_s` using the existing loop-region fallback order: first finite
  non-negative downbeat, then first finite non-negative beat, then `0.0`.
- When trigger quantization is set to next beat or next bar and master BPM is available, Rust
  computes the target frame from `TransportTimeline` and schedules `PlaySample` at that absolute
  output frame. If master BPM is unavailable, the request falls back to immediate playback.
- Scheduler-full quantized play requests are rejected without evicting existing scheduled events
  or changing currently playing pads.
- `AudioEngine.play_sample_exclusive(id, velocity)` publishes one fixed-size command for
  one-at-a-time playback. With quantization enabled, Rust schedules the stop-all operation and
  requested pad start as one atomic `StopAllThenPlaySample` event at the same absolute output
  frame; scheduler-full rejection leaves current playback unchanged.
- The first playback slice for `add-phase-aware-playback-sync` is in place:
  `TransportTimeline` can compute bar phase for arbitrary scheduled target frames without
  advancing the transport clock, and `RtMixer` can compute a phase-aligned initial sample frame
  from pad BPM, a bounded phase anchor, active loop bounds, and target bar phase.
- Quantized scheduled `PlaySample` and `PlaySampleExclusive` events now carry an optional
  fixed-size target bar phase computed from the scheduled output frame. When the descriptor is
  present, Rust starts or retriggers the pad at the phase-aligned sample frame; missing pad
  metadata falls back to the existing effective loop start.
- Immediate playback commands carry no phase descriptor, so `play_sample` and
  `play_sample_exclusive` keep the existing prompt loop-start behavior when trigger quantization
  is disabled.
- `AudioEngine.anchor_transport_phase_from_pad(id)` publishes a fixed-size BPM-lock phase-anchor
  request. When the selected pad is active and has valid BPM/timing metadata, the audio thread
  derives the pad's current bar phase from mixer state and moves the Rust transport downbeat anchor
  to the matching phase. If the pad is inactive, paused, missing BPM/timing metadata, or master BPM
  is unavailable, the transport downbeat is left unchanged and existing BPM-ratio tempo matching
  continues.

The planned direction is:

- Rust owns the global transport timeline and advances it by rendered output sample frames.
- Scheduled playback events target absolute output-frame positions.
- Rust stores master BPM and derives beat/bar phase from the audio-thread sample-frame clock.
- Quantized pad triggers use a fixed-capacity scheduler owned by the audio thread.
- Existing immediate trigger behavior remains the default when trigger quantization is disabled.
- Beatgrid and downbeat metadata is prepared outside the audio callback, then published as
  bounded per-pad timing metadata for Rust playback timing.

The audio callback must remain real-time safe for this work. It must not perform disk I/O,
Python/GIL access, blocking waits or locks, logging, heavy allocations, neural network
inference, real-time stem separation, or long-running work.

Later Gen3 stem work must be offline/cache-based. Stem generation is only allowed for pads
that are not currently playing; the audio callback may only mix already prepared audio data.

The active Gen3 stem planning slice is `openspec/changes/add-offline-stem-cache/`. It defines
offline/background stem generation, project-local stem cache identity, inactive-pad gating,
fixed-size publication of prepared immutable stem buffers, and future real-time-safe stem mixing
using the same voice playhead and loop timing as full-mix playback. It does not implement neural
inference, stem UI, or mixer stem toggles.

The performer-facing stem control planning slice is
`openspec/changes/add-stem-performance-controls/`. It defines stem availability indicators,
selected-pad generation entry points, explicit full-mix versus all-stems mode selection, future
per-stem mute/solo/toggle controls, persistence expectations, and fixed-size audio-thread stem mix
control messages. This planning slice does not implement UI controls, production source
separation, neural model integration, or new mixer control behavior.

The current stem implementation defines Python-side project metadata for a
project-local `samples/stems/<source-version-hash>/` cache layout, using a source-version token
derived from the cached source path plus file metadata. The controller rejects stem generation
requests for pads that are playing, loading, analyzing, already generating stems, missing a loaded
source, or missing the cached source file. The low-level Rust API exposes a gated
`generate_stems_async(id, source_version, cache_dir)` background task that reports loader-style
progress and writes deterministic project-local WAV cache artifacts outside the audio callback.
This initial cache writer is non-neural: `instrumental.wav` contains the aligned full mix, while
`vocals.wav`, `melody.wav`, `bass.wav`, and `drums.wav` are aligned silence placeholders. It proves
the cache layout, background I/O, and availability validation without implementing production
source separation. `AudioEngine.publish_prepared_stems(id, source_version, cache_dir)` now
validates those cached WAV artifacts against the currently loaded full-mix buffer outside the audio
callback, then publishes shared immutable prepared-stem handles to Rust through a fixed-size control
message. The audio callback accepts the handles only for loaded inactive pads and stores them in
bounded per-pad/per-stem state. The first performer-control implementation slice adds a durable
per-pad `full_mix`/`all_stems` preference with `full_mix` as the default for new and older
projects. Rust stores that preference as bounded audio-thread state updated by fixed-size control
messages, and prepared stems are used only when `all_stems` is selected and the accepted prepared
set matches the requested source-version hash. Missing, stale, incomplete, rejected, or disabled
stems fall back to the loaded full-mix buffer. Performer-facing stem indicators, Generate Stems
button wiring, per-stem mute/solo/toggle controls, and production source separation are still
intentionally absent.

The active Gen3 phase-aware sync slice is `openspec/changes/add-phase-aware-playback-sync/`. It
defines how quantized starts will use the Rust transport phase plus bounded per-pad timing anchors
to choose the initial pad sample frame, and how BPM lock can anchor the transport downbeat from a
selected playing pad. Phase-aware scheduled playback is wired for quantized starts and exclusive
transitions; BPM-lock phase anchoring is wired for selected active pads.

## Current Python API surface

The Rust engine is exposed to Python as `AudioEngine` with:

- Lifecycle
  - `AudioEngine()`
  - `run()`
  - `shut_down()`

- Sample workflow
  - `load_sample_async(id, path)` schedules loading on a Rust background thread.
  - `poll_loader_events()` polls for background loader events (e.g. started/success/error).
  - `play_sample(id, velocity)` triggers playback (`velocity` in `0.0..=1.0`).
  - `play_sample_exclusive(id, velocity)` stops all active voices and starts the requested loaded
    sample as one audio-thread command. The controller uses this for MultiLoop-disabled playback.
  - `set_trigger_quantization(mode)` sets low-level Rust trigger quantization mode. Supported
    modes are `"immediate"`, `"next_beat"`, and `"next_bar"`. The Python controller persists the
    selected global mode and the performance UI exposes those three supported modes.
  - `set_pad_timing_metadata(id, phase_anchor_s)` publishes a finite non-negative per-pad phase
    anchor derived from analysis metadata. It is stored in Rust state for phase-aware quantized
    playback; full beat-grid vectors are not sent to the callback.
  - `anchor_transport_phase_from_pad(id)` requests BPM-lock transport downbeat anchoring from the
    selected playing pad using only audio-thread-owned transport and mixer state.

- Messaging utilities
  - `ping()` sends a ping to the audio thread.
  - `receive_msg()` polls for an `AudioMessage` from the audio thread and returns `None` when no message is available.

## Not implemented (yet)

- Audio device selection/configuration (the engine currently uses the default output device/config).
- Broader channel-layout support; currently decoding only supports mono↔stereo mapping.
- Real-time stem separation is intentionally out of scope.
- Offline stem cache identity, request gating, and deterministic cache artifact writing are
  implemented. Prepared stem-buffer validation/publication and prepared-stem rendering fallback
  infrastructure are implemented. Durable full-mix/all-stems mode plumbing is implemented, but
  production source separation, performer-facing stem indicators, Generate Stems button wiring, and
  per-stem mute/solo/toggle controls are planned in
  `openspec/changes/add-stem-performance-controls/` and not implemented.

## Related specs

- `openspec/specs/minimal-audio-engine/spec.md`
- `openspec/specs/ring-buffer-messaging/spec.md`
- `openspec/specs/load-audio-files/spec.md`
- `openspec/specs/play-samples/spec.md`
- `openspec/changes/add-rust-transport-timeline/`
- `openspec/changes/add-phase-aware-playback-sync/`
- `openspec/changes/add-offline-stem-cache/`
