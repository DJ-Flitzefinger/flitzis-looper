# Change: Add sample audio analysis (BPM, key, beat grid)

## Why
The application currently loads samples for playback but does not extract musical metadata. Adding BPM, key, and beat grid detection enables upcoming workflow features (sync/quantization/UI guidance) and avoids re-running expensive analysis repeatedly.

## What Changes
- Add an **audio analysis stage** to the async sample loading pipeline that runs after decode/resample and before the load operation is considered complete.
- Use the `stratum_dsp` crate to detect **BPM**, **key**, and **beat grid** for each loaded sample.
- Extend progress reporting to include an **Analyzing** stage; when fine-grained analysis progress is not available, use weighted progress that jumps to completion when analysis finishes.
- Store analysis results (BPM/key/beat grid) in app state intended for persistence, so results do not need to be recalculated after restart.
- Update UI:
  - Display BPM + key in each loaded pad (top-right corner).
  - Display BPM + key for the selected pad in the sidebar.
- Rename **"Re-detect BPM"** to **"Analyze audio"** and make it trigger analysis manually from both pad context menu and sidebar.
- Introduce support for per-pad background tasks beyond loading (starting with analysis-only jobs), so future operations like stem generation can run asynchronously with progress reporting.

## Impact
- Affected specs:
  - `async-sample-loading` (new stage + progress semantics)
  - `load-audio-files` (store per-sample analysis metadata)
  - `performance-ui` (display BPM/key in pads + sidebar)
  - `performance-pad-interactions` (new/renamed analysis action)
  - New capability: `audio-analysis` (analysis behavior and results)
  - New capability: `background-tasks` (general background operations)
- Affected code areas (implementation stage):
  - Rust: `rust/src/audio_engine/mod.rs`, `rust/src/audio_engine/sample_loader.rs`, `rust/src/messages.rs`
  - Python: `src/flitzis_looper/models.py`, `src/flitzis_looper/controller.py`, `src/flitzis_looper/ui/render/performance_view.py`, `src/flitzis_looper/ui/render/sidebar_left.py`
  - Tests: `src/tests/**`
