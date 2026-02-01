# Change: Waveform editor grid offset

## Summary
Add a per-pad Grid Offset control in the waveform editor to shift the musical grid (and snapping anchor) left/right by a sample-accurate offset.

## Why
Some loops are recorded slightly off the intended downbeat. A per-pad grid anchor offset lets users align the rendered musical grid and snapping to the actual audio transient without changing the underlying audio or grid resolution.

## What Changes
- Add a per-pad Grid Offset control in the waveform editor.
- Apply the offset to the musical grid anchor used for both grid rendering and snapping.
- Persist the offset per pad in project save/load.

## Scope
- UI control
- Applying the offset to the grid anchor (snapping + grid rendering alignment)
- Per-pad persistence

## Non-Goals
- Do not introduce the grid resolution knob yet
- Do not change snapping resolution (still 1/64 grid) except by applying the anchor offset

## Impact
- Affected specs: `openspec/specs/waveform-editor/spec.md`, `openspec/specs/loop-region/spec.md`, `openspec/specs/project-persistence/spec.md`
- Affected behavior: waveform editor toolbar UI; musical grid/snapping anchor math; project save/load per pad
