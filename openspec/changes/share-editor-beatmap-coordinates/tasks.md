## Prerequisites

- [ ] Complete the offline source-beatmap foundation and measured map-quality gate.
- [ ] Confirm the proposed editor scope and correction UX before implementation.

## Implementation

- [ ] Expose bounded visible-range native projections keyed by map/rate/viewport identity.
- [ ] Route display and snap operations through that evaluator via existing UiContext/controllers.
- [ ] Preserve scalar activity-base intent and loop-relative map numbering; keep continuous native phase independent of UI labels.
- [ ] Display current local source BPM at the retained/live playhead through that evaluator;
  distinguish master target and optional track summary, including seeks, wraps and uncertain coverage.
- [ ] Compute explicit map-mode auto-loop operations in beat space while preserving stored markers on map edits.
- [ ] Implement distinct global alignment and local beat-correction actions with undoable revisions.
- [ ] Add sample-domain anchor edits, separate count/downbeat repair and explicit uncertainty; evaluate optional onset refinement separately from raw Beat This.
- [ ] Preserve scalar BPM mode, source-time markers and manual corrections on reanalysis/restore.
- [ ] Show unavailable/uncertain coverage and displayed versus pending-audio revisions.
- [ ] Test shared coordinates, offsets, sample edits, invalid/count corrections, restore, master/key invariance and stale results.
- [ ] Update architecture/UI docs and native API typing.

## Acceptance

- [ ] Run full project checks and strict validation for this change.
- [ ] Verify editor behavior on constant/drifting sources with manual anchors and ambiguous breaks.
- [ ] Confirm unchanged live audio addressing and scheduler behavior.
