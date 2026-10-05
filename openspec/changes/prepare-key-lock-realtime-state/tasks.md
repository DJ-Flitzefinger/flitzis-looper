## 1. Safety and delay evidence

- [x] 1.1 Inspect pinned native cold/reset allocation paths and all callback reset edges.
- [x] 1.2 Define bounded ownership/delay safety gate and explicitly retain audible pre-roll acceptance.
- [x] 1.3 Measure native/adapter impulse timing and cold/warmed preparation costs in release.

## 2. Prepared native ownership

- [x] 2.1 Implement fixed unique-handle reserve/recycle lanes and one fallible engine worker.
- [x] 2.2 Replace callback native resets with local invalidation and nonblocking warm exchange.
- [x] 2.3 Preserve bounded silence fallback and dry playback when preparation is unavailable.
- [x] 2.4 Make adapter delay independent of regular/irregular segment partitions.
- [x] 2.5 Preserve processor history across stem-source crossfades.

## 3. Verification

- [x] 3.1 Test queue saturation/ownership, reset/retrigger/seek/modes, pause/resume and stem history.
- [x] 3.2 Test identical adapter output under unequal partitions, finite/pitch output and error paths.
- [x] 3.3 Pass official strict OpenSpec validation and full native/Python/static checks.
- [x] 3.4 Update maintained docs and record measured limits and next audible-preparation step in handoff.

## 4. Shared source-policy foundation

- [x] 4.1 Extract loop/seek addressing, stem compatibility and source crossfades into one borrowed-buffer reader used by the live mixer.
- [x] 4.2 Verify loop wraps, outside-loop seeks, stem masks/version fallback and source transition progress with focused tests and full native/Python checks.
- [x] 4.3 Update maintained architecture/backend docs and continuation state after validation.

## 5. Fractional source and rate progression

- [x] 5.1 Implement a reusable scalar fractional source epoch and linear two-tap reads through the shared integer loop/seek policy for every playback mode.
- [x] 5.2 Preserve fractional carry across rate rebases, pause/resume and in-range loop edits; keep explicit seek/retrigger/out-of-range edit behavior.
- [x] 5.3 Track source-domain stem transition progress fractionally and feed already-resampled canonical samples into the fixed native adapter.
- [x] 5.4 Apply per-voice ratio steps in every lock mode on fixed 512-active-output-frame intervals and split bounded rendering at rate boundaries.
- [x] 5.5 Verify immutable nonconstant sources, fractional ratios/BPM, loop wraps, explicit intro/tail seeks and prepared-stem sums across fixed/irregular/one-frame partitions at 44.1/48/96 kHz.
- [x] 5.6 Verify rate changes, pause/resume, source rebases and equal native input/output sequences without callback allocation or changed adapter delay.
- [x] 5.7 Pass official strict OpenSpec validation and full native/Python/static checks; update maintained docs and continuation state with evidence and remaining limits.

## 6. Non-live exact-source preparation proof

- [x] 6.1 Share fractional buffer filling with the live mixer and centralize exact inverse-pitch setup before native reset.
- [x] 6.2 Implement a test-only coherent native/FIFO fixture with bounded explicit discard and separate logical/feed cursors.
- [x] 6.3 Compare retained output and continuation against independent algebraic source reads and raw native output across rates, ratios, seeks, stems and partitions.
- [x] 6.4 Measure uncropped and retained startup/settled transient residuals, discarded energy and clipping in release; keep nominal discard experimental.
- [x] 6.5 Pass official strict validation and full native/Python/static checks; update maintained docs and handoff with exact evidence and limits.

## 7. Pending live audible preparation

- [x] 7.1 Anchor explicit source history before the requested logical phase; sweep bounded discard/marker-phase/short-burst fixtures and define/report the musical timing criterion before choosing compensation.
- [ ] 7.2 Resolve the failed nonneutral common-translation criterion using steady Key Lock and musical attack references; justify an onset/content policy before selecting compensation without hiding inherent transient spread or cut loss. The varispeed and unity-source bridges are measured offline against matched continuous histories. Unity source pitch incurs explicit rhythmic phase/interference costs; target-local mixture diagnostics do not restore discarded native energy or replace the unchanged failed criterion. Required pre-target headroom can exceed the nearest fine-grid future interval. Resolve causal scheduling/content feasibility before further bridge sweeps or live adoption.
- [ ] 7.3 Implement source/generation/loop/seek/stem/exact-ratio identity with fixed future handover frames and stale/late rejection.
- [ ] 7.4 Adopt coherent native/FIFO/feed state at the accepted render frame and retire replaced or stale ownership off-thread.
- [ ] 7.5 Verify source-aligned start/retrigger/seek, pause/resume and click-safe wet/dry/neutral/global/per-pad transitions, including queue/failure paths.
- [ ] 7.6 Validate audible compensation and obtain release/device acceptance before synchronized Quantize activation.

Non-live preparation and reference equality alone do not certify audible synchronization.
