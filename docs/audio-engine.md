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
5. The CPAL callback drains pending control messages, routes play/stop commands through the Rust scheduler, mixes active voices into the output buffer, and advances the Rust transport timeline by rendered output frames.
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

The planned direction is:

- Rust owns the global transport timeline and advances it by rendered output sample frames.
- Scheduled playback events target absolute output-frame positions.
- Rust stores master BPM and derives beat/bar phase from the audio-thread sample-frame clock.
- Quantized pad triggers use a fixed-capacity scheduler owned by the audio thread.
- Existing immediate trigger behavior remains the default when trigger quantization is disabled.
- Beatgrid and downbeat metadata is prepared outside the audio callback, then published as bounded timing metadata for Rust playback timing.

The audio callback must remain real-time safe for this work. It must not perform disk I/O,
Python/GIL access, blocking waits or locks, logging, heavy allocations, neural network
inference, real-time stem separation, or long-running work.

Later Gen3 stem work must be offline/cache-based. Stem generation is only allowed for pads
that are not currently playing; the audio callback may only mix already prepared audio data.

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

- Messaging utilities
  - `ping()` sends a ping to the audio thread.
  - `receive_msg()` polls for an `AudioMessage` from the audio thread and returns `None` when no message is available.

## Not implemented (yet)

- Audio device selection/configuration (the engine currently uses the default output device/config).
- Broader channel-layout support; currently decoding only supports mono↔stereo mapping.
- Quantized pad trigger routing through the scheduler.
- Phase-aware beat/bar/downbeat playback alignment.
- Real-time stem separation is intentionally out of scope.

## Related specs

- `openspec/specs/minimal-audio-engine/spec.md`
- `openspec/specs/ring-buffer-messaging/spec.md`
- `openspec/specs/load-audio-files/spec.md`
- `openspec/specs/play-samples/spec.md`
- `openspec/changes/add-rust-transport-timeline/`
