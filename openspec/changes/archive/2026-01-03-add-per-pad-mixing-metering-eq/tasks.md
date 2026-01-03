## 1. Implementation
- [x] 1.1 Add per-pad gain + EQ fields to `ProjectState` and defaults (length `NUM_SAMPLES`)
- [x] 1.2 Add per-pad meter fields to `SessionState` and update helpers in UI state
- [x] 1.3 Extend Rust `ControlMessage` for `SetPadGain` and `SetPadEq` and thread them into `AudioEngine` Python API
- [x] 1.4 Implement per-pad gain application in `RtMixer` and validate with Rust unit tests
- [x] 1.5 Implement 3-band EQ DSP (crate reuse or in-tree copy) and validate with Rust unit tests (sanity + RT-safety review)
- [x] 1.6 Add audio-thread peak computation and rate-limited `AudioMessage::PadPeak` emission
- [x] 1.7 Wire Python polling of `receive_msg()` each frame and update session metering state
- [x] 1.8 Render a small per-pad VU meter overlay in `performance_view.py` using cached peak values
- [x] 1.9 Render new left sidebar section for Gain + EQ controls for selected pad (prefer `imgui_knobs`, fallback to sliders)
- [x] 1.10 Add/extend Python tests for controller state updates and basic wiring (no real audio device required)

## 2. Validation
- [x] 2.1 `cargo test --manifest-path rust/Cargo.toml`
- [x] 2.2 `uv run ruff check src` and `uv run ruff format --check src`
- [x] 2.3 `uv run mypy src`
- [x] 2.4 `uv run pytest`
