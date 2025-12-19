## 1. Implementation
- [x] 1.1 Add `AudioEngine.stop_sample(id)` to the Python API.
- [x] 1.2 Extend Rust message protocol with a `StopSample { id }` control message.
- [x] 1.3 Implement real-time-safe stop handling in the audio callback (stop all active voices for `id`).
- [x] 1.4 Expand sample slot ID validation from `0..32` to `0..36` in `load_sample` and `play_sample`.
- [x] 1.5 Update the performance pad UI to bind:
  - left-click → stop-then-play
  - right-click → stop
  - middle-click → open context menu
- [x] 1.6 Implement a minimal per-pad context menu with at least an "Unload Audio" action.

## 2. Tests / Validation
- [x] 2.1 Update Python/Rust tests that assert out-of-range behavior (32 → 36).
- [x] 2.2 Add/adjust an integration test that verifies `stop_sample(id)` silences an active voice deterministically.
- [x] 2.3 Run `cargo test --manifest-path rust/Cargo.toml`.
- [x] 2.4 Run `uv run ruff check src`.
- [x] 2.5 Run `uv run mypy src`.
- [x] 2.6 Run `uv run pytest`.
