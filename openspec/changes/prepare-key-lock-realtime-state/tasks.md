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

Exact-ratio source pre-roll, independent DSP feed-ahead and audible prepared-state handover remain
pending slice 3 work. Fractional feed equivalence alone does not certify audible synchronization.
