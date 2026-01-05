# Change: Portable project persistence (state + samples)

## Why
Currently `ProjectState` is intended to be persistent, but it is not saved/restored on startup/shutdown. This makes the app feel “stateless” across restarts and forces users to re-load samples and re-run analysis.

## What Changes
- Persist `ProjectState` to `./samples/flitzis_looper.config.json` and restore it on application start.
- Save is triggered by `ProjectState` mutations and debounced to at most once every 10 seconds.
- Make projects portable by ensuring every user-loaded audio file is copied into `./samples/` as a decoded, resampled WAV using the original filename (basename) as the primary name.
- Cached WAVs are encoded in the engine’s internal sample format (currently `f32`) to minimize conversions on fast loads.
- Persist cached audio analysis (BPM, key, beat grid/onsets if available) as part of project state so restart does not require re-analysis.
- Startup handles missing/mismatched sample cache files gracefully (ignore missing samples; ignore cached WAVs with wrong sample rate).
- When a pad is unloaded, its corresponding cached file in `./samples/` is removed (best-effort; missing is ignored).

## Impact
- Affected specs:
  - `load-audio-files` (loading workflow gains project-local WAV caching behavior)
  - New: `project-persistence` (save/restore lifecycle, debouncing, portability rules)
- Affected code (implementation stage):
  - Python: `src/flitzis_looper/controller/facade.py`, `src/flitzis_looper/controller/loader.py`, `src/flitzis_looper/ui/__init__.py`, and potentially a new small persistence module.
  - Rust: likely none required initially; existing decode/resample pipeline already exists.
