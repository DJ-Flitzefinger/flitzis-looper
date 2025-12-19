# Design: Minimal AudioEngine with cpal

## Architecture
```
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

## Data Flow
1. Python instantiates AudioEngine via PyO3 FFI
2. AudioEngine creates cpal stream on initialization
3. Looper sends buffer requests via ring buffer (future extension)
4. AudioEngine fills buffer with silence (initially)
5. cpal streams buffer to system audio device
6. Engine maintains single output stream

## API

### Rust API
```rust
pub struct AudioEngine {
    stream: cpal::Stream,
}

impl AudioEngine {
    pub fn new() -> Result<Self, AudioError> { ... }
    pub fn play(&mut self) -> Result<(), AudioError> { ... }
    pub fn stop(&mut self) -> Result<(), AudioError> { ... }
}
```

### Python API
```python
from flitzis_looper_rs import AudioEngine

# Instantiate the audio engine from Python
engine = AudioEngine()

# Control playback
engine.play()
engine.stop()
```

## FFI Integration
- Use PyO3 `#[pyclass]` attribute to expose AudioEngine to Python
- Use PyO3 `#[pymethods]` to expose play/stop methods
- Audio thread runs independently of Python GIL
- Ring buffer communication planned for future message passing (not in this minimal implementation)

## Error Handling
- Use `thiserror` for typed errors
- Return `AudioError::DeviceNotFound` for missing audio devices
- Return `AudioError::StreamCreationFailed` for cpal failures
- Propagate errors through PyO3 to Python exceptions

## Performance
- Single output stream, no threading
- Buffer size: 512 samples (10ms @ 48kHz)
- No dynamic allocation in audio callback
- Audio thread never calls into Python

## Security
- Validate sample rate and channel count from FFI
- Avoid unsafe code in audio callback
- Audio thread never holds GIL

## Alternatives Considered
- PortAudio: Too heavy, more dependencies
- rodio: Doesn't support low-latency streaming well
- External audio server: Adds network latency

## Open Questions
- Should we support device selection?
- How to handle sample rate mismatches?
- What additional parameters should be exposed to Python?