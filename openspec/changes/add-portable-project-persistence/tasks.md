## 1. Implementation

Note: Audio-file caching/copying into `./samples/` is handled in the separate `load-audio-files` session (not in this implementation pass).

- [x] 1.1 Add a small persistence module to load/save `ProjectState` at `samples/flitzis_looper.config.json` (atomic write).
- [x] 1.2 Restore `ProjectState` on app startup and apply it to controller initialization.
- [x] 1.3 Implement debounced save (max once per 10 seconds) triggered by `ProjectState` mutations.
- [x] 1.4 Ensure cached analysis data stays in `ProjectState` and is reused on restart.
- [x] 1.5 Remove cached WAV files on unload (best-effort; ignore missing).
- [x] 1.6 Implement graceful startup handling:
  - missing expected WAV -> ignore that pad
  - wrong sample-rate WAV -> ignore that pad
- [x] 1.7 Add/adjust tests for config serialization, restore behavior, and missing/mismatched sample handling.

## 2. Validation
- [x] 2.1 Run `pytest` for Python suite.
- [x] 2.2 Run `ruff`/`mypy` if part of CI for touched modules.
- [x] 2.3 Run `cargo test` for any Rust changes (if needed).
