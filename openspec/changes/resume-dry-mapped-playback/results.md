# Stage A: qualified dry waveform Hold continuation

Baseline: `9e79e93a5ac3626b4629f6665d3435ebce7674c6`. At this independently
delivered Stage A checkpoint, Stage B ordinary Pause/Resume was unimplemented
and gated. This section is bounded synthetic software
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

# Stage B: ordinary transport and durable effective state

Implementation baseline: `cd3c811daaab4a0e2c09271bf57b485326a2b320`.
This stage connects actual public ordinary Pause/Resume to the qualified dry
continuation from Stage A. It retains each lane's exact saved fractional source
position and original map while the existing master continues. Unqualified mapped
lanes, mapped Seek, Native/pitch/KeyLock and other unsupported envelopes retain
their guards. A no-op still validates the complete relevant mapped cohort.

Native now returns an opaque admission with the original content lifetime,
transport revision and requested pause value. A fixed per-cell callback snapshot
records actual ordinary state and the exact completed Pause/Resume frontier.
Current observations remain queryable when a raw notification is superseded,
temporarily fenced or missed. A separate current-slot query bootstraps the first
observation and a genuinely new semantic source without validating old receipts.
Snapshot sequence and semantic-source epochs cannot wrap or resurrect old owners.

Accepted ordinary commands retain their existing ordered revision authority.
Feedback capacity is checked before dequeue/effect. A fixed pending-origin marker
preserves an exact caused Hold through pressure and an earlier superseded release
until the original ordinary command can dispose of that cause under its full key.
A refused effect remains refused; it does not grant a stale Hold release or alter
the trajectory. Source-stale dispositions retain the original source tag. Actual
source adoption carries only confirmed surviving ordinary cause and resets the
new source's completion frontier. Hold-only pause remains distinct.

Python keeps requested/pending intent per living content instance and projects
Native effective state before settling the exact admission. Its bounded current
query covers all living slots behind the existing loading, incoming, Pair,
arrangement and Bank fences, with instance/fence checks before and after query.
Fast gestures do not optimistically modify effective Session pause flags.
Ordinary feedback coalesces by stable owner/cell and newest observation sequence;
the old generic feedback route retains its own contract. Move/Swap follows content,
while replacement, Stop/birth and source invalidation prevent old-state revival.

Measured Release metadata is 88 B per observation slot; all 216 observation slots,
semantic-source epochs and pending origins together occupy 22,464 B within the
existing 32 KiB gate. The terminal 32-lane optional-key scratch is 1,024 B;
ControlMessage is 1,160 B and AudioMessage 56 B. These are absolute fixed-array
measurements, not a baseline delta, whole-engine RSS or full 216-PAD acceptance.

Compatible Release filter receipts and the final race regression contain 365 distinct Native cases and
15 existing ignored cases; overlapping filters and zero-case analysis results
are not counted again. The full Bank suite passes 45 cases with six existing
ignored in 1,561.96 s, including both actual 216-distinct and 216-duplicate stopped
batches. Cold adoption passes 17 cases in 11.26 s; ordinary start 61 in 66.02 s;
captured global start 22 with five existing ignored in 7.26 s; global playback
batch 21 in 6.78 s. The other receipts cover ordinary transport, source fencing,
Hold, placement feedback, runtime binding, source history, Copy and pitch births.
These are bounded software proofs with declared nonneural batch output; the
ignored actual-model resource profiles and full musical/parent gates stay open.

The final Clippy run initially passed in 45.82 s but exposed one new manual
saturating-arithmetic warning. Replacing `checked_add(1).unwrap_or(u64::MAX)` with
the exactly equivalent `saturating_add(1)` preserves every value and sticky MAX,
ordering and source guards. The same independent reviewer confirms equivalence.
The focused real terminal-source-fence case passes in 0.01 s after a 3m03 rebuild;
prior compatible suite evidence is retained. Final Release/workspace/all-targets
Clippy passes in 47.93 s: 96 library warnings, 131 test warnings (56 duplicates)
and 196 example warnings. The new warning is gone; no warning points into the new
ordinary-transport/custody/source tests or the repaired 216-source facade.

Earlier implementation checks also caught a missing test-only DTO Debug display,
an unqualified mapped no-op bypass and an outdated unconditional Native-epoch
expectation for already-paused ManualPause. The no-op now validates the complete
mapped cohort; the test expects no epoch mutation only for that genuine no-op.
Four later fixture repairs initialize embedded Python in the fresh child, seek
inside the genuinely prepared window, establish legitimate equal numeric origins
across different lifetimes, and forward the actual current-state query. Original
guards, PCM/phase/ACK/isolation and exact settlement assertions remain intact.

The final installed-Native Python profile passes 922 cases in 70.54 s across
17 affected controller, UI, capability, launcher and resident-API files. Ruff and
format pass for all 15 changed Python/stub files; Mypy passes for the five affected
production controller/UI files. Cargo fmt, explicit changed-Rust-file rustfmt,
both affected strict OpenSpec validations and whitespace checks pass.

Before the race correction, normal `maturin develop --locked --release` installs the editable
Release in 37.61 s after a fresh check at 22:03:49.690 UTC finds zero Native holders
and zero uninspected candidate processes. That then-installed checkout Native is
34,446,848 B, SHA-256
`f4f22cd581597ea514b77236deadd78cbbebec58c5d2af68fa88d8fbd0c98e56`.
A fresh interpreter validates its actual origin, Release profile, all 13 bootstrap
API families, editor API 4, ordinary transport API 1 and nonconstructible DTOs.
After another holder check, canonical `start.bat --check` passes without starting
the App/device; the installed bytes/hash remain identical afterward.

The replacement exact staged-diff independent review and delivery receipt remain pending. The
owned logs are in `test-artifacts/ordinary-transport-feedback-20261022` in the
workspace; short disposable fixtures are under `test-artifacts/otf22`. No App or
audio device is started for these checks. Full SYNC, automatic musical qualification/B2,
continuous live policy adoption, mapped Seek, LoopOFF/EOF, the complete rate/pitch
range and all original/extensions/Human/final gates remain open.

The first repaired bank run found a test-facade mismatch during its actual
216-distinct stopped batch: its streamless cached-shape accessor additionally
asserted that the optional current source-digest query could never be unavailable.
The real shape API does not impose that authority condition; a concurrent
residency publication can hold the shared placement marker. The child failed in
597.98 s. Its log does not identify the exact internal unavailable branch and does
not prove source corruption. Root ended the exact owned failed filter and its
subsequent duplicate child; that interrupted case has no passing result. The
narrow adapter correction now has a passing deterministic regression (0.34 s)
using real canonical cold adoption and its own callback ACK, then an actual
same-lifetime slot-write claim: digest is unavailable while the original raw
digest, PCM Arc, generation/rate and cached shape remain exact; releasing the
claim restores the original digest. Initial source-digest checks, all final 216
own-ACK/ownership assertions, production guards and test deadlines remain intact.
The full affected bank rerun now passes as recorded above. A filename-based
cold-adoption filter initially matched zero cases; the zero-case guard stopped it
and the actual module was rerun successfully. Zero-case results are never passing
evidence.

The first exact final review withheld clearance despite these passing sequential
checks. Render-tail Hold cleanup read pending custody before intent freshness;
an ordinary admission between those loads could make it discard an exact caused
pause while the original command still awaited feedback capacity. Cleanup now
acquires intent freshness before reading custody. Observing the newer intent
therefore also observes its preceding custody publication; invalid content,
cohort and lifetime retirement retain precedence.

The unchanged deterministic actual-public-admission regression fails on the old
test-only ordering after a 3m31 rebuild (0.19 s, actual Superseded versus expected
PausedOwned). The normal Release path stayed corrected during this negative
control. Restoring the exact fixed bytes produces the expected pass after a 3m08
rebuild (0.35 s), with both admission schedules, real held voices, zero feedback
capacity and the later exact single completion. No guard, deadline or assertion
is weakened. Fresh affected integration passes ordinary transport 29 (10.05 s),
Hold 32 (4.57 s), control Hold 12 (25.64 s), mapped custody 48 with three existing
ignored (41.28 s), feedback 15 (6.35 s), and the two actual active/recovered
Bank-with-Hold regressions (12.60 s and 0.91 s). The earlier stopped 216-source
and unchanged Python receipts remain compatible with this local cleanup order
correction; they do not substitute for the new interleaving proof.

Final Release/workspace/all-targets Clippy passes in 41.83 s with the same 96
library, 131 test (56 duplicates) and 196 example warnings. Cargo fmt, explicit
changed-file rustfmt with the actual 2024 edition, both strict validations and
whitespace checks pass. An explicit rustfmt
check was first invoked with the wrong edition 2021 and rejected existing 2024
syntax; it made no writes. The actual crate edition is 2024. This command error
is separate from source or formatting acceptance. The old staged review tree is
obsolete; the same continuous independent reviewer must clear its replacement.

Final corrected normal Release build/install passes in 42.56 s after the fresh
22:32:02.208 UTC holder check finds zero candidates, holders or uninspected
processes. The installed checkout Native remains 34,446,848 B and now has SHA-256
`91881c187435c0bad0e2c6382a901f7af5c6585caf3665e0da1b6928ac35e6ab`.
Fresh actual-origin/Release/13-family API, editor 4, ordinary 1 and nonconstructible
DTO checks pass. After another zero-holder check at 22:33:29.395 UTC, canonical
`start.bat --check` passes with the same installed hash and starts no App/device.
The unchanged 922-case Python and static-check evidence is reused explicitly;
the final source change affects the Native cleanup ordering, covered freshly above.
Counting all 27 accepted compatible passing filter receipts by actual test name
confirms 365 distinct passing and 15 existing ignored cases. A first overly strict
log parser missed embedded-Python-interleaved terminal lines; the corrected count
requires every passing filter's complete header count to match its result summary.

The same continuous independent nonauthor reviewer clears staged tree
`edd8a2c04c6f2332d033bc3725cd5fe44fbee412`, diff SHA-256
`ea37a6b4b43fccff02b809fe1e54ae6d963adb94af6a9b4fb9572620e201dda7`, with no
remaining actionable finding. Clearance explicitly covers this factual receipt
and the final Stage B verification checkbox; it does not cover new behavior or
test changes. This delivery closes the bounded ordinary transport stage, while
the complete product, resource, device, music and hearing gates stay open.
