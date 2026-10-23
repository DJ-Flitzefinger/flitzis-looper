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

## Independent musical scalar coordinate prerequisite, 2026-10-23

`SourcePlayback::scalar_musical_continuation` now constructs a distinct PairMusical
trajectory for an already canonical separate Normal frame/fraction and the exact
original `Some(P)` domain. There is no production caller, conversion/adoption
permission, new reader, owning source work or alternate time/rate authority.
Existing Legacy and PairPhysical arithmetic remains unchanged. Shared pair guards
retain checked elapsed counts, complete copy comparisons, existing .05/512 glide,
unchanged-domain refresh, no-op Normal seek clearing and no invented prior history.

The admitted period is finite binary64 [.5,2^64], the physical region is valid,
exact |P-L|<=1 and both rates are [.5,2]. Separate q/f validation retains tiny
fractions and virtual physical-end cursors. Fixed H=2^53 exactly represents P and
rate; C=P*H<=2^117 and n*rate*H<2^118 fit u128 for complete u64 epochs. Reduction
precedes large integer anchor addition; exact seam comparison retains raw f.
Signed Euclidean splitting and TwoSum classify the exact fractional carry before
one canonical-fraction RN, including rounded-one/period equality before usize
conversion. Circular source error is <=2^-54 per constant epoch. Generic integer
displacement preserves raw fraction bits, including -0 and subnormals. This does
not establish a global ideal unrounded glide or representation-conversion bound.

### Fresh verification and limits

The first Release test build passes all 12 new tests (2m40s compile), including all
12 independently derived exact Fraction vectors. These cover P<L/=/>L, full-u64
epochs, binary64 1.16, >2^53 source offsets, tiny fractions, exact compatibility
at P2^64/LusizeMAX versus rejected LusizeMAX-1, canonical virtual ends, exact and
rounded carries, and two cases attaining the 2^-54 bound. Other new regressions
exercise unchanged actual taps/gains/PCM, the legacy narrow-seam formula, copied
SourceFeedSpan/SourceReadCheckpoint, unequal partitions, existing glide/history,
invalid domain/rate/Intro/Tail and overflow refusal before state/output mutation.

Eight current-source receipts have **144 distinct PASS**, 156 selected PASS
(12 new cases repeated in the whole playback suite), zero failures and one
pre-existing ignored source-feed-history resource profile. Selection: new12,
whole SourcePlayback39, SourceFeedHistory15+1ignored, PreparedNativeHistory10,
L3 reader6, SourceReader17, MappedSourceClock21 and RuntimePolicy36. RuntimePolicy
finishes42.67s; PreparedNativeHistory18.76s. No failed build/test was observed in
this block; invalid/refusal fixtures are actual passing negative cases, not a
claimed prior regression run. Copy/no-drop, mapped span<=240B and 32 optional
playbacks<=16KiB pass. No owning allocation or data-dependent traversal is added.

Cargo fmt, explicit Rust2024 checks on both files, strict OpenSpec and whitespace
checks pass. All-target Release Clippy passes39.74s with existing warning debt:
100lib/131lib-test (56duplicates)/200example; three additional dormant-prerequisite
warnings relative to StageS are retained, not suppressed. Private Release passes
37.81s (43production warnings), private origin/profile, all13 Portable API checks
and editor API4. Native is34,838,528B, SHA256
`20c36188c8a38c09f769999ae6c95243719561caf0ffa7146d741610f035a11a`.

No install was needed for an unused internal primitive with unchanged Python/API
surface. The installed StageS Native remains byte-identical34,838,016B/SHA
`3af872db756fe6f017fb0ac826f978902473e874c5a254f2462098ef53a0d209`.
Fresh canonical start.bat --check passes without opening the App. StageS Python
and installation evidence remains compatible for unchanged callers; its968PASS
is reused evidence, not a fresh Python run against this private wheel.

Receipts and the independent Fraction derivation are retained in
`test-artifacts/pair-musical-continuation-20261023` outside Git; own generated TMP
is47,355,007B before delivery cleanup. One existing cache/env/worker is retained
under the standing storage decision. Cleanup follows reviewed commit/push and
fresh clean actual-remote equality, terminal jobs and exact owner/path/process/
no-reparse checks; unrelated, denied and Human roots remain protected.

Primitive throughput and actual callback budget qualification remain open before
a productive conversion caller uses this coordinate mode. The anchored-map,
original-geometry, differing-reader-domain, first-PCM and actual live-conversion
gates remain OPEN. No whole-process resource, device/hearing, real model inference,
musical/Human or complete-Goal acceptance is claimed. STOP before Rust port/Slice8.

One continuous independent NONAUTHOR final-diff review returns CLEAR for exact
staged tree `e941af2351ee5a4f3a3db0dd67ae28fac50ca899`, including the six-file diff,
independently recomputed distinct test count, actual Release/import/preflight and
unchanged installed hash. Only this factual receipt and the final musical
prerequisite checkbox follow; the same reviewer checks their consistency before
coherent commit and normal existing-main push. General Stage A/B remain OPEN.

## Accepted scalar/map reader-domain boundary, 2026-10-23

The new test uses one actually loaded original cold source, 131072 generated mono
PCM frames at Fs48000, with original source/decoder custody. The existing explicitly
synthetic test-only BeatThis helper binds complete verified PCM, runs production
fit/acceptance/publication and obtains the actual callback ACK/current metadata.
It does not run inference. The actual accepted period in source frames is exactly
32768.25 (`40e0000800000000`), without constructing a replacement projection.

An independently completed and qualified original map artifact uses source anchors
frame0->beat0 and frame65536->beat2, truthfully sealed against that same original
source. Manual markers [32768,65536) derive Q1 and mapped reader period32768
(`40e0000000000000`). Public controls set speed.5 and master65536 output frames per
quarter. The source is started through actual capture/publish/scheduler/render,
stopped through the existing control, then started with mapped synchronization.
The original PCM Arc, source generation, content/lifetime admission and accepted
projection are preserved. The same master clock epoch remains unchanged.

Both successful source journals yield exactly `(65535, .5, Normal)` with identical
fraction bits `3fe0000000000000` and taps65535->32768. The original plans retain
different periods, and both exact plan/source-contract comparisons refuse equality:

| Actual journaled read | Right gain | Gain bits | Raw source PCM, both channels | PCM bits |
| --- | --- | --- | --- | --- |
| Scalar | .4 | 3ecccccd | .2 | 3e4ccccd |
| Mapped | .5 | 3f000000 | .25 | 3e800000 |

Local elapsed E is65535 in each separate launch. Scalar absolute/output-active E
is65535; mapped absolute/output-active E is131071. These counters coincide only
for this uninterrupted fixture. The physical slot's attempted-feed journal stays
monotonic across reuse; its ranges are0..65536 then65536..131072. The PCM values
above are original reader evaluation replayed from each actual successful journal
span, not captured final DSP output or an executed simultaneous conversion.

### Checks and preserved negative results

The first new test run fails on full SlotSnapshot equality after legitimate policy
publications: layout14->26, with owner/slot/cell/binding epoch/lifetime unchanged
(2m42s compile, .33s test). The fixture now uses the established same_projection
identity and explicitly asserts the changed layout; separate original-source,
generation, accepted ACK, map-currentness and exact PCM-Arc guards remain intact.
The second run fails on the fixture's incorrect reset-to-zero journal assumption
(2m35s compile, .36s test). Exact monotonic ranges and local/active/absolute times
are now asserted separately. No production guard, clock, reader or acceptance
criterion changed. Both failed receipts are retained as fixture errors.

The corrected unique test passes after2m37s compile/.35s execution. Five compatible
final receipts contain **170 distinct PASS**,171 selected (one overlap),0 final
failures and3 existing ignored mapped Native measurement/aligned-birth profiles.
Full map-admission/custody59PASS/3ignored finishes110.52s, timing89PASS10.38s,
content ACK5PASS and reader17PASS. The three ignored diagnostic/acoustic profiles
remain actual open gates; they were not disabled or newly ignored by this block.

Cargo fmt, explicit Rust2024, whitespace and strict OpenSpec pass. The first
OpenSpec command from the outer workspace reports Unknown item; rerunning from
the actual repo passes, with both command results retained. All-target Release
Clippy passes39.10s with unchanged warning debt100lib/131lib-test(56duplicates)/
200example; no warning suppression or production edit was added.

Only this existing cfg(test) file and official documentation change. Production
Release/import/all13 API/editor4 evidence from c49b898a remains compatible and is
reused; there is no new private build/install or claim of fresh Python validation.
Fresh start.bat --check passes without opening the App, and the actual installed
StageS Native remains byte-identical34,838,016B/SHA
`3af872db756fe6f017fb0ac826f978902473e874c5a254f2462098ef53a0d209`.

Receipts are in `test-artifacts/accepted-reader-domain-20261023` outside Git.
Own TMP before delivery cleanup is241files/347188B; Scratch1480->1480, zero new
entries. Only exact owned TMP may be cleaned after reviewed normal main push,
clean fresh actual-remote equality and terminal/path/process/no-reparse guards.
One existing compatible cache/env/worker remains retained; no full copies.

This closes a source-grounded reader-boundary regression, not conversion or a new
runtime refusal implementation. It confirms why matching q/f alone cannot waive
the original reader-domain/first-PCM contract. General Stage A/B, original geometry,
numerical/throughput/callback qualification, Native/music/B2/resource/Human/final
gates remain OPEN, with STOP before full Rust port/Slice8. No inference, device,
hearing, final-DSP-output or complete-Goal acceptance is claimed.

One continuous independent NONAUTHOR final-diff review returns CLEAR for exact
staged tree `60e14284b6acda23a728b550a9550ff70f82ec6a`, covering the five-file diff,
actual receipts, independently recomputed test count and retained fixture errors.
Only this factual receipt and the bounded reader-boundary checkbox follow; the
same reviewer checks their final consistency before coherent commit and normal
existing-main push. Conversion and all remaining acceptance gates stay OPEN.

## Checked exact numerical support and finite master horizon — 2026-10-23

After ae7602bf, the next bounded gap was reusable exact arithmetic, before any
geometry/conversion choice. The new private SourceInteger adapter uses pinned
bnum0.14.4/default-features=false, 800 inline bytes/6400 magnitude bits and checked
immutable sign-magnitude operations. It decodes finite f64 on G=2^1074, rejects
nonfinite/high-bit-loss/capacity/zero-divisor input, and keeps signed Euclidean
quotients without u64 narrowing. Rational splitting retains whole:u128, rounds
only its exact fraction once with ties-to-even, and refuses exact values above
2^64 before rounding. A fractional-one carry may produce the virtual 2^64
endpoint for later checked caller normalization. Numeric zero is +0; raw source
fraction identity, geometry, reader custody and physical addressing are separate.

The independent stdlib Fraction/struct oracle supplies20 exact vectors: large
whole parts, non-dyadic fractions, both even neighbors, zero/subnormal/normal
ties, fractional-one carry, full5304-bit denominators and 2^64 boundaries.
Accepted N is necessarily below2^5368 under the D/value bounds; the conservative
6363-bit input cap is not presented as a reachable accepted numerator. The
design retains the conditional original-knot algebra and capacity derivation
without choosing exact-seconds versus legacy geometry. Its splitter bound uses
r<D, then at most1074 shift bits, requiring at most6378 of6400 bits.

TransportBeatProjection now has a read-only finite horizon helper using exactly
the existing evaluator. Ordered finite endpoints bound its nondecreasing actual
projection; cast plateaus and intermediate overflow remain unchanged. An actual
Timeline with Fs8192 and valid period2^-1013 has M(2^24-1) finite but M(2^24)=Inf.
The helper refuses that longer horizon and full-u64 use. Tests also retain a
valid -MAX anchor whose actual intermediate division overflows, rather than
reassociating it into another clock. The projection stays32 bytes/Copy/no-drop.

Four final compatible receipts contain121 DISTINCT PASS, zero overlap/failure/
ignore: new Numerics15, whole Transport46 (six new), SourcePlayback39 and
MappedClock21. Initial Numerics15 passed after2m40 compile; final15 after2m44,
both0.00s. No software-test RED occurred. The Rust allocator scope observed zero
alloc/alloc_zeroed/realloc requests on its thread for the measured operations;
it does not observe deallocations, Native/C++ allocation or whole-process cost.
Fixed values are Copy/no-drop and no wide operand enters any lane/span.

CargoFMT/explicitRust2024/strict OpenSpec/diff checks pass. Initial all-target
Release Clippy37.36s passed but exposed one redundant constant test assertion;
only that assertion was removed, with actual zero-decode/cancellation/negation/
product checks retained. Final Clippy39.78s passes117lib/131lib-test(56duplicates)/
217example warnings. The17 additional production/example warnings identify
these dormant numerical prerequisites; existing warning debt is not hidden.

Private locked Release38.44s passes, Native34,838,528B/SHA256
`c3cead37a890a018c393eb4a21580becb7f78a451883b6616f3ef9104fdf3c0c`;
actual private origin/release profile/all13 Portable API checks/editor4 pass.
The later test-only assertion removal leaves that production evidence compatible.
Bnum adds only its locked package/direct edge, no mandatory dependencies, new
runtime DLL or bootstrap route; MSRV1.87 fits actual rustc1.95.0. Complete
Portable acceptance remains open. No installation or fresh whole-Python claim:
installed StageS Native remains34,838,016B/SHA3af872..., and fresh canonical
start.bat --check passes without App/device start. Existing unchanged-path
Python evidence is reused only within its compatible scope.

Receipts/oracle/repro live outside Git at
test-artifacts/bounded-source-numerics-20261023. Own TMP may be removed only after
reviewed coherent commit/normal main push/clean fresh actual-remote equality,
terminal jobs and exact ownership/path/process/no-reparse guards. One shared
compatible cache/env/worker is retained under the existing storage decision.
No productive converter/adoption caller, new clock or geometry decision follows.
Anchored first-pair/PCM, original knots/wraps, source/ACK/retirement, copied layout,
actual stack/callback throughput and all FULL-Goal Native/music/B2/resource/Human/
final gates remain OPEN; STOP before full Rust-port/Slice8.

One continuous independent NONAUTHOR final-diff review returns CLEAR on exact
staged tree `9a0ad01d61ad8e375385f15d0c7fbfc5e4c714e8`, independently confirming
the121 distinct/selected passes and actual Clippy/private Release/API/preflight
receipts. Only this factual receipt and the bounded validation checkbox follow;
the same reviewer verifies final consistency before commit/normal main push.
Own TMP is6files/47,354,535B; direct Scratch1480->1480, zero new entries. Cleanup
remains gated by publication/clean actual remote/terminal and ownership checks.

## Prepared original geometry/reader intersection — 2026-10-23

After e01a192f, the remaining gap was exact original loop closure and reader
compatibility, before an anchored trajectory. MappedLoopGeometry now supports
explicit consuming off-realtime qualification on the same original owner.
Legacy new only initializes the optional certificate to None; it performs no
new scan and keeps every projection/epoch/span operation unchanged. The private
Copy/no-drop certificate retains two original slope-extrema segments in the
owning geometry, never a copied span/lane or separate source/clock authority.

The borrowed original-segment visitor preserves interval-open coverage: only
positive-length overlap counts, with the original endpoint knots retained.
Qualification reuses the exact PairMusical domain guard, checks each used knot's
exact seconds*Fs against its existing rounded product, exact B+Q against the
captured legacy beat endpoint, and exact source closure at integer start and
start+original P. It does not clip/rescale/replace knots or change B/Q/P. The
cheap reader check requires complete original Some(P) domain equality. Exact
rational extrema/rate checks retain the original nominal epoch admission first;
they do not bypass its intermediate overflow or introduce a new derivative.

Independent stdlib Fraction/struct evidence supplies11 eligibility vectors;
the new table executes actual legacy constructor B/Q/P assertions for1–10.
Cases cover variable/offset/partial loops, P.75/P1.25 virtual extents, unused
inexact outer geometry, used inexact inner knots, lost B+Q and an isolated
non-dyadic endpoint closure. The differing-period boundary is additionally checked
on positively qualified geometry, preserving the prior actual-source regression.
The oracle's raw cursor pairs are not inputs to this qualifier: no cursor
conversion, raw-fraction preservation or first-PCM acceptance follows.

Two further independent exact-rate vectors establish valid legacy construction,
P1 and legacy epoch admission before refusal: rounded rate.5 hides exact
9007199254740992/18014398509481985<.5; rounded2 hides exact
36028797018963968/18014398509481983>2. Original products and closure are exact,
so these prove the additional rate guards rather than another geometry failure.

Quick Release typecheck passes27.54s. Before first test execution, source review
caught two new fixture labels using invalid shorthand gap; they were corrected
to the existing gap_unknown_count schema value without product/criterion changes.
Initial new12 PASS after2m37 compile/0.00s; no software-test RED occurred.
Four final compatible receipts contain193 DISTINCT/193selected PASS, no overlap:
MappedClock33 (21old+12new), SourceBeatmap62, SourcePlayback39 and actual map
admission/custody59 (108.53s). Three existing aligned/Native output measurement
profiles remain ignored and OPEN. An initial overly anchored custody filter
selected zero tests; the count guard rejected it, and the corrected actual-name
substring ran the59. Its empty receipt and initial12 duplicates are not passes
added to the final distinct total.

CargoFMT/explicitRust2024/strict OpenSpec/diff checks pass. All-target Release
Clippy37.94s passes126lib/131lib-test(56duplicates)/226example warnings; nine new
dormant production/example prerequisite warnings remain visible. Private locked
Release38.02s passes, Native34,838,528B/SHA256
`6624e47fd97e1b33f9c9f778bd8cd6e0ea20e7201081e8cbe3526fa06314d314`.
Actual private origin/release/all13 API checks/editor4 pass. No install or fresh
whole-Python claim: installed StageS Native remains34,838,016B/SHA3af872..., and
fresh canonical start.bat --check passes without App/device start. New fixtures
are explicitly synthetic geometry, not accepted source/custody/music evidence;
the affected existing runtime/source integration is a separate bounded proof.

Receipts/oracles/repro are outside Git at
test-artifacts/anchored-geometry-qualification-20261023. Own TMP239files/47,690,424B,
direct Scratch1480->1480, zero new entries. Cleanup follows reviewed coherent
commit/normal main push/clean fresh actual-remote equality, terminal jobs and
exact ownership/path/process/no-reparse guards. The existing shared compatible
cache/env/worker remains retained; no full copies or old/Human cleanup.

This closes opt-in numeric geometry qualification only. General geometry outside
the intersection, actual raw-anchor conversion, generic fraction identity,
first-PCM/reader/custody, spans/history/master transitions, copied layout and
actual stack/callback cost remain OPEN, as do full Stage A/B and all original/
extension/Native/music/B2/resource/Human/final gates. STOP before full Rust-port/
Slice8.

One continuous independent NONAUTHOR final-diff review returns CLEAR for exact
staged tree `0fae41843d7c38829604beca63103b847729927c`, independently verifying
the193 distinct/selected passes, three old ignored profiles and actual private
Release/API/Clippy/preflight receipts. Only this factual same-review receipt and
the bounded validation checkbox follow; the same reviewer checks final
consistency before coherent commit and normal existing-main push. All broader
conversion, geometry, callback-cost and acceptance gates remain OPEN.

## Certified anchored reference coordinates — 2026-10-23

This independent Stage A prerequisite adds a distinct copied anchored mode using
original qualified geometry, the complete original Some(P) reader domain, raw
Normal q/f, original inverse segment/E0 and cached actual M(E0). Exact original
phase/knot/wrap classification and the shared one-round fractional splitter retain
generic raw signed-zero/subnormal identities. No productive adoption caller is
connected. Legacy projection/reader/glide and source/custody/ACK authority remain.
Pause uses checked address translation; a changed master requires actual addressed
M-bit continuity, finite new horizon and a history floor exactly at actual E.
The replacement API rechecks those conditions, immutable anchor and mode itself.
Terminal spans allow observing MAX but refuse refresh/render there. Normalization
compares exact start*G+I(P) before any usize cast, including whole2^64.

Independent stdlib Fraction/struct evidence is retained in
`test-artifacts/anchored-coordinates-20261023/fraction-oracle.py` and `.md` outside
repo. Its35 table rows mean34 intended arithmetic evaluations and1 intentionally
wrong-M0 counterexample, not35 admitted Native cases. Initial FPB32000 master
witness was corrected BEFORE Native execution to supported FPB12000: slow rate.25
must refuse, while12000 gives2/3..2. Original M0 preserves40000/+0; recomputing it
would produce45333/fraction bits3fd55555555501ff. Tests retain the unsupported
rate refusal and original nominal/exact bounds.

Initial12-test Native run:11PASS/1FAIL, compile2m37, wrapper158.62s. Its terminal
fixture tried to anchor transport at MAX-10 and hit the existing downbeat-address
guard before constructing this mode. Only the fixture changed to an ordinary
valid frame0 master anchor; actual last-ten-frame cast plateaus still exercise
terminal MAX. Separately, independent final-source review found a genuine missing
changed-projection history-floor guard. Root repaired it after jobs terminated,
added a real SourcePlayback replacement/immutability regression, and reran the
complete affected clock suite. No existing transport guard or criterion relaxed.

Final post-repair source: **319 distinct /319 selected PASS, zero overlap**, four
OLD ignored profiles, across nine logs: MappedClock46 (33 preceding+13new),
SourcePlayback47 (39+8new), Numerics17 (15+2new), Beatmap62, Transport46,
SourceFeed15, PreparedNativeHistory10 (18.63s), Reader17 and Map/Custody59
(107.9s test/108.32s wrapper). Final clock compile2m35. The initial11 passes and
repeated isolated profile are excluded. Root's first summary guard expected419
through an addition error; the actual parser already returned319. Correcting that
expectation changed no Native result. Four ignored cases remain the existing
source-feed resource profile and three mapped Native/aligned measurement profiles.

Copy/noDrop and emitted Windows x64 sizes: projection32, epoch128, span232,
SourcePlayback344, SourceFeedSpan1320, checkpoint504, 32-lane copy11008 bytes.
Existing legacy history storage also grows: startup span payload82,837,504 to
86,507,520B (+3.5MiB), maximum165,675,008 to173,015,040B (+7MiB). These counts
exclude inline work, Native/PCM and allocator/Arc/ring overhead. Full resource
acceptance remains open. No wide operand or owning reference is stored in a lane.

Descriptive profiles: initial128 queries5.6777ms/16 refreshes5.0752ms; final
parallel-suite6.6871ms/5.2274ms; isolated one-test thread6.3011ms/5.2461ms.
Only the128 span queries were inside the allocation observer and requested zero
Rust allocations; the16 refreshes were timed outside that scope. The observer does
not count frees, Native/global allocation or stack high-water. These are reference-path
cost observations, NOT a callback deadline or RT qualification. The costly wide
path remains without a productive caller; actual stack/throughput/cohort costs
must close before enabling it for living voices.

Quick Release typecheck32.11s PASS; final CargoFMT/explicitRust2024/strict/diff PASS.
Release all-target Clippy37.02s PASS with108lib/131libtest(56dup)/208example warnings
(previous126/131/226; existing exact helpers are now referenced). Fresh private
Release38.76s compiler/39.80s wrapper PASS; actual extracted Native34,577,920B,
SHA25683edc804775168c450256053b174ff59d7020911f3bf7b4ef4e3beaf013686a5,
actual private origin/release/13 bootstrap API/editor4 PASS. No installation or
whole-Python rerun. Installed StageS34,838,016B/SHA3af872db... remained freshly
identical; canonical `start.bat --check` PASS without starting App/device.
Python/control/API evidence is reused only for unchanged inputs. Synthetic geometry
and checked original-reader PCM do not prove actual accepted source/custody or
public live conversion. General geometry, first-PCM authority, Stage A/B adoption,
Native/music/B2/resource/Human and all FULL-Goal final gates remain OPEN; STOP Slice8.

One continuous NONAUTHOR actual final-diff review returns CLEAR for staged tree
`042279e4dcb774dc30dbaa7bafd40d580dac9458`, auditing actual source and repaired
history regression, independently parsing319 unique passes and reading actual
resource/private Release/API/preflight receipts. Only this factual same-review
receipt and bounded validation checkbox follow, with final metadata consistency
checked by that same reviewer. No broader conversion/RT/full-Goal gate is closed.
After a checked coherent delivery, mandatory Conventional Commit and
normal existing-main push without individual approval; verify clean HEAD/tracking/
fresh actual remote before an independent next step. Preserve concise positive
and negative evidence, then clean only exact own TMP after terminal jobs and
ownership/path/no-reparse/process guards. One shared env/buildcache/worker remains.

## Exact numerical cost refinement — 2026-10-23

The existing 6400-bit checked algebra now shares exact zero/unit/symmetric
power-of-two multiplication across signed products, qD reconstruction and raw
fraction identity. General operands retain pinned bnum checked multiplication.
Finite binary64/u32 scaling multiplies the integer significand in u128 before
widening/shifting (maximum85 bits before shift,2130 after), retaining nonfinite
refusal before factor zero. Anchored phase requests the checked nonnegative
Euclidean remainder directly; bnum may still compute an internal quotient. The
source splitter retains shared div/rem, exact raw-fraction identity and single RN.
The original geometry qualifier still compares against its unchanged legacy
rounded product. No source/domain/history/first-PCM/adoption/RT guard was relaxed.

The independent retained-algorithm tests cover98,256 signed finite exponent/edge/
factor cases plus40 nonfinite cases,900 signed multiplication pairs and360
signed full-width division/remainder combinations. An independent stdlib Fraction
oracle confirms .1*10=1+2^-54 while the legacy binary64 product equals1; both the
numeric residual and existing geometry-intersection refusal remain tested.

Serial Release regressions:324 distinctPASS/324 selected/0overlap,4OLDignored
(one feed resource and three existing Native diagnostics). Numerics21,Clock47,
Playback47,Beatmap62,Transport46,Feed15,Prepared7+Retained3,Reader17,Custody59.
The initial Prepared selector selected direct tests only (7PASS); all3
retained-start neighbors then ran separately, without duplicate counting.
No failed functional test. Fresh fmt/strict/diffPASS; Release all-target Clippy
39.73sPASS with109lib/131libtest(56duplicates)/209example warnings: one additional
unused signed-div/rem support method after phase switched to remainder; no
suppression or admission change. Full Python/device/music/Human gates not claimed.

Isolated descriptive before/after values (ms,128 queries or16 refreshes):

| Shape | Queries before | Queries after | Refresh before | Refresh after |
| --- | ---: | ---: | ---: | ---: |
| Unchanged minsub |5.2905|1.8856|5.1819|3.7831|
| Near one |5.2864|2.2153|5.1599|3.9761|
| Negative zero |5.0065|1.8683|5.0737|3.7803|
| Virtual period |2.2673|0.4236|4.7971|3.7050|
| Large epoch |5.2629|1.9044|5.1646|3.8946|

128 general products: sparse277.5→281.5us; dense281.4→287.2us. Thus general
multiplication did not improve in this run. Same Release profile/input/count,
one test thread, no concurrent owned Native job during isolated profiles. These
single-run host timings are descriptive, not a statistical/worstcase guarantee.
Only query/product thread Rust allocation requests were observed (zero); refresh,
frees, Native/global allocators, actual stack, callbacks and32-lane deadlines are
NOT qualified. Current exact coordinate path remains without productive RT caller.

Artifacts: workspace/test-artifacts/exact-numerics-cost-20261023, including retained
before/after logs, oracle, complete distinct-test summary and actual checks. Root
observed installed Native34577920B/SHA1cd2c058d8d5298e60fe2bdabdc3922b08d0aa7ef34b0ed25c21691d41695c1c,
mtime07:43:30UTC BEFORE this chat's08:14+ activation, differing from the historic
StageS handoff snapshot. Fresh local baseline controls this block; no installation
or App/device action was performed. Private Release/import/final review/publication
receipts follow; entire FULL scope and STOP before full Rustport/Slice8 remain.

Private Release37.79sPASS (52 production build warnings),34577408B/SHA256
fb08f148ebe7d6465c5b23a14bcba7f67b0e46807433c194638c518a49b9fd3e. Fresh isolated
private-origin/release/13API/editor4 checksPASS; NOT installed. Canonical
start.bat --checkPASS/noApp on the freshly observed existing installation;
installed-after independently equals installed-before34577920B/1cd2c058…,
mtime07:43:30UTC predating this chat. OwnTMP6files46998968B (<128MiB), allNativejobs
and numeric authorTERMINAL. Source-compatible prior Python integration reused;
no full Python rerun, installation or device/hearing acceptance claimed.

ONE continuous independent NONAUTHOR final source/results/meta review CLEAR;
raw logs and exact counts independently checked. Documentation-only final diff
check initially detected two appended CR line endings; normalized to the existing
LF format and final diff/strict checks passed. No runtime source changed after
the successful tests/build. Delivery census (all regular including hidden, no
reparse traversal) before own TMP cleanup: test-artifacts17841042794B/454692files,
env3249696864B/21323files, target16022538990B/10749files. Retain ONE compatible
shared env/cache/worker under the existing >30GiB decision; only new exact own TMP
46998968B is disposable after reviewed publication/clean fresh remote/terminal/
ownership/path/no-reparse/process guards. Human/unknown/denied/old data protected.

## Actual copied-reader cohort cost evidence — 2026-10-23

This tests-only block changes no production behavior, source/reader/domain guard,
adoption caller or RT eligibility. The new test uses32 independent copied
playbacks and shared immutable stereo PCM, preallocated scratch/output storage,
real chunk/refresh, separate mixer-style endpoint preflight and copied advance,
and the original full or checked two-pass reader. Every route renders512frames
per lane. Complete full/checked PCM, final copied state and scalar first-PCM
comparisons pass outside timing. Zero-chunk refresh must immediately yield a
positive consuming chunk; it cannot hang. Scalar first-PCM checks use the same
unchanged reader with original domain/raw pair; they do not grant source custody.

One isolated Release run,32lanes/16384cohort frames, nominal512/48000=10.6667ms:

| Shape | Full reader ms | Checked reader ms | Chunks per route | Refreshes per route |
| --- | ---: | ---: | ---: | ---: |
| minsub |260.6119|518.2424|32|0|
| near one |298.0743|594.2045|32|0|
| negative zero |257.3028|513.7272|32|0|
| large epoch |259.2630|523.8033|32|0|
| aligned knot |433.9083|854.5548|64|32|
| virtual aligned |2430.8454|2694.4045|8416|8384|
| virtual staggered |2488.1280|2773.6272|8404|8372|

The observed times are24.122–260.028 times that nominal interval. Thus these
actual software cases do NOT qualify the exact path for productive callback use.
They are not a statistical worstcase or actual device test. Full reader requests
16384coordinates, checked reader32768, plus two explicit endpoint calls per
chunk; refresh-internal coordinate evaluations are additional and not counted.
Timing includes PCM copying/bookkeeping and original span refresh, excludes
setup/verification, actual mixer/DSP/device scheduling. Only observed rendering
thread Rust allocation requests were zero; frees, Native/global allocators and
whole process resources remain unqualified. No timer threshold was weakened or
functional RED hidden: the test checks exact behavior and reports the negative
budget result descriptively.

Fresh tests: new profile1PASS/14.96s plus8existing anchored regressionsPASS/0.01s,
9distinct/0overlap/0ignored. Release compilation2m53; fmt/strict/diffPASS and
all-target Release Clippy40.57sPASS, unchanged109lib/131libtest(56duplicates)/
209example warnings. Earlier bfacd1e2 production324-regression/privateRelease/
13API/editor4 evidence is source-compatible and reused, not rerun or relabeled
as fresh. No new production Release, installation, App or device action.

Read-only MSVC14.51.36231 dumpbin extraction retained named unwind records from
the FINAL rebuilt Release test EXE, without executing/loading the image:
EXE87909376B/SHA256c3f9bd1304792f5a227d4e2f5d316d2dd7287b9e6757085c28982eb35dc69061;
PDB29814784B/SHA256191769e4dbf7b9c00c884d0359d9c4f131d8014a2d218cdf0090293028f6eaec.
The header reports the exact hashed adjacent PDB as resolved, CodeViewGUID
4BCE41E8-5601-40F4-8F3E-D9130CEB697D/Age8. Local symbol search only; input/tool
hashes were equal before/after extraction.95matching named records retained;
45unsupported flags/structures stay explicitly unresolved, not assigned zero.

| Named function | Fixed allocation + pushes, bytes |
| --- | ---: |
| MappedSourceSpan::position_at |14568|
| MappedLoopGeometry::anchored_position_at |14600|
| SourceInteger::checked_add |6472|
| SourceInteger::checked_mul |1672|
| SourceInteger::rem_euclid |4856|
| checked_magnitude_mul |1752|
| split_source_coordinate_impl |9704|
| SourcePlayback::refresh_mapped_span |952|

These are individual fixed prologue adjustments, excluding entry return addresses
and transitive callees; they are NOT a peak callchain bound. SAVE operations do
not further adjust RSP and are not added. Anonymous, chained/dynamic and external
callgraph gaps remain open. Interpretation follows Microsoft's
[x64 unwind format](https://learn.microsoft.com/en-us/cpp/build/exception-handling-x64?view=msvc-170)
and [stack usage](https://learn.microsoft.com/en-us/cpp/build/stack-usage?view=msvc-170).
No anonymous function was inferred from a neighboring symbol. Inlining belongs
to its compiled container; this test build is not callback-stack qualification.

Artifacts: workspace/test-artifacts/anchored-cohort-cost-20261023 with OWNER,
actual raw profile logs, exact image headers, matching raw unwind records,
checked extractor and binary/tool identities. No binary/environment/cache copy
or temporary tree was created. Actual peak-stack/callback cost, Source/FirstPCM/
StageA/B/live adoption and ALL original/extension/Human/final gates remain open.

ONE continuous independent NONAUTHOR source/evidence review CLEAR, including all
raw14 timing rows,9distinct tests, exact resolved EXE/PDB identities and all8named
fixed-stack adjustments. Only the completed profiling task is checked; callback
eligibility and maximum stack are still open. Delivery all-regular/hidden census:
test-artifacts17794479570B/454702files, env3249696864B/21323files,
target16022043525B/10749files,0reparse traversed. Keep the existing ONE compatible
shared env/cache/worker under the prior >30GiB decision; no new disposable TMP,
binary copies or isolated environment existed to clean. Prior/Human/unknown/
denied data and installations remain protected.

## Exact native odd-coefficient multiplication — 2026-10-23

The shared magnitude helper additionally factors A=a*2^s and B=b*2^t. Only when
both odd coefficients and their product fit losslessly in u128 does it rebuild
the exact result through the existing lossless6400-bit shift. Wider coefficients,
conversion misses and native product overflow retain the original checked bnum
fallback. Thus native overflow is NOT a new arithmetic refusal. The successful
native reconstruction has exactly the original criterion
bit_width(a*b)+s+t<=6400. Existing zero/unit/power-of-two paths, sign/canonical-zero
handling, splitter/RN/raw fraction/domain/first-PCM/custody/history/master guards
and APIs/layouts remain unchanged. No productive caller or RT permit is added.

Two new tests exercise25 product pairs, both orders and all signs:200 direct
pinned-bnum differential comparisons with zero observed product-thread Rust
requests. Independent Python arbitrary-integer vectors include
(2^64−1)*(2^64+1)=2^128−1 at combined shift6272 (6400bits admitted), shift6273
(6401bits refused), native129-bit coefficient overflow with2277-bit wide success,
and the1075-bit odd coefficient of1+minsub with2157-bit wide product. Word/scale,
huge odd, summed-shift exhaustion and dense fallback cases remain covered.

Fresh327distinctPASS/327selected/0overlap/4OLDignored. Numerics23, isolated
profiles2, isolated actualcohort1, Clock45, Playback47, Beatmap62, Transport46,
Feed15, Prepared10, Reader17, Custody59. Integrated filters skip the separately
executed profile tests instead of rerunning/counting them twice. Numerics compile
2m47/PASS0.13s; actualcohort1PASS6.65s. FMT/strict/diffPASS; Release all-target
Clippy38.61sPASS with unchanged109lib/131libtest(56duplicates)/209example warnings.
No failed functional test or weakened guard/timing assertion.

Unchanged isolated profiles before/after (ms,128queries or16refreshes):

| Shape | Queries before | Queries after | Refresh before | Refresh after |
| --- | ---: | ---: | ---: | ---: |
| minsub |1.9608|1.2541|3.7937|1.4858|
| near one |2.3673|1.1078|4.8216|1.4863|
| negative zero |1.9894|1.0067|4.0746|1.3988|
| virtual |0.4376|0.4293|3.8754|1.3778|
| large epoch |1.9575|1.2725|4.0263|1.5315|

Generic128products sparse288→293.3us and dense293.2→297.8us were slightly slower
in these runs. This fallback tradeoff is retained rather than claiming a universal
gain. Full32-lane/stereo512-frame profile comparison against unchanged831f42a3:

| Shape | Full before ms | Full after ms | Checked before ms | Checked after ms |
| --- | ---: | ---: | ---: | ---: |
| minsub |260.6119|165.6068|518.2424|337.0970|
| near one |298.0743|145.8277|594.2045|294.9381|
| negative zero |257.3028|132.9471|513.7272|267.3729|
| large epoch |259.2630|167.6932|523.8033|333.7094|
| aligned knot |433.9083|169.8243|854.5548|332.9825|
| virtual aligned |2430.8454|944.8021|2694.4045|1186.3257|
| virtual staggered |2488.1280|994.1581|2773.6272|1147.0000|

Reader/endpoint request counts and actual chunks/refreshes are unchanged. These
single-run host figures remain descriptive and still cost12.464–111.218 times
the nominal10.6667ms interval; callback eligibility remains NEGATIVE. Only query,
product or render thread Rust requests were observed as zero, not refresh frees,
Native/global allocators, process resources, device scheduling or worstcase time.

Re-extracted FINAL test EXE87970304B/SHA256
9d2ccd97b0fa892008735e1d7258ea574fafd3589396633c5848eacff757b514;
resolved adjacent PDB29831168B/SHA256
c4b82efc2ed38136fd5d726344ddfddbe483a885c2b3e5c92f4693a4c010a2d4.
Named fixed-prologue facts: the multiplication helper grows1752→4168bytes
(+2416), while span14568/anchored14600/add6472/mul1672/rem4856/split9704/refresh952
stay unchanged. These exclude return addresses/transitive callees; no overall
peak-stack improvement or callback-stack permission is claimed.97matching named
records/45unsupported structures remain explicit; no anonymous inference or
zero-frame assumption. Exact header/GUID/age/tool/hash/raw-record receipts are
retained under workspace/test-artifacts/native-odd-products-20261023.

Private Release38.75sPASS/52existing build warnings, actual private-origin Native
34578944B/SHA256d739573cebcf12acf94ee1c4717c9f89abb2e7101c1a5a74a997bed1a37f0dd8;
fresh private release/API13/editor4PASS. NOT installed; canonical start.bat
--checkPASS/noApp on the existing installation. Whole Python/device/music/Human
acceptance is not claimed. Actual peak-stack/callback cost, original geometry/
Source/FirstPCM/StageA/B/live adoption and every FULL parent/final gate remain open.

One continuous independent NONAUTHOR final review CLEAR on the actual code/spec/
evidence diff, including all327 distinct raw test receipts, negative callback
timings, generic slowdowns and increased helper frame. Only the completed support
optimization checkbox is closed. Delivery census (all regular including hidden,
no reparse traversal): artifacts17,842,151,777B/454738files; shared environment
3,249,696,864B/21323files; target16,022,125,294B/10749files. Retain ONE compatible
environment/cache/worker under the existing >30GiB decision, without copies.
Only exact owned tmp6files/47,000,878B is disposable after reviewed normal push,
clean fresh actual remote equality and terminal/path/reparse/process guards.
Installed Native remains the unchanged fresh1cd2c058 baseline; Human/private/
unknown/denied data, real installations and the shared environment/cache stay
protected. Complete positive/negative/reproduction/limit receipts are retained.
