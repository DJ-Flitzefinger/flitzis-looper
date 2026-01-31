## 1. Spec Delta
- [x] 1.1 Update `specs/loop-region/spec.md` delta with 1/64-note grid snapping rules
- [x] 1.2 Specify grid anchor (default onset) and future additive grid offset (offset = 0)
- [x] 1.3 Document which BPM source is used for the grid (effective BPM)
- [x] 1.4 Validate change with `openspec validate add-64th-grid-snapping --strict`

## 2. Implementation
- [x] 2.1 Locate loop marker placement code path used by the waveform editor
- [x] 2.2 Implement 1/64-note grid snapping when `auto_loop_enabled = true`
- [x] 2.3 After grid snapping, quantize marker position to integer sample index and store time exactly `sample_index / sample_rate_hz`
- [x] 2.4 Ensure no snapping when `auto_loop_enabled = false`
- [x] 2.5 Handle missing effective BPM (confirm behavior matches spec)

## 3. Tests
- [x] 3.1 Add unit tests for snapping math (BPM -> grid step, nearest grid point)
- [x] 3.2 Add unit/integration tests for auto-loop enabled vs disabled marker placement
- [x] 3.3 Add regression test for effective BPM selection (manual override vs analysis BPM)
- [x] 3.4 Add tests that assert exact sample indices for snapped markers (no approximate float comparisons)

## 4. Manual QA
- [ ] 4.1 Auto-loop enabled: click near multiple grid points; verify markers land on nearest 1/64
- [ ] 4.2 Auto-loop disabled: click at arbitrary times; verify markers do not snap
- [ ] 4.3 Extreme zoom: verify sample-accurate marker placement still works
- [ ] 4.4 No BPM available: verify snapping behavior matches the spec
