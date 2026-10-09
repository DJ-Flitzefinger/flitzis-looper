# Serial delivery and measurable acceptance

All steps operate on the current verified hybrid app; only the coordinator starts
successor chats. P0 is planning. P1-P6 are pending implementation, not a backlog
invitation or permission to port the full app. Do not run GUI/CPAL/devices/recorders;
human operation/listening and required corrections remain the final pre-port phase.

## Common exit gate for each implementation slice

- [ ] Freeze actual source, runtime/PYD/native EXE and fixture/model identities used
  by that slice; retain actual argv/cwd/logs/exit and all failed attempts.
- [ ] Use genuine hardware-free controller/native admission, worker, command-drain
  and render tests. Select relevant tests listed below; full validation for runtime/
  persistence/shared audio changes: `uv sync`, `uv run maturin develop`,
  `uv run cargo check --manifest-path rust/Cargo.toml`,
  `uv run cargo test --manifest-path rust/Cargo.toml`, `uv run pytest`,
  `uv run ruff check src`, `uv run mypy src`, and format checks. Serialize Windows
  builds/tests; use the maintained Rust test harness where applicable. Debug/Release
  and installed PYD are separate roles; do not transfer prior acceptance to a new build.
- [ ] Official strict validation for this change and each other actually touched
  active change; doc/diff/link/identity/ownership checks and private preservation.
- [ ] Complete independent nonauthor semantic review of all final changed source/
  tests/specs/docs, plus raw/canonical/index/blob/tree review before commit.
- [ ] Coherent Conventional Commit, authorized normal publication, fresh clean
  main/HEAD=origin/main=network main proof and coordinator bounded acceptance.
  Keep entire remaining program and original gates in the absolute local handoff.

## P0: Official bounded plan (this chat)

- [x] Read actual source/contracts and prior root clickfix acceptance; preserve its
  separate completed engineering scope and all original gates.
- [x] Define final pad-owned layout/migration/PCM/residency/lazy live mode/Settings/
  all216/resource contracts in real proposal/design and six capability deltas.
- [x] Maintain affected architecture/development/PCM/stem/UI/KeyLock docs and the
  absolute serial handoff with exact next P1 scope.
- [x] Pass official strict, document/diff/link checks and complete independent
  nonauthor semantic review. Final index/tree and publication evidence must be
  retained locally before reporting P0 terminal completion.

No product/runtime/test code changes, model inference or full build matrix is
needed for P0. A plan cannot close feature, finite DSP, performance or human gates.

## P1: Central pad path/ownership and new-write foundation

- [ ] Introduce one authoritative contained pad asset resolver (native0..215 ->
  #1..#216), extending existing stable capture/lifecycle boundaries, with typed
  legacy/new layout recognition; no standalone product branch/global wrapper.
- [ ] Route new import originals/complete FullMix PCM/new stem WAV generations to
  pad-owned roots with original encoding/name and short immutable internal names.
  Keep verified legacy read/cleanup paths; do not migrate existing assignments yet.
- [ ] Prove same-name/different-content replacement, #1/#216 endpoints, invalid
  #0/#217/traversal/absolute/reparse paths, two pads/same bytes, sealed retained
  readers and cancellation/rollback/cleanup generation isolation on Windows.
- [ ] Update architecture/development/PCM/stem docs; relevant loader/stem artifact/
  lifecycle/persistence tests, native cold_store/project_assets/publication tests.

Exact boundary: new destinations and safe dual readers only. No mass migration,
stem-f32 format, lazy-mode/DSP redesign, separator rebuild or feature completion.
P1 is implementable without questions using the design's collision/lineage policy.

## P2: Transactional idempotent migration

- [ ] Implement bounded journal/copy-verify/immutable commit and atomic current
  config reference transaction serialized with autosave.
- [ ] Establish fresh path-bound SourceVersion/native source ownership/ACK and
  fresh accepted-timing verification/adoption; preserve raw saved/manual/TAP/
  accepted analysis, loop markers/key/settings/mode with explicit alias lineage.
- [ ] Prove retry/crash/cancellation at every copy/flush/rename/reopen/config/native
  boundary, changed session revision, unconfirmed native claim, partial/corrupt/
  missing sources and duplicate/multi-project references; old leased bytes/config
  remain valid for recovery. Retire only unreferenced old global storage safely.
- [ ] Update persistence/architecture/development docs and genuine save/restore/
  loader/native-ACK/lifecycle tests. No private audio/model/evidence migration.

## P3: Complete aligned persistent stem PCM

- [ ] Produce five complete f32le derivatives under the pad's `.pcm-cache`, with
  WAVs under its `stems`; common descriptor lineage/hash/dimension/shared offset
  selects the verified immutable pair only after both area commits. Reuse alignment.
- [ ] Prove cross-area partial/flush/rename/reopen/joint-marker failure, crash/retry/
  rollback and paired last-reader retirement without exposing half-complete sets.
- [ ] Add actual retained sealed PCM descriptors and fresh complete integrity
  verification, cancellation/partial/corrupt/incompatible transform handling.
- [ ] Separate five-artifact disk integrity from four-component runtime readiness;
  instrumental PCM remains disk/on-demand offline data. Test masks/sum/source zero
  and no stale-generation publication; document format/version/retirement policy.

## P4a: Direct source-range residency

- [ ] Replace complete reread/redecode/re-align relocation with cancellable direct
  FullMix/stem PCM range reads from retained descriptors and final-window allocation.
- [ ] Reuse exact unchanged handles/revisions; prepare only affected pad windows.
  Prove old voice/source/window/history/queued owners, real ACK, STOP/unload/
  source/bank races, seeks/ALL and source-relative complete metadata.
- [ ] Run dry interpolation/fractional P-H/seam/75-and1000-cycle complete-buffer
  oracles and instrument zero complete WAV conversion/alignment on warm changes.
  Update PCM/native timing/publication docs; labelled KEYLOCK fallback stays open.

## P4b: Finite DSP supply and continuation

- [ ] Audit executed source-domain feed/lookahead/history/FIFO/filter/seam and
  two-selection dependencies; derive bounded context descriptors and feasibility.
- [ ] Implement necessary finite native/history continuation outside realtime,
  preserving current source-specific permits/ownership and preallocated callback work.
- [ ] Prove parity/causality across current supported rate/unity at production k=0, loop/
  seek/old-voice/transition cases and the1598-frame33.292-ms unavailable-input fixture.
  Preserve original acoustic/latency gates and Rubber Band backend; no guessed halo.
- [ ] Use already available isolated nonzero-k diagnostic fixtures only as future
  compatibility checks; do not add production KEY/pitch here. Original B5/K1 full
  nonzero-k/rate/unity/latency matrix remains OPEN for its own original stages.
- [ ] Remove fallback dependence for supported finite loop contexts only after
  these proofs. If a context remains unsupported, keep explicit fallback/error and
  this step OPEN with its concrete bounded remediation; do not close the feature.
  Update KeyLock/PCM/architecture docs and native processor/history tests.

This coupled DSP proof merits an Astra Ultra recommendation in the next handoff
before starting it; model choice stays with the user/coordinator. Routine P1-P3
and measured P6 stay Sol Ultra. No model change is performed by P0.

## P5a: Safe first active residency and effective-mode truth

- [ ] Add a typed None->Some residency transaction for the current selected
  committed disk set; retain inactive inference/generation/content replacement.
- [ ] Preserve all source/cache/ticket/request/STOP/voice/geometry/accepted timing/
  native/history/FIFO/lease guards and reserved retirement/feedback before adoption.
- [ ] Add genuine current-bound effective-mode feedback; preserve the bounded
  live128-source-frame crossfade, same voice/playhead/loop/output timeline and
  FullMix while pending. No stop/restart simplification or enqueue-as-ACK.
- [ ] Headless native/controller/drain/render tests: first lazy switch while
  FullMix plays, active KEYLOCK, queue pressure, rejected/stale/cancelled work,
  source/set/window/bank/STOP changes, older pinned source, warm toggles and clickfix
  ready starts under saturated cold lane. Update alignment/publication/UI docs.

## P5b: Lazy policy, project Settings and complete216 admission

- [ ] Separate durable desire/disk eligibility/pending/error/resident-ready/
  effective mode; explicit ALL STEMS always requests missing windows despite
  saved equality. Default startup/generation retain only FullMix absent demand.
- [ ] Persist preload off/default and aggregate resident budget128..16384MiB/
  default512 in ProjectState; render warning/estimate/exact counts/bytes/errors
  through existing Settings/sidebar snapshots. No disk work in rendering.
- [ ] Implement bounded216 scheduler and aggregate unique-backing/old/new/pending
  reservations; preserve 2/32/8/1GiB and separate512MiB preparation limits. Prove
  descriptor capacity (roughly432 base owners plus actual overlap), not216voices
  or a fictional216*96-handle requirement; justify any concrete capacity delta.
- [ ] Prove complete216 unique FullMix and eligible four-component demand windows
  actually usable after matching ACK at sufficient declared budget, preload off/on
  and explicit lazy cases. Mocked/deferred/partial counts are not acceptance.
- [ ] Verify215 unchanged handles after one edit, warm idle reuse, pinned readers
  under budget reduction, fair backpressure, invalid settings, saved ALL STEMS and
  passive failure/retry. Update persistence/Settings/PCM docs and real tests.

## P6: Full feature engineering/resource/performance acceptance

- [ ] Verify all occupied owners and successful migrated projects have final
  pad-owned originals/.pcm-cache/stems; obsolete global storage has no remaining
  dependency. Complete copy/restore/crash/retry/rollback/private integrity evidence.
- [ ] Run matched actual cold/new-process warm/in-process warm 216 unique and
  duplicate-content workloads, FullMix/stems/preload off/on and first live activation.
  Include longer loops requiring deliberate RAM budget, not only tiny fixtures.
- [ ] Measure repeated unchanged starts, single-loop edit, native ACK/effective
  mode latency, I/O bytes/full decode/alignment counts, disk duplication, resident/
  transient overlap, whole-process working set/commit/peaks, integrity/save costs
  and lifecycle final retirement. Publish individual repeated runs/spread/negative
  outcomes and exact source/runtime identity; no Arc-bytes-as-RSS or folder speed proof.
- [ ] Re-run original affected timing/seam/history/STOP/bank/ownership/clickfix
  regressions with complete216 actual ACK proof and bounded admission failures.
  Record separate engineering versus human/device/hearing completion status.
- [ ] Independent complete final nonauthor review and coordinator acceptance;
  return to the retained original pre-port program, not Slice8/full-app port.

Original B2 independent references/six paired human sessions/absolute20%-zero-
baseline musical/default/remediation gates, B3-B8/K1/Slice7, B6 terminal ordering
fix, pitch/RubberBand/nonzero-k/rate/unity/latency,1/2/4/6-pad,30-minute device/live
and final visual/hearing/corrections remain required. Exact-fixture B3 offline B/S
preparation may follow without B2 closure; no synthetic human acceptance.

Original power-loss/non-Windows immutable-capture gates stay OPEN; fault injection
does not establish either. Original model-unavailable/independent-component/
atomic/freshness/E2E obligations remain required.
