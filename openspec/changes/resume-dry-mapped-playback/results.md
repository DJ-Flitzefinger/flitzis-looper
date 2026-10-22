# Stage A: qualified dry waveform Hold continuation

Baseline: `9e79e93a5ac3626b4629f6665d3435ebce7674c6`. Stage B ordinary
Pause/Resume remains unimplemented and gated. This is bounded synthetic software
evidence, not full SYNC, automatic map qualification, device or hearing acceptance.

## Actual control and PCM evidence

The new tests launch a genuinely qualified original map, then use the public
PyO3 Hold admission and real callback. One test also invokes the actual Python
WaveformController. They check silence while master time advances; exact saved
fractional source bits; independent dry PCM through later knots and wrapping;
repeated pauses; bit-identical single-frame versus irregular caller output;
unchanged historical read spans; and a later actual all-dry master claim followed
by another Hold. Independent PCM comparison retains its fixed `3e-6` bound.

Other cases cover release before acknowledgement, pre-existing pause/no effect,
mixed mapped/scalar cohort masks, all-or-none refusal, original content following
a completed Move, unrelated scalar playback, stop/retrigger/seek supersession,
stale owner/cursor/source/acknowledgement/master, terminal output and exact-work
retirement deferral. Artificial mapped fixtures without qualified ownership
remain negative. Ordinary public transport remains refused in Stage A.

All checks use the normal locked Release profile, without opening an audio device.
The final focused run passed 13 cases in 6.43 s after the bounded retirement
fixture cleanup (including one existing legacy rejection). It overlaps the
integrated suites below; unchanged production regression evidence is reused.

| Final production regression filter | Passed | Existing ignored | Runtime |
| --- | ---: | ---: | ---: |
| `mapped_source_clock` | 20 | 0 | 0.01 s |
| `mapped_launch_custody_tests` | 43 | 3 | 34.73 s |
| `hold_action` | 32 | 0 | 4.03 s |
| `control_hold_tests` | 12 | 0 | 24.11 s |
| `content_feedback_tests` | 13 | 0 | 4.98 s |
| `transport_intent_control` | 3 | 0 | 0.50 s |
| `source_feed_history` | 22 | 1 | 10.29 s |

These contain 145 distinct passing Native cases. No zero-case analysis-crate
results are counted. Four existing Python waveform/playback/Pair/isolation
controller files pass 69 cases in 2.30 s; those mock-based regressions supplement
the real embedded Python/PyO3/callback test above.

## Measured copied-state bounds

Actual Release layout: epoch 136 B, mapped span 224 B, SourcePlayback 336 B,
SourceReadCheckpoint 496 B, SourceFeedSpan and its Option 1,312 B. The old
`MappedSourceSpan <= 240` ceiling remains unchanged. The copied 32-lane preflight
array is 10,752 B, within its 16 KiB ceiling; it creates no callback heap owner.

Each span owner has 1,024 slots. There are 32 voice journals and 32 prospective
snapshots allocated at startup: their combined span payload is 85,983,232 B
(82 MiB). Up to 64 additional action snapshots are control-allocated under the
existing shared action capacity: the maximum simultaneous span payload is
171,966,464 B (164 MiB). These are absolute payload measurements, not a measured
baseline delta, whole-engine RSS, Native/PCM, allocator/Arc/ring/header or 216-PAD
resource acceptance.

## Preserved negative checks and delivery boundary

The first compile failed before tests because two inner doc comments occurred
in an `include!` test file; changing only those comments corrected it. The next
run had 12 passes and one new Move-fixture failure: control placement correctly
remained Pending until actual Pair reconciliation/validation/finish. The corrected
test checks Pending first and retains its original completed-Move, exact-resume,
source-owner and neighbor assertions. No production guard or criterion changed.

The first Clippy run completed successfully in 38.95 s and reported one new
test-only `Vec<Box<_>>` warning. Its fixture now retains the exact two retirement
boxes in fixed Option slots. The 13-case focused recheck passes. Final Release
Clippy passes in 34.26 s with the unchanged 97 library, 132 test (57 duplicate)
and 197 example warning counts; the new fixture warning is gone.

Normal `maturin develop --locked --release` built and installed the editable
Release in 37.15 s after a fresh check found zero Native holders and zero
uninspected candidate processes. No App was started or stopped. The installed
Native is 34,397,696 B, SHA-256
`4a1ff877f7a111c69e5ff271f5a8dc26b0ba88e8cd905ef6beddbe06f8344754`.
A fresh interpreter validates the actual checkout origin, Release profile and
all 12 existing bootstrap API families (including editor API 4). Canonical
`start.bat --check` passes without starting the App/device. Both affected strict
OpenSpec checks, Cargo fmt, explicit included-file rustfmt and whitespace pass.
The single continuous independent NONAUTHOR review is CLEAR for staged tree
`0590bed029857c10d765e050372af8123f3c49f9`, exact staged diff SHA-256
`912c55d29a1e2983de963fe4285afee7d815b0c91dbab00099e0163713e76e34`.
It explicitly covers only this subsequent factual receipt and the final Stage A
task checkbox; no behavior or test changed after the reviewed tree.
Detailed command logs remain in the owned `test-artifacts/dry-mapped-hold-20261022`
workspace evidence directory.

Ordinary feedback, mapped Seek, live policy adoption, Native/pitch/KeyLock,
full-map/B2, both complete start policies, the full rate/pitch range and all
original/extension/Human/final gates remain open. Stop before full Rust-port/Slice8.
