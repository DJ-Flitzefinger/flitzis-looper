# Audio Engine Module

This module contains the real-time audio engine for Flitzis Looper.

## Structure

The audio engine is organized into modular components following the single responsibility principle:

- **`mod.rs`** - Main orchestration and Python-facing API
- **`constants.rs`** - Configuration constants (NUM_BANKS, GRID_SIZE, MAX_VOICES, etc.)
- **`errors.rs`** - Error types (SampleLoadError and conversions)
- **`voice.rs`** - Voice management and lifecycle (sample_id, frame_pos, volume)
- **`mixer.rs`** - Real-time mixer (RtMixer) with load/unload/play/stop/rendering logic
- **`sample_loader.rs`** - Audio file decoding (decode_audio_file_to_sample_buffer, map_channels)
- **`audio_stream.rs`** - CPAL stream management (stream creation, callback setup, logger config)

## Design Principles

1. **Encapsulation**: All sub-modules are `pub(crate)` - only `mod.rs` exposes the public API
2. **Single Responsibility**: Each module has one clear purpose
3. **Testability**: Each module can be tested independently
4. **Real-time Safety**: Audio callback avoids blocking, allocations, and Python GIL

## Testing

Each module contains its own unit tests:
- Voice tests → `voice.rs`
- Mixer tests → `mixer.rs`
- Decoder tests → `sample_loader.rs`
- Integration tests → `mod.rs`

Run tests with: `cargo test --manifest-path rust/Cargo.toml`

## Python FFI

The audio engine is exposed to Python via PyO3:
- Public types are exported through `lib.rs`
- The `AudioEngine` class provides the main interface
- All methods maintain compatibility with Python expectations

## Architecture

```
Python (Control)
    ↓ ↑ (PyO3 bindings)
mod.rs (Orchestration)
    ↓ ↑ (module calls)
sub-modules (Implementation)
    ↓
CPAL (Audio Hardware)
```

## Future Extensions

When adding new features:
1. Place related code in the appropriate module
2. Add tests in the same module
3. Update this README if creating new modules
4. Ensure public API changes are documented

## Notes

- The original monolithic `audio_engine.rs` was refactored into this structure on 2025-12-22
- No public APIs were changed during the refactor
- Internal structure enables better maintainability and collaboration
