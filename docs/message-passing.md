Here is a concrete breakdown of how the message passing looks from both sides.

The key to high performance is that **Python never sees the ring buffer**. Python sees an `AudioEngine` object with methods. The Rust implementation of those methods converts Python arguments into a small Rust enum (`ControlMessage`) and pushes it into a lock-free SPSC queue (`rtrb`).

## Audio Buffer Flow

With the introduction of the AudioEngine, audio processing follows this flow:

1. Python instantiates `AudioEngine` via PyO3 FFI
2. `AudioEngine.run()` creates a CPAL output stream and ring buffers
3. The CPAL audio callback runs on a real-time thread, independent of the Python GIL
4. On each callback, the engine:
   - drains pending `ControlMessage`s (e.g., `LoadSample`, `PlaySample`)
   - mixes active sample voices into the output buffer
5. CPAL streams the output buffer to the system audio device

## The Shared Protocol (Rust)

The wire types live in `rust/src/messages.rs`.

```rust
use std::sync::Arc;

pub(crate) struct SampleBuffer {
    pub channels: usize,
    pub samples: Arc<[f32]>,
}

pub enum ControlMessage {
    Ping,
    Stop,
    LoadSample { id: usize, sample: SampleBuffer },
    PlaySample { id: usize, velocity: f32 },
}

#[pyclass]
pub enum AudioMessage {
    Pong,
    Stopped,
}
```

Notes:
- `SampleBuffer` is immutable and shared via `Arc`, so publishing a loaded sample to the audio thread is a cheap handle copy.
- Valid `id` range is `0..32` (i.e., `id < 32`).
- `velocity` is clamped/validated in Python-side methods; missing samples are ignored safely by the audio thread.

## Python Usage

From Python, loading is synchronous (it may block the Python thread), but playback triggering is fast because it only pushes a small message.

```python
import time

from flitzis_looper_rs import AudioEngine

engine = AudioEngine()
engine.run()

engine.load_sample(0, "kick.wav")

while True:
    engine.play_sample(0, 1.0)
    time.sleep(0.5)
```

## Real-Time Safety

- `AudioEngine.load_sample(id, path)` performs disk I/O and decode using `symphonia` outside the audio callback.
- The CPAL callback avoids disk I/O, heap allocations when triggering playback, and logging.
- Polyphony and sample slots are fixed (`MAX_SAMPLE_SLOTS = 32`, `MAX_VOICES = 32`) for predictable performance.
