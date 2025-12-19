## 1. Implementation
- [ ] 1.1 Add `AudioEngine.stop_sample(id)` to the Python API.
- [ ] 1.2 Extend Rust message protocol with a `StopSample { id }` control message.
- [ ] 1.3 Implement real-time-safe stop handling in the audio callback (stop all active voices for `id`).
- [ ] 1.4 Expand sample slot ID validation from `0..32` to `0..36` in `load_sample` and `play_sample`.
- [ ] 1.5 Update the performance pad UI to bind:
  - left-click → stop-then-play
  - right-click → stop
  - middle-click → open context menu
- [ ] 1.6 Implement a minimal per-pad context menu with at least a "Stop" action.

## 2. Tests / Validation
- [ ] 2.1 Update Python/Rust tests that assert out-of-range behavior (32 → 36).
- [ ] 2.2 Add/adjust an integration test that verifies `stop_sample(id)` silences an active voice deterministically.
- [ ] 2.3 Run `cargo test --manifest-path rust/Cargo.toml`.
- [ ] 2.4 Run `uv run ruff check src`.
- [ ] 2.5 Run `uv run mypy src`.
- [ ] 2.6 Run `uv run pytest`.
