## Why

Adjust Loop currently draws/snaps a scalar grid independently of Rust's source-phase helper.
After the offline map foundation, accepted variable maps need one evaluator for editor display,
snapping and diagnostic phase. Otherwise a correct analysis can still disagree with playback.

## What Changes

- Use the accepted map revision through one Rust evaluator for variable-map editor coordinates.
- Keep scalar mode and existing projects stable; make map acceptance and corrections explicit.
- Separate alignment correction, individual beat edits and creative playback phase.
- Show current local source BPM from the map at the pad playhead, distinct from master
  target BPM and an optional whole-track summary (user clarification, 2026-10-05).

Status: proposed only; depends on `prepare-versioned-source-beatmaps` and accepted map-quality
evidence. Rendering/SYNC activation remains a later change. See the research report in docs.

## Non-goals

No global master reanchoring, changed Quantize options, inference during drawing, live seek,
new meter support in the master, or automatic stretching. No second Python musical scheduler.

## Realtime Constraints

Visible-range evaluation and correction validation run on the control side. UI draws snapshots;
the callback does no UI work, allocation, analysis, locking, persistence or GIL access.

## Impact

Extend waveform editor and loop controller with accepted-map projections and correction intent.
Reuse UiContext action/state boundaries. Update UI/architecture docs in the implementation slice.
