## Decision

**REJECTED for matrix expansion or productive adoption.** The real pinned
Signalsmith default preset passes its own unity coordinate control but fails
all four canonical q1/p2 kick cases under the unchanged nominal and strict
criteria. No production backend, clock, source map or admission guard changed.

## Actual measurements, 2026-10-22

The explicit Release feature run completed three technical tests in0.17s after
a2m48s build. Technical execution is separate from acoustic acceptance.
Queried W5292/I1323/Li2646/Lo2646 match the predeclared natural D5292/P0.
All four kick captures use complete streaming+single-flush output, retaining
278024 frames at origin or409096 at the interior event. Their full10584-frame
extra suffix remains present; no internal tail-exhaustion claim is made.

Both caller patterns are bit-identical over complete Raw. Distinct cases below
are shown once, omitting their identical irregular-caller repetitions:

| Canonical event | Actual nominal q10/q50/q90 errors, frames | Original peak relative to event | Nominal removed original energy | Strict pre-event original energy |
| --- | --- | --- | --- | --- |
|0|62,-78,51|-1|0.04655035205901923|0.04655035205901923|
|131072|34,-106,8|-1|0|0.046530746008063215|

The origin nominal window removes the original peak and about4.66% of full Raw
energy, exceeding0.001. The interior nominal q50 error exceeds88frames, and
strict first-sound gating would remove its original peak and about4.65% energy.
Fixed nominal end windows, complete extra suffix and final returned20ms all
have zero measured energy for these fixtures. Tail completion cannot repair
the attack failures.

Both full-Raw timing-only translation intervals are[-54,-18]. Their joint
timing/original-energy/peak intervals are[-54,-213] and[-54,-212], respectively,
and therefore empty. These are diagnostic intervals, never selected delay
corrections. No profile/preset sweep follows this failed prerequisite.

Own q1/p1 unity under both callers has q10/q50/q90 errors0/0/0 and peak offset0.
Observed maximum absolute PCM error is1.7881393432617188e-7, RMS
1.6713344510509268e-9; no sample-exact/timbre claim follows. All unity fixed
coordinate/energy/peak/tail criteria pass. Complete caller parity is exact.

## Source and resource evidence

All five vendored source/license byte counts and SHA256 values independently
match provenance.json; total107216 bytes. The three actual headers and their
portable include closure are pinned to the revisions in design.md. No external
FFT library, model, worker or application was started.

The maximum conservative pre-growth owned-array bound is129215168 bytes,
below256MiB. Complete current/previous Raw, source/shifted/canonical PCM, ending,
staging, process metadata and unchanged scorer temporaries are covered; actual
capacities are reported. Opaque Native RAM, allocator overhead, diagnostics,
process RSS, callback deadlines and resource216 are excluded.

## Verification

- Three explicit feature tests pass technically; audio decision remains0/4 REJECTED.
- Existing stretcher/owner/scorer consumers pass6 tests in2.27s, with4 deliberate
  opt-in diagnostics ignored. Two new control tests overlap the explicit run.
- Release all-target Clippy passes39.67s; existing97 library/132 test warnings
  (57 duplicates) and197 example warnings remain, with none in changed modules.
- Existing Portable build-path/toolchain/installation suites pass68 tests in221.07s.
- Feature-off locked Release wheel builds in38.66s with40 existing warnings;
  its private Native import passes12 existing API checks, editor4/globalTiming1
  and the optional timing-policy signature. Default direct build dependencies
  contain pkg-config only; the experimental C++ module/cc are disabled.
- Fresh private Release is34425856 bytes, SHA256
  `6e5f9f1b7cd146b5b3a8b751054e9030bf355031c1810f3e9a73922c24fe40fe`.
  No Native was installed; the actual installed file retains its prior
  `eaa3ca15b694218851cc138d62ed61c5843f6817ba528e5d328490df1413cf44`
  hash. Canonical `start.bat --check` passes without opening the App/device.
- Final formatting/strict checks and independent review complete the delivery boundary.

One independent nonauthor review is CLEAR for staged tree
`dffd09282f0cb9be2cd2072d1519c2ff96f65e9d`, including this subsequent factual
receipt and final task checkbox. Authored-code whitespace checks pass; only the
three exact upstream headers retain their original whitespace under scoped
attributes. All five upstream index bytes were verified against their pins.

Initial Portable attempt failed68 setups before product tests because Root placed
the temporary parent in the wrong directory. The owned harness path was corrected;
the final68 cases passed unchanged. Two initial Native regression filters selected
zero tests and are not passes; corrected `stretcher_probe` ran the six actual tests.
No product guard or assertion was weakened for these harness corrections.

Run the explicit measurement through the existing Windows wrapper:

```powershell
$env:UV_NO_SYNC = '1'
./scripts/run-rust-tests.ps1 -CargoArgs @('--release','--locked','--features','signalsmith-probe','signalsmith','--','--include-ignored','--nocapture','--test-threads=1')
```

Complete positive/negative rows, pin receipts and command logs remain in the
owned local `test-artifacts/signalsmith-canonical-20261022` evidence. The normal
installed Native is protected. This rejected component closes no continuous
SYNC, automatic map/B2/default cutover, prepared history, dynamic pitch, all-rate,
start-mode, actual stem, resource or musical/Human/device gate.
