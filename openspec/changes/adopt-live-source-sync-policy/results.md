## Stage 0: compact existing master projection, 2026-10-23

The existing mapped source clock now retains a 32-byte immutable projection
extracted by TransportTimeline instead of copying the whole mutable transport.
Its four fields are exactly the existing beat/epoch inputs. The binary64
expression order, original phase/marker, pause address translation, master
continuation anchors, ownership and exact continuity guards remain unchanged.
This adds no policy adoption, converter, audio owner or second clock.

Current coherent source change: transport.rs, mapped_source_clock.rs and their
mapped-clock/playback tests. New transport tests compare projection bits with
the unchanged original TransportTimeline evaluator across fractional periods,
negative/positive anchors and large frames through u64::MAX; check all four
epoch inputs, signed zero and missing master; and distinguish frozen projection
from unrelated progress/display/bootstrap state. The new mapped test compares
original master arithmetic at distant addresses and preserves marker/phase fences.

### Fresh verification

Commands use the existing environment/buildcache and
scripts/run-rust-tests.ps1 -CargoArgs with --release --locked --lib. Each selected
looper test set is disjoint; filtered analysis-crate zero-test output adds no pass.

| Affected native set | Result |
| --- | --- |
| audio_engine::transport::tests | 40 PASS, 0 FAIL/IGNORE; compile 3m07s, tests <0.01s |
| audio_engine::mapped_source_clock::tests, including copied SourcePlayback | 21 PASS, 0 FAIL/IGNORE; 0.01s |
| mapped_dry_global_rate_tests, including two-owner cohort | 8 PASS, 0 FAIL/IGNORE; 5.15s |
| mapped_hold_continuation_tests, including ordinary Pause/Resume | 17 PASS, 0 FAIL/IGNORE; 16.58s |
| mapped_fixed_pitch_birth_tests | 6 PASS, 0 FAIL/IGNORE; 11.63s |

Total: **92 distinct native PASS**, including four new test functions. Existing
canonical position/PCM/partition, pause/master-change, admission and owner guards
remain in these tests. Python source/API/persistence behavior is unchanged; the
prior compatible Python evidence is retained, not reported as freshly rerun here.

Measured fixed layout: projection 32 B; mapped epoch 88 B; mapped span 176 B
(previously 224 B, still <=240); SourcePlayback 288 B; 32-lane optional playback
copy 9,216 B (still <=16 KiB); checkpoint 448 B. The historical-span payload
measurement explicitly excludes Native/PCM, allocator/Arc/ring and inline Work
overhead; these sizes do not prove whole-process RSS or 216-PAD resource acceptance.

Release all-targets Clippy PASS, 48.62s; existing warnings 96 lib / 131 lib-test
(56 duplicates) / 196 example. Warning sites in touched paths refer to unchanged
old fields/methods/test bodies; no lint suppression was added. Cargo fmt, both
included-test rustfmt checks, strict OpenSpec and whitespace checks PASS.

One normal private Release wheel built in 43.36s (39 existing lib warnings).
Fresh private-origin import/profile and all 13 existing Portable API checks PASS,
including editor API4 and ordinary transport. Private Native: 34,440,704 B,
SHA256 `51014911a1a0a88aeb87baef1cb661bdd3b4f39076a31b080f600f81ff3b0ee1`.
It was not installed. The actual previously installed Native remains byte-identical
at 34,446,848 B / `91881c187435c0bad0e2c6382a901f7af5c6585caf3665e0da1b6928ac35e6ab`;
the canonical start.bat --check also PASS on that explicitly unchanged installation.
No App/device was started or stopped, and no held Native was replaced.

### Actual negative results and limits

- Initial formatting check found new test assertion layout; targeted rustfmt fixed it.
- The first wrapper invocation failed at PowerShell parameter binding because -p
  was ambiguous. Explicit -CargoArgs fixed invocation; that attempt ran no tests.
- First actual native compilation found six direct epoch.timeline references in
  the included playback fixture. They now use the same original 100-Hz/1-second
  fixture timeline; assertions and product guards were preserved. The full affected
  compile/test sets above passed after that fix.
- Two exact rational arithmetic counterexamples reproduce fraction-bit loss when
  combining integer/fraction, and in the old noninteger-musical scalar projection
  at zero elapsed output. They are arithmetic evidence, not an engine or audio test.
  The compact projection does not resolve or conceal either conversion gap.

Evidence/scripts/logs: workspace test-artifacts/compact-master-projection-20261023.
One continuous independent NONAUTHOR final-diff review returned CLEAR for staged
tree `4eb8b06545265752c64bce37fb08a8e198b25686`, including actual receipts and hashes,
and explicitly covers this factual CLEAR receipt and final Stage-0 task checkbox.
The reviewer made no source changes or test/build/Git mutations.

Stage A's anchored numerical domain/algorithm/error/wrap/actual-PCM and reverse
scalar contract remains open. Stage B's original-custody live policy/map adoption,
32-lane freshness/atomicity/retry and actual feedback remain open. Global SYNC and
Realtime BPM are not complete. All original/extension/Human/Native/pitch/Key-Lock/
B2/music/device/resource/parent/final gates remain; STOP full Rust-port work/Slice8.

## Independent custody prerequisite, 2026-10-23

PadStartCustody now keeps the exact original CapturedPadStart and a separately
shared effective MappedStartSource in one voice/retirement payload. Existing
successful ordinary/global mapped births share their original immutable map;
scalar births still have no mapped custody. Live geometry and witnesses use the
effective reference, while original admission ownership/content checks remain
explicitly on the original launch. Pending start Work and MappedNativeInput retain
their original geometry. There is no replacement setter, converter or live policy
adoption in this prerequisite, and no existing admission guard is opened.

The existing PadStartTarget entry retires the whole aggregate. Raw captured-target
cleanup wraps only the original with no effective map. Reservations remain one
entry for the aggregate, in addition to any independently required PCM entry.
Move/Swap and capacity refusal preserve the preceding lane and custody.
Stop/retrigger transfer the preceding complete aggregate to retirement; a
successful retrigger attaches its newly claimed launch and effective map. Final
destruction remains on the existing worker or close path; the callback does not
release a final map owner.

### Fresh verification

The same optimized locked wrapper and shared environment/cache were used. Native
selection sets below are disjoint. Filtered analysis-crate zero-test output and
the initial failed run add no passing tests to the final count.

| Affected native set | Result |
| --- | --- |
| mapped_launch_custody_tests, including mapped birth/global/Hold/Pause neighbors | 52 PASS, 0 FAIL, 3 existing IGNORE; compile 2m39s, tests 99.93s |
| audio_engine::buffer_retirement::tests | 2 PASS, 0 FAIL/IGNORE |
| source_callback_close_tests | 2 PASS, 0 FAIL/IGNORE; 0.19s |

Total: **56 distinct native PASS**, including four new real-route tests: raw
target cleanup, Stop with an actually paused retirement worker and final Weak
release, Move/Swap preserving the same voice/custody, and retrigger refusal at
exhausted retirement capacity followed by a complete successful claim. Existing
global and refusal assertions also check the original owner and effective Arc.
The three ignored Native-output cases are retained gates, not passing evidence.
Constant fixture PCM proves addressing/lifetime, not musical or acoustic quality.

Release all-targets Clippy PASS, 39.90s; the same existing 96 lib / 131 lib-test
(56 duplicates) / 196 example warnings remain. Warning sites in touched paths
refer to unchanged old fields/methods/test bodies; no lint suppression was added.
Cargo fmt, five included-test rustfmt checks, strict OpenSpec and whitespace
checks PASS. Source-compatible earlier Python and transport/clock evidence is
reused explicitly; it is not counted as freshly rerun here.

One normal private Release wheel built in 38.97s (39 existing lib warnings).
Fresh private-origin import/profile and all 13 existing Portable API checks PASS,
including editor API4 and ordinary transport. Private Native: 34,438,144 B,
SHA256 `ff4bb07ff4846eba99178c4498a00a80ed3c146fd5903ab531d0812a996d461d`.
It was not installed. Actual installed Native is still byte-identical at
34,446,848 B / `91881c187435c0bad0e2c6382a901f7af5c6585caf3665e0da1b6928ac35e6ab`.
Canonical start.bat --check PASS applies to that explicitly unchanged installation;
it did not start the App. No App/device was started or stopped or held Native replaced.

### Actual negative result and remaining limits

The first default-parallel mapped run compiled successfully but ended with
51 PASS / 1 FAIL / 3 IGNORE, 106.26s. The existing actual-Python waveform/Hold test
hit importlib's module-lock DeadlockError when parallel fixtures imported different
controller leaves in the shared embedded interpreter. LaunchPath now completes
the existing controller package entry before those fixture operations. The exact
same full selection passed with default parallelism as reported above. Assertions,
production code, admission guards and ignored cases were not changed for this fix.
Both failed and successful logs are retained in workspace
test-artifacts/effective-map-custody-20261023.

One continuous independent NONAUTHOR final-diff review returned CLEAR for staged
tree `cb405868ccc23cfd7707238842c2131acbb71957`, explicitly covering this factual
receipt and the last custody-prerequisite checkbox. The reviewer stayed read-only.
It also corrected an overbroad custody sentence: Stop retires the preceding
aggregate, while a successful retrigger attaches newly claimed custody.
Numeric conversion, replacement custody, actual live
policy/map adoption, complete 32-lane freshness/atomicity/retry and feedback remain
open. Every FULL-Goal Human/Native/pitch/Key-Lock/B2/music/device/resource/parent/
final gate and STOP before full Rust-port work/Slice8 remains in force.

## Independent physical scalar coordinate prerequisite, 2026-10-23

SourcePlayback has an explicit PairPhysical projection mode and a validated
coordinates-only scalar_continuation constructor. Exact u64 elapsed frames times
the binary64 rate significand fit in u128; integer displacement is reduced before
fraction addition and absolute source start is added using integers. Generic zero
fractional displacement preserves tiny and signed-zero fraction bits on every
whole-loop revisit. The arithmetic circular error bound is <=2^-53 source frame
per constant-rate epoch, not an absolute wrap-coordinate bound or an ideal real
trajectory across multiple rounded glide rebases.

This accepts only a canonical Normal pair in a genuine physical domain with
musical_period=None and finite rates [.5,2]. Musical domains, including an equal
integer musical period, are refused. Domain changes refuse before mutation;
same-domain refresh is exact. Explicit Seek starts a new legacy trajectory.
Earlier/shifted history is unavailable; constant in-epoch rewind is retained.
Comparisons include projection mode. Offset, advance, checkpoint and reader
preflight reject u64 exhaustion before mutation/output. Legacy constructors,
musical projection, reader arithmetic and saturation remain unchanged. No live
caller, conversion, source permit or policy-adoption path is opened.

### Fresh verification

Optimized locked wrapper, the same shared environment/cache and disjoint native
selection sets were used. Initial/repeated runs are not added to the final total;
filtered analysis-crate zero-test output adds nothing.

| Affected native set | Result |
| --- | --- |
| audio_engine::source_playback | 27 PASS, 0 FAIL/IGNORE; final compile 2m36s, tests <0.01s |
| source_feed_history::tests | 15 PASS, 0 FAIL, 1 existing resource-profile IGNORE |
| prepared_native_history::tests | 7 PASS, 0 FAIL/IGNORE; 18.74s |
| source_reader::tests | 17 PASS, 0 FAIL/IGNORE |
| mapped_source_clock::tests, including copied playback | 21 PASS, 0 FAIL/IGNORE |

Total: **87 distinct native PASS**, including ten new physical-pair tests on this
64-bit target. They cover invalid pairs/rates/domains, signed-zero/subnormal/tiny
fractions, independent analytical full-u64 dyadic cases and giant source offsets
without giant allocation; actual physical reader taps/PCM at anchor and distant
revisits; exact history/mode/overflow refusal; real span slices/checkpoints and
the .05/512 glide with unequal partitions and identical executed PCM bits.
The prepared-history neighbor includes the existing actual Native FIFO/independent
PCM comparison through 1,000 musical cycles; this is bounded software evidence.
The ignored host-specific resource profile remains a separate gate.

Measured layout is unchanged: projection 32 B, mapped epoch 88 B, mapped span
176 B (<=240), SourcePlayback 288 B, optional 32-lane copy 9,216 B (<=16 KiB),
checkpoint 448 B and source-feed span 1,264 B. Copy/no-drop checks PASS. These
measurements do not establish whole-process RSS, callback budget or 216-PAD limits.

Final all-targets Release Clippy PASS, 34.96s: 97 lib / 131 lib-test (56 duplicate)
/ 197 example warnings. The intentionally unconnected constructor joins the old
unused project_mapped_to message, and its validator adds one unused-function
warning. No suppression was added; this debt remains until its actual integration.
Cargo fmt, both explicit Rust2024 source/test checks, strict OpenSpec and whitespace
checks PASS. Compatible existing Python/launch evidence is retained, not counted
as freshly rerun here.

Normal private Release build PASS, 38.98s, 40 warnings (39 old plus the unused
validator). Fresh private-origin/profile import and all 13 Portable API checks,
including editor API4 and ordinary transport, PASS. Private Native is 34,448,384 B,
SHA256 `43d9af0e1262b68f85c9e549a8a928a3d49c859eb831a90b0f972f5647aab67f`.
It was not installed. Actual installed Native remains byte-identical at 34,446,848 B
/ `91881c187435c0bad0e2c6382a901f7af5c6585caf3665e0da1b6928ac35e6ab` before/after.
Canonical start.bat --check PASS applies to that explicitly old installation; it
did not start the App. No App/device was started/stopped or held Native replaced.

### Review findings, intermediate results and limits

The first 27-test run passed. Independent review then found two new-mode API
edges that its initial assertions did not cover: clearing an already-Normal seek
rebased the immutable epoch, and the inactive final-glide timer prevented exact
constant rewind/replay. PairPhysical clear is now a no-op; its inactive timer is
canonical zero on reaching target. Legacy's existing timer remains unchanged.
Stronger assertions verify clear's exact full state, actual PCM across partitions,
and history PCM plus full-state rewind/replay. The rerun passed 27 tests.

First Clippy PASS 39.60s also reported a new range-loop warning in the expanded
PCM trace test. Iterating its exact frames-sized buffer with enumerate fixed that
warning without changing sampled output or assertions. Final 27 tests and final
Clippy above passed. No failed test/RED result is claimed for these review fixes.
All intermediate logs are retained in workspace
test-artifacts/pair-physical-continuation-20261023.

One continuous independent NONAUTHOR final-diff review returned CLEAR for staged
tree `58ea3c1dcf9b1c754cbdf4e350abc6e83b8ed227`, explicitly covering this factual
receipt and the last physical-prerequisite checkbox. The reviewer stayed read-only.
General musical scalar/reader semantics, anchored mapped
conversion and actual policy/map adoption remain open. Existing exact source/PCM,
ownership, all-lane atomicity, retry and full raw-revision requirements are retained.
Every FULL-Goal Human/Native/pitch/Key-Lock/B2/music/device/resource/parent/final gate
and STOP before full Rust-port work/Slice8 remains in force.

## Stage S scalar policy adoption — 2026-10-23

Baseline is published `498b5497`. Stage S adopts only scalar policy requests and
targets on the unchanged scalar projection/reader; all representation conversions
and general mapped admission retain the open Stage A/B prerequisites. Root owns
checks/build/install/Git; coordinated Native and Python authors use disjoint files.
Evidence is retained in workspace test-artifacts/scalar-live-policy-20261023.

Python publication/cache-hit hook, strict current-attempt feedback, fixed human
guidance, public type stubs and Portable/launcher no-argument reflection gate are
authored. Initial combined affected Python selection: **894 PASS**, 41.41s,
covering InputMapping, source-sync/global-policy controllers, GUI/MIDI routes,
source-map/global renderers, intent persistence, pitch/global-rate neighbors,
Portable APIs and Windows launcher routing. These ran with the old installed
Native and existing test doubles; they establish Python/control boundaries only.
Earlier 54/299/4 passing subsets overlap and are not added to this total.

Ruff passes for all 15 changed Python/stub files. Mypy passes for the six affected
production/stub paths. Initial Phase 2 Ruff reported parser complexity/return and
line-length findings plus an oversized UI test; helpers and separate full-editor
scenario preserve all validation/oracles without suppressions. Three formatting
findings were corrected. The new stub also had one corrected formatting finding.
Initial Portable membership/Phase 1 assertion lint findings and final passing
results remain in the logs.

Final installed-Release Python evidence is **968 distinct PASS**: 766 combined
controller/input/UI/intent/pitch/rate/Portable/launcher cases in 42.17s, plus 202
disjoint complete Portable resident/runtime-preflight cases in 1.25s. The earlier
894 and focused 48 overlap these selections and are not added. These are control/
API proofs; actual callback/audio behavior has separate Native evidence below.

### Native integration and resolved failures

The first complete serial affected selection passed **334 distinct Native tests**, zero
failures or ignores: 336 selected passes across 15 sets, with two overlaps removed.
This includes 32 runtime-policy tests (30 routes and two counter edges), global
rate 9, input mapping 129, binding 9, source-sync neighbors 13, pitch controller 3,
remap 4, custody 4, pair 52, Hold 32, ordinary transport 18, reader 17, scheduler
10, retirement 2 and callback close 2. Earlier repeated runs are not added.

Final compatible Native evidence totals **382 distinct PASS**, 392 selected passes
across 16 receipts with ten overlaps removed and zero failures. The final 36-test
runtime-policy selection replaces its earlier 32-test receipt; its compile took
2m36s and execution 42.42s. The complete mapped-launch-custody neighbor suite adds
52 PASS / 3 existing diagnostic IGNORE in 98.36s, including actual map feedback,
Hold and ordinary-continuation neighbors. Its unchanged ignored profiles concern
bounded aligned/native-output measurements and do not close their acoustic gates.
The other 14 selections retain compatible unchanged-source evidence rather than
claiming that every test was rerun after the final feedback and dispatch-owner
corrections.

Actual generated-source public runtime/callback tests cover scalar directions,
exact E/reader taps/PCM, unequal partitions, existing pending glide, complete raw
away/back revisions, both genuine start modes, 32-lane atomic refusal, source and
resident-window invalidation before claim, Move/Swap, caused Hold release, Stop/
retrigger, scheduler pressure/order exhaustion, retirement pressure and callback
close with the cached snapshot still strongly retained. Actual public resident
control admitted before the first preflight is also rejected through its original
content-cell fence, including the repeated check under the original claim.

First production compile found a private type path and an exhaustive retirement
test helper; both wiring errors were fixed. First execution: 18 PASS / 5 FAIL,
20.66s after 2m42s compile. Its real Hold failure exposed raw-request publication
advancing the caused Hold transport identity. The narrow request-only predicate
now retains that identity for scalar and mapped Holds while original launch/runtime
fences still invalidate future starts. Mixed source/window/loop/pitch/map changes
and counter exhaustion keep the old supersession/refusal. A second run had 27 PASS /
4 FAIL, 35.18s: the Hold predicate still compared historical layout revisions,
and two resident fixtures mistook preparation proposals for actual admission.
The predicate now uses existing semantic binding projection; fixtures wait for
the real worker-published resident fence. Final 31-test rerun passed, then the
additional already-admitted-resident guard/test and full 334-test selection passed.

The other first-run failures were exact fixture/setup errors: established BPM to
period to master arithmetic produces 1.2500000000000002 rather than literal 1.25;
a manual ACK7 cannot qualify an accepted-none binding; the stopped-only mode setter
needed the actual empty control drain. Tests now use the original operation order,
authorized manual ACK0 recovery and real stopped drain. No tolerance, admission
guard, failure criterion or effective arithmetic was weakened. The ACK cache test
proves independent manual-fence recapture/stable reuse, not accepted-timing-ready
integration. Initial negative results are retained.

All-target Release Clippy initially failed with E0599: the newly reused reader
preflight called an existing gains helper still restricted to test builds. Removing
only that restriction preserves its existing one-line arithmetic and the expanded
test-profile code; the 334-test evidence remains compatible. Final all-target
Release Clippy passed in 37.76s with existing warning debt and no suppressions.

The final review then confirmed an old-finalizer/new-attempt race: dispatch owners
reached zero before matching pending cleanup, permitting the old destructor to
overwrite a new attempt. A deterministic barrier test reproduced it. The existing
counter now includes one cleanup reservation: begin uses two, the final Work drop
leaves one through cleanup, then releases zero. This is constant work and adds no
field, queue, allocation or retirement slot. A real scheduler-derived three-Work
test verifies nonfinal releases, final pending cleanup and exact cached retry.

Feedback follow-up Python tests first reported 15 FAIL / 33 PASS, then all 48
passed in 2.63s after exposing/validating the existing optional target-current bit
and giving fully matched active scalar settlement precedence. Three scalar modes,
missing/false/malformed bits, every raw request mismatch, source fences, stopped
voices and mapped exclusion are covered; no settled/effective ACK is fabricated.
The Native ordinary-glide test reproduced the missing getter field. Its additional
Native/pitch fixture initially lacked a production SourceACK; after using existing
real LoadSample/SourcePublication helpers it exposed a missing actual transport
master in that bare fixture. The fixture now uses the real public combined rate/
master publication and parameter drain. Neither source nor master guard changed.
The subsequent 35-test run had 33 PASS / 2 FAIL, 41.70s: the reproduced race and
that exact missing-master-before-unsupported fixture expectation. The final bias
fix, three-owner test and both corrected actual feedback cases pass in the final
36-test selection, including the pending-close/cache-strong/cache-replaced,
scheduler failure, retirement pressure and zero-allocation/deallocation cases.

### Delivery checks and scope limits

Strict OpenSpec, Cargo fmt, explicit Rust2024 formatting for all 23 changed Rust
files, Ruff/format for all 15 changed Python/stub files, Mypy for six production/
stub paths and whitespace checks pass. Final all-target Release Clippy passes in
37.41s, retaining 97 lib / 131 lib-test (56 duplicate) / 197 example warnings.
The intentionally unused physical-continuation constructor/validator remain
unconnected; no suppression or conversion permit was added to silence them.

Normal locked private Release build passes in 37.81s with 40 production warnings.
Fresh private-origin/profile import, all 13 Portable API checks and editor API4
pass. Native is 34,838,016 B, SHA256
`3af872db756fe6f017fb0ac826f978902473e874c5a254f2462098ef53a0d209`.
The fresh 2026-10-23T02:27:59.8706729Z holder scan found zero candidates, holders
or uninspected relevant processes and the unchanged old installed hash. Normal
editable Release installation then passes (hot build 0.18s); fresh installed
origin/profile, all 13 APIs and editor API4 pass and its bytes/hash exactly match
the private wheel. Canonical start.bat --check passes without starting the App.
The earlier updated-preflight rejection of the old missing-method Native is
retained as an expected negative result. No App/device was started or stopped.

One continuous independent NONAUTHOR final-diff review returned CLEAR for staged
tree `92c34367a27c3c16bef495bd9547aae9d656270e`, covering the actual 43-file diff,
receipts and corrected regressions. Only this factual review line and the last
Stage S checkbox followed; their final staged tree receives the same reviewer's
brief consistency check before coherent commit and normal main push. Stage A
conversion and Stage B general mapped adoption stay open; the scalar envelope
does not discharge them.

Bounded zero-Rust-allocation/deallocation preflight evidence and u64 execution
coverage do not establish whole-process RSS, callback budget, 216-PAD or physical
device acceptance. No App/device/hearing result, representation conversion,
general mapped adoption, Native/pitch/Key-Lock acceptance or full-Goal gate is
claimed. All original music/B2/resource/Human/parent/final gates remain open as
applicable, with STOP before full Rust port/Slice8.
