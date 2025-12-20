## 1. Rust/PyO3: speed message plumbing (no DSP behavior)
- [ ] 1.1 Add a `ControlMessage` variant for global speed (e.g., `SetSpeed(f32)`).
- [ ] 1.2 Expose `AudioEngine.set_speed(speed: float)` in PyO3 with validation (finite, 0.5×..2.0×, initialized engine).
- [ ] 1.3 Send `SetSpeed` via the existing control ring buffer.
- [ ] 1.4 Handle `SetSpeed` in the CPAL callback message loop by storing the latest value (no change to mixing/rendering in this change).
- [ ] 1.5 Ensure `set_speed` is best-effort if the control ring buffer is full (drop update; no exception).
- [ ] 1.6 Update Python type stubs (`src/flitzis_looper_rs/__init__.pyi`) to include `set_speed`.

## 2. Python: app state + UI controls
- [ ] 2.1 Add app-level speed state (default 1.0×) and helper methods for setting and resetting speed.
- [ ] 2.2 Add performance UI controls:
  - [ ] 2.2.1 Slider: range 0.5×..2.0×, default 1.0×, stable tag `speed_slider`
  - [ ] 2.2.2 Reset button: sets speed to 1.0× and updates the slider, stable tag `speed_reset_btn`
- [ ] 2.3 Ensure slider interaction stays performance-friendly (avoid modal dialogs during drag; validate/clamp in the control layer).

## 3. Tests
- [ ] 3.1 Update `FakeAudioEngine` used by `src/tests/flitzis_looper/test_app.py` to include `set_speed`.
- [ ] 3.2 Add Python unit tests for app-level set/reset speed wiring.

## 4. Validation
- [ ] 4.1 `cargo test --manifest-path rust/Cargo.toml`
- [ ] 4.2 `cargo fmt --manifest-path rust/Cargo.toml --check` and `cargo clippy --manifest-path rust/Cargo.toml`
- [ ] 4.3 `uv run pytest`
- [ ] 4.4 `uv run ruff check src` and `uv run ruff format --check src`
- [ ] 4.5 `uv run mypy src`
- [ ] 4.6 Manual smoke: `python -m flitzis_looper` → verify Speed slider + Reset update without errors

## Follow-up (separate change proposal)
- Implement varispeed playback in Rust using a specialized DSP/resampling library and update specs/tests to require audible behavior.
