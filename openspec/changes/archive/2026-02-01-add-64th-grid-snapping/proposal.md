# Change: Add 1/64-note grid snapping for loop markers

## Problem
Today, when auto-loop is enabled, loop marker edits snap to analysis beat times ("beat snapping").
This is too coarse for tight loop editing and prevents musically meaningful placement between beats.

## Scope
- Snapping rules only (behavior when setting/moving loop start/end)
- No UI potentiometer (no user-selectable snap resolution yet)
- No waveform/grid rendering changes
- No grid offset UI yet

## Non-Goals
- Do not redesign pitch/BPM behavior
- Only document which BPM source is used for the grid: effective BPM (manual BPM override first, else analysis BPM)

## Proposed Change
When auto-loop is enabled, loop start/end adjustments snap to a musical grid at 1/64-note resolution,
derived from the effective BPM.

When auto-loop is disabled, loop marker adjustments do not snap (current behavior).

## Precision
Snapping MUST be sample-accurate (no sub-sample timestamps) because millisecond-level offsets are audible.

## Impact
- Affected spec: `openspec/specs/loop-region/spec.md`
- Affected behavior: waveform editor loop marker placement when `auto_loop_enabled = true`
