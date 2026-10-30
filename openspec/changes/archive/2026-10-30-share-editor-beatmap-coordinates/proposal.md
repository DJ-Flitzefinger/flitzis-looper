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

Status: full B4 acceptance remains open. The source-qualified foundation and persisted
selection are delivered. `edit-manual-source-beatmaps` implements the explicit
manual/exact-fixture editor subset under the current full Human objective; measured
automatic detector/map quality is still required for automatic/default acceptance.
Audible map rendering/SYNC activation remains a later change. See the research report in docs.

## Non-goals

No global master reanchoring, changed Quantize options, inference during drawing, live seek,
new meter support in the master, or automatic stretching. No second Python musical scheduler.

## Realtime Constraints

Visible-range evaluation and correction validation run on the control side. UI draws snapshots;
the callback does no UI work, allocation, analysis, locking, persistence or GIL access.

## Impact

Extend waveform editor and loop controller with accepted-map projections and correction intent.
Reuse UiContext action/state boundaries. Update UI/architecture docs in the implementation slice.
