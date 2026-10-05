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
