# Change: Update sample loading progress indicators

## Why
`load_sample_async()` may emit `LoaderEvent::Progress` events, but the current implementation only emits a single coarse update (and does not provide sub-task context). This makes it hard for the UI to communicate what is happening and how long it will take.

## What Changes
- Emit `LoaderEvent::Progress` regularly during `load_sample_async()` with a best-effort *total* progress value in `percent` (0.0..=1.0) across the full pipeline.
- Include a human-readable `stage` string for each progress update using a main task plus optional sub-task, e.g. `Loading (decoding)` and `Loading (resampling)`.
- Update the UI to show progress in both places:
  - the pad grid: stage + percentage text and an in-pad background progress bar
  - the selected-pad sidebar: stage + percentage text

## Impact
- Affected specs:
  - `async-sample-loading` (progress event semantics and payload)
  - `performance-ui` (pad loading/progress indicator)
- Affected code (expected):
  - Rust sample loading + event emission (`rust/src/audio_engine/mod.rs`, `rust/src/audio_engine/sample_loader.rs`, `rust/src/messages.rs`)
  - Python controller/UI state + rendering (`src/flitzis_looper/controller.py`, `src/flitzis_looper/models.py`, `src/flitzis_looper/ui/render/performance_view.py`, `src/flitzis_looper/ui/render/sidebar_left.py`)