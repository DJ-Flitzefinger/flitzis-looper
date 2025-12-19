## 1. Core state (Python)
- [ ] 1.1 Add a `multi_loop_enabled: bool` flag (default: disabled)
- [ ] 1.2 Track an `active_pads` / `active_sample_ids` set based on play/stop/unload actions
- [ ] 1.3 Implement mode-dependent trigger logic:
  - MultiLoop disabled → stop other active pads first
  - MultiLoop enabled → do not stop other pads

## 2. Rust/PyO3: stop-all API
- [ ] 2.1 Expose `AudioEngine.stop_all()` in PyO3 with consistent runtime checks
- [ ] 2.2 Wire `ControlMessage::Stop` end-to-end in the audio callback
- [ ] 2.3 Add Rust unit tests covering stop-all behavior in `RtMixer`
- [ ] 2.4 Update Python type stubs (`src/flitzis_looper_rs/__init__.pyi`) and add Python tests for `stop_all()`

## 3. Performance UI
- [ ] 3.1 Add a MultiLoop toggle control below the bank row with stable tag `multiloop_btn` and legacy-inspired styling
- [ ] 3.2 Change pad triggering and right-click stopping to fire on **mouse down** (not mouse release) and ensure no double-trigger
- [ ] 3.3 Visually indicate active pads (playing) and keep the indicator in sync on play/stop/unload

## 4. Validation
- [ ] 4.1 `cargo test --manifest-path rust/Cargo.toml`
- [ ] 4.2 `cargo fmt --manifest-path rust/Cargo.toml --check` and `cargo clippy --manifest-path rust/Cargo.toml`
- [ ] 4.3 `uv run pytest`
- [ ] 4.4 `uv run ruff check src` and `uv run ruff format --check src`
- [ ] 4.5 `uv run mypy src`
- [ ] 4.6 Manual smoke: `python -m flitzis_looper` → load two loops, toggle MultiLoop, verify mouse-down onset + mouse-down right-click stop + MultiLoop button placement + active-pad indicators
