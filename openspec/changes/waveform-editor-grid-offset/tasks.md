## 1. Spec Delta
- [x] 1.1 Add delta specs (waveform-editor, loop-region, project-persistence).
- [x] 1.2 Validate change with `openspec validate waveform-editor-grid-offset --strict`

## 2. Implementation
- [x] 2.1 Implement UI control and storage (per pad).
- [x] 2.2 Apply offset to the snapping/grid anchor.
- [x] 2.3 Clamp behavior (+/- 1 bar).

## 3. Tests / QA
- [x] 3.1 Add tests where feasible.
- [ ] 3.2 Manual QA checklist.

## 4. Manual QA
- [ ] 4.1 Grid Offset default is 0 on older projects.
- [ ] 4.2 Drag left/right changes offset in samples; value display updates.
- [ ] 4.3 Left-drag uses 1-sample increments; right-drag uses 10-sample increments.
- [ ] 4.4 Auto-loop enabled: snapping uses shifted anchor; markers align with rendered grid.
- [ ] 4.5 Clamp: cannot exceed +/- 1 bar; changing BPM re-clamps if needed.
- [ ] 4.6 Extreme zoom: snapped markers remain sample-accurate (integer sample indices).
