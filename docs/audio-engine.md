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
5. The CPAL callback drains pending control messages and mixes active voices into the output buffer.
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
- BPM detection
- Time-stretch/pitch-shift

## Related specs

- `openspec/specs/minimal-audio-engine/spec.md`
- `openspec/specs/ring-buffer-messaging/spec.md`
- `openspec/specs/load-audio-files/spec.md`
- `openspec/specs/play-samples/spec.md`
