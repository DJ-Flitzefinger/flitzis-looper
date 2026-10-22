## Result: candidate rejected

The separate R3 realtime Standard/HighConsistency canonical-input candidate
fails all four retained kick cases. Both test functions complete their technical
assertions, and the unity coordinate control passes under both caller patterns.
This is a valid negative acoustic measurement, not a supported renderer.

The actual pre-signal API values match the declared values: P=2048, N=1024 at
q=1,p=2; unity uses P=N=2048. Every input sample is submitted once in identical
512-frame blocks plus the same remainder. All actual output drains to available
-1, and caller pairs are bit-identical over the entire raw capture. The original
interior transcript matches the retained Live hard-negative input exactly.

| Canonical event | Actual nominal q10/q50 errors, frames | Complete raw frames | Extra raw suffix frames |
| ---: | ---: | ---: | ---: |
| 0 | +222 / +107 | 266179 | 3007 |
| 131072 | -164 / -17 | 397231 | 2987 |

Each row is observed under both callers. Every fixed nominal window is available.
The unchanged budget is 88 integer frames at 44.1 kHz. Original raw peaks survive
the documented nominal crop; all nominal fixed-end, extra-suffix and drained-end
energy fractions are zero. Negative-nominal discarded energy is about 0.0008235884
for the first event and zero for the interior event. Timing still fails in all cases.

The retained unchanged strict calculation also fails all four cases. Its necessary
timing intervals are [134,195] and [-105,-76], so there is no shared constant
translation across the two origins. Their joint timing/retention intervals are
[134,0] and [-105,-173], both empty. At the interior FirstSound boundary, about
15.124% of original raw energy and the original peak 172 frames earlier would be
lost. These are complete-raw diagnostics; no fitted compensation is selected and
the captured PCM is not replaced by that hypothetical gated response.

Unity has zero measured q10/q50/q90 errors and zero peak offset. It is not
sample-identical to the impulse: maximum PCM error is 0.08216536045074463 and RMS
error is 0.001779121905154778. Those errors remain visible; this API's timing
contract is not treated as a sample-exact waveform guarantee.

## Verification and limits

The two new tests pass their technical checks in 1.40 s after a 2m28s Release compile.
The existing R2/R3 offline-owner technical regression passes in 0.39 s with its
offline-only assertions intact. Release Clippy passes in 33.71 s with existing
warnings and no changed-file warnings. No test failed; acoustic rejection is an
explicit result rather than a weakened assertion or a claimed audio pass.
Cargo formatting, standalone formatting of the new test file, both affected
strict OpenSpec validations and final diff checks also pass.

Before the first execution, source review strengthened the fixed-end measurement
to require both its actual nominal-output energy denominator and its full-original
energy denominator, preserving the existing emitted-output criterion. Both are
reported. The old offline bodies, fixtures and negative results are unchanged.

This failed prerequisite does not justify expanding this candidate into the
extreme-pitch matrix or introducing a productive backend, adapter, guard change
or compensation. Production code and the installed Native module are unchanged;
compatible preceding Release/import/preflight evidence is reused. No App or audio
device is started. Musical maps, tones, dynamic pitch, callback resources,
owning-future/Mixer adoption and all whole-program/Human gates remain open.
