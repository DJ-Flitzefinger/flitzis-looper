## 1. Rust: audio engine + messaging
- [ ] 1.1 Add `ControlMessage::UnloadSample { id }` (and wire it in the audio callback)
- [ ] 1.2 Implement sample-slot unload (clear slot, stop voices)
- [ ] 1.3 Implement looping voice rendering (wrap `frame_pos` at sample end)
- [ ] 1.4 Update/add Rust unit tests for looping + unload

## 2. Python/Rust FFI surface
- [ ] 2.1 Add `AudioEngine.unload_sample(id)` PyO3 method with consistent range/runtime checks
- [ ] 2.2 Update Python type stubs (`src/flitzis_looper_rs/__init__.pyi`)
- [ ] 2.3 Add/adjust Python tests for `unload_sample`

## 3. UI: context menu load/unload
- [ ] 3.1 Add a single **Load/Unload Audio** context menu item and toggle its label from pad state
- [ ] 3.2 When labeled **Load Audio**, show a file dialog filtered to `wav`, `flac`, `mp3`, `aif/aiff`, `ogg`
- [ ] 3.3 On selection, load audio into the pad’s sample slot and update the pad state
- [ ] 3.4 When labeled **Unload Audio**, stop playback and unload the slot, then update the pad state
- [ ] 3.5 Manual smoke: `python -m flitzis_looper` → load file, trigger loop, retrigger, unload

## 4. Validation
- [ ] 4.1 `cargo test --manifest-path rust/Cargo.toml`
- [ ] 4.2 `cargo fmt --manifest-path rust/Cargo.toml --check` and `cargo clippy --manifest-path rust/Cargo.toml`
- [ ] 4.3 `uv run pytest`
- [ ] 4.4 `uv run ruff check src` and `uv run ruff format --check src`
- [ ] 4.5 `uv run mypy src`
