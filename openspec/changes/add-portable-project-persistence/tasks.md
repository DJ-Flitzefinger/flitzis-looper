## 1. Implementation
- [ ] 1.1 Add a small persistence module to load/save `ProjectState` at `samples/flitzis_looper.config.json` (atomic write).
- [ ] 1.2 Restore `ProjectState` on app startup and apply it to controller initialization.
- [ ] 1.3 Implement debounced save (max once per 10 seconds) triggered by `ProjectState` mutations.
- [ ] 1.4 Copy user-selected audio into `./samples/` as a decoded+resampled WAV and update `ProjectState.sample_paths[*]` to point at the cached WAV.
- [ ] 1.5 Ensure cached analysis data stays in `ProjectState` and is reused on restart.
- [ ] 1.6 Remove cached WAV files on unload (best-effort; ignore missing).
- [ ] 1.7 Implement graceful startup handling:
  - missing expected WAV -> ignore that pad
  - wrong sample-rate WAV -> ignore that pad
- [ ] 1.8 Add/adjust tests for config serialization, restore behavior, and missing/mismatched sample handling.

## 2. Validation
- [ ] 2.1 Run `pytest` for Python suite.
- [ ] 2.2 Run `ruff`/`mypy` if part of CI for touched modules.
- [ ] 2.3 Run `cargo test` for any Rust changes (if needed).
