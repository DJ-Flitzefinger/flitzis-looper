# Change: Manual Pad BPM (Direct Entry + Tap BPM)

## Change ID
`add-pad-manual-bpm`

## Status
Proposed

## Summary
Add a per-pad manual BPM workflow in the left sidebar: direct numeric BPM entry plus a Tap BPM button that averages recent taps to compute and set the manual BPM.

## Why
Auto-detected BPM is useful but not always correct. During performance, the user needs a fast and reliable way to correct tempo metadata per pad, either by typing an exact value or by tapping along to the loop.

## What Changes
1. Introduce per-pad **manual BPM** state that can override the detected BPM for UI display and downstream tempo workflows.
2. Add a **BPM input** control to the selected-pad (left) sidebar to set/clear manual BPM.
3. Add a **Tap BPM** control to the left sidebar that records taps on **mouse down** and computes BPM from the average of recent taps.
4. Update BPM display behavior to show the **effective BPM** (manual when present, otherwise detected) on both the pad and in the sidebar.

## Out of Scope
- BPM lock/master BPM behavior (tracked separately in legacy parity item 6).
- Any change to playback rate/time-stretching based on BPM (this change only establishes and edits tempo metadata).
- Persistence/migration strategy for saved projects beyond adding the new fields (tracked in legacy parity item 13).

## Impact
- Affected specs:
  - `performance-ui` (new sidebar controls and display rules)
  - `pad-manual-bpm` (new capability)
- Affected code (expected):
  - `src/flitzis_looper/models.py` (store manual BPM per pad)
  - `src/flitzis_looper/controller.py` (actions to set/clear/tap BPM)
  - `src/flitzis_looper/ui/render/sidebar_left.py` (BPM input + Tap BPM)
  - `src/flitzis_looper/ui/render/performance_view.py` (effective BPM overlay)
  - Tests under `src/tests/`
