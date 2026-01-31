# Change: Waveform editor musical grid

## Summary
The waveform editor currently shows a grid that is not aligned to the musical 1/64-note grid used for loop snapping.
We want a single musical grid in the waveform editor that aligns with the 1/64 snapping grid, and adapts its visible subdivision based on zoom level.

## Scope
- Grid rendering only
- No new snapping changes; snapping is already implemented
- No UI knob for grid resolution
- No grid offset UI yet

## Non-Goals
- Do not change audio playback
- Do not change BPM detection
- Do not change loop-region snapping rules in this change

## Impact
- Affected spec: `openspec/specs/waveform-editor/spec.md`
- Affected behavior: waveform editor grid overlay rendering
