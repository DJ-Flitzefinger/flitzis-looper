## B1a: First implementation slice, no weights or default switch

- [x] Freeze typed PCM/request/component-result contracts and existing request/event integration
  for the explicitly invoked diagnostic boundary; leave normal routing and persistence intact.
- [x] Add the non-realtime immutable full-track PCM export/worker boundary; do not use viewport
  waveform summaries. Bound queue, transfer chunks, sizes and off-thread resource retirement.
- [x] Implement lazy adapter preflight and explicit unavailable/corrupt/mismatched-checkpoint
  outcomes with an injected worker test double; no model acquisition during these tests.
- [x] Reuse request IDs/progress/stale-result rejection. Add bounded cancellation, timeout,
  process teardown and off-thread PCM cleanup; stale-result rejection alone does not stop work.
  Reject late responses after unload, source replacement, timeout or cancellation. Publish
  validated diagnostic components atomically, without adopting saved analysis/maps.
- [x] Separate bounded beat-process termination from non-preemptible Rust KeyNet calls. Track
  in-flight key work as retiring; cap key/retirement slots and retained PCM with backpressure.
  Test stalled key work: whole-request cancellation/resource release remains incomplete until
  both branches actually settle, while stale publication and further unbounded work are blocked.
- [x] Exercise immutable loaded PCM without a decoder, mono/time origin, complete tails and
  loaded-rate metadata; derive key input directly at 44100 Hz and test independent terminal
  beat/key outcomes. Optional diagnostic failures do not replace playback buffers or saved data.
- [x] Keep current new-analysis default unchanged and document this slice as adapter-only;
  defer actual 22050-Hz frontend parity and model inference to B1b.
- [x] Complete full project build/Rust/Python/lint/type checks and official strict validation
  of this change: 450 Rust tests (one ignored doc test), 877 Python tests, development build,
  cargo check, production Clippy, Ruff/mypy and formatting passed. Final native publication
  acceptance refinement also passed all eight focused lifecycle tests.

## B1b: Explicit setup and real reference inference

- [x] Implement separately invoked worker setup and locked compatible runtime; pin Beat This
  1.1.0, `final0`, minimal postprocessor, FP32 front end/configuration and full provenance.
- [x] Acquire the selected model only through the explicit setup operation; establish the
  accepted manifest/checksum, verify SHA-256 and install atomically. Block inference downloads.
- [x] Derive 22.05-kHz Beat This and 44.1-kHz KeyNet inputs from shared mono, preserving origin
  and tails. Retain KeyNet computation and error behavior; keep runtime provenance per branch.
- [x] Test offline CPU operation, missing worker/model, corrupted model, wrong hash, cancellation,
  worker crash, stale response and independent outcomes. Existing legacy restore remains covered;
  persisted new-analysis cache restore belongs to B2, since diagnostics are not adopted here.
- [x] Measure full-track first/repeat fresh-process time, installed/model footprint, RAM,
  CPU-only VRAM policy, cancellation and concurrent playback telemetry with real inference.
  Cold-disk cache, shared uv cache/base Python size and callback/acoustic acceptance are not
  claimed. See docs/beat-this-reference-evidence.md for raw-evidence paths and exact limits.
- [x] Contain Windows launcher descendants before execution; retire process handles, output
  readers and PCM after cancellation/timeout/crash, including kernel exit-signaling delay.
- [x] Pass full project validation: 450 Rust tests, 904 application Python tests, 35 worker
  tests including real short/chunk-boundary parity, debug/release builds, lint/types/format
  and official strict validation. Default routing and live map/pitch activation remain off.

## B2: Quality acceptance and separate default cutover

- [x] B2b1: implement lossless inline binary64/Base64 full-result publication under the
  unchanged final/worker/count limits; validate both final schema versions and adversarial
  bounds/lifecycle cases; rerun complete T04/T05/R01 worker cases with preserved v1 lineage.
  All four arrays/raw wire/PCM are bit/byte-identical; full v2 envelopes are 391849/571105/
  642053 bytes and pass actual native validation. Full checks pass (477 ordinary Rust tests,
  separate private-evidence test, 1025 Python tests, debug/release builds, Ruff/mypy and strict
  validation); three release native lifecycle tests pass. This addresses final publication
  only; overall resource acceptance still failed at the end of B2b1. Native long-track
  staging is addressed by the completed B2b2 tasks below.
- [x] B2b2: stream complete loaded-rate f32-LE shared mono with the unchanged f64 channel
  mean, bounded buffers and a retained readable native file handle; release the analysis
  source pin off-thread only after successful complete flush, preserving playback ownership.
- [x] B2b2: derive the complete native 44100-Hz key vector from staged mono in bounded chunks
  with the same Rubato configuration, delay/tail/ceiling-count rules and full CQT/KeyNet path.
  Account for actual simultaneous source/export or key/output-buffer ownership under the
  unchanged 512-MiB cap, plus the independent complete-file cap; reject overflow/oversize.
- [x] B2b2: prove mono bit parity and full key-converter parity at 22050/44100/48000/96000 Hz,
  including silence, first/last impulses, chunk boundaries and fractional output lengths.
  Cover cancellation, partial/read/export failures, source replacement, stale publication,
  non-preemptible key/file lifetime, cleanup failures and one-job/zero-queue backpressure.
  Include all 5120 96-kHz remainders and the valid zero-output tail case exposed by R01;
  retain its failed first attempt and compare newly supported tails to the zero-extended oracle.
- [x] B2b2: rerun complete native T04/T05/R01 at their real 96000-Hz loaded rate with actual
  KeyNet and the frozen selected worker; measure live-process RSS, full publication, natural
  retirement and re-admission. Preserve frozen corpus/model/limits and historical failures.
  Do not count worker-only results as native/full-job resource acceptance.
  Final release passes at 24.943/33.930/38.581 seconds and staged PCM peaks
  274786096/398247896/461222648 bytes. See docs/beat-this-acceptance.md for all gates,
  source/revision lineage and the separately retained earlier short-track evidence.
- [x] B2b2: update maintained ownership/architecture docs, pass full build/Rust/Python/lint/type
  checks and official strict validation; report remaining resource/quality gates separately
  without enabling default analysis, saved-data adoption or live timing behavior.
  Final checks pass: 495 Rust tests, 1030 Python tests, six release native lifecycle tests,
  debug/release builds, cargo check, production Clippy, Ruff/mypy, formatting and strict
  validation. All three real cancel/unload probes retire without stale publication.
- [x] Freeze the finite private pilot corpus, annotation uncertainty policy, held-out
  correction-burden gates, critical-downbeat criteria and local memory/time limits before
  new inference/tuning. See docs/beat-this-acceptance.md and the hashed local v1 manifest.
- [x] Correct the legacy QM downbeat sample-hop defect before collecting the paired
  comparator: use one shared production/fixture pipeline, actual ODF integer hop for
  segments/seconds/BPM, retained frame-zero origin and bounded invalid-config handling.
  Spectral bar-phase regression distinguishes the corrected result from the historical
  zero-hop tie; multi-rate tests prove original sample coordinates. Saved/manual grids,
  legacy algorithms and selected Beat This configuration remain intact.
- [x] Repair the standard analysis converter's rejection of valid zero-output FFT tail
  padding by sharing the diagnostic converter's finite dimension-derived rule for both
  source and target rates. Preserve cancellation, PCM limits, origin, one delay trim and
  exact ceiling length; compare both paths with independent full-block explicit padding
  for the 4703-frame fixture, all 5120 96-kHz remainders and coprime rates. Validate the
  normal wrapper and real-source automatic loading, update maintained docs and pass full
  project checks and official strict validation without changing default analysis routing.
- [x] Prepare and validate the independent reference and measured paired-correction input
  workflow with exact frozen source/PCM/protocol hashes, private drafts/seals and an honest
  missing-input inventory. Input validation does not supply labels or certify acceptance.
- [x] B2 temporal metric core only: implement pure exact bounded monotone matching and
  complete supplied beat/downbeat timing reports with frozen uncertainty eligibility,
  explicit event/region denominators and companion interval errors on the same point pairs.
  Keep input certification unchecked, musical acceptance pending and default adoption blocked.
  The core supplies no file/CLI/seal integration or actual musical scores; the separate
  orchestration below binds complete native source/PCM/request/raw/envelope lineage.
  All independent-reference, count/bar, paired-human and cutover/restore/end-to-end
  parent tasks below remain open. See docs/beat-this-temporal-scoring.md for the exact boundary.
- [x] B2 private scoring orchestration/CLI only: revalidate full reference receipt bytes,
  source/PCM/coverage before opening candidates; bind approved historical native lineage,
  complete raw worker/component/final arrays and strict request identities. Support explicit
  content-verified T01/T02 path aliases while retaining the original manifest. Reject partial
  arrays, mismatches, duplicates and unsupported lineage; preserve original failed attempts
  and explicit missing inputs. No actual musical score, human acceptance or default promotion
  follows. Full independent reference, paired correction and cutover parent gates
  below remain open. See docs/beat-this-scoring-workflow.md.
  Actual T04/T05 archived lineages are complete; T01-T03 original completion-event
  logit roundtrip mismatches remain rejected with all historical failures retained.
- [x] B2 fresh native lineage only: obtain fresh complete T01-T03 copy-first native
  source/finite playback/full PCM/export/request/raw-worker/component/native-finish/v2
  publication chains through hardware-free productive preparation and the existing
  service/worker. Pin and rehash source, cold manifest, full native PCM, model/lock/setup,
  actual executing native binary and retained producer bytes through an explicit
  supported provenance contract. All six actual Debug-v2/Release probes pass with
  seven complete bit-exact array representations, actual ready KeyNet, native finish,
  retirement and same-source subsequent admission. Preserve the first Debug import
  failure and original historical v1 parity failures. Register the three fixed fresh
  Release profiles for draft selection; old IDs retain strict rejection. Recorded
  Release job walls are 20.2726/14.8532/18.3885 seconds, without RSS/resource acceptance.
  No ReferenceSeal, musical score, human acceptance or default cutover follows;
  remaining parent B2 gates below remain open. See docs/beat-this-scoring-workflow.md.
- [ ] Obtain independent full-span labels, certify recording groups/class coverage and
  measure paired held-out correction burden. Sparse manually verified grids alone do not
  satisfy whole-track quality/count continuity; raw predictions never serve as labels.
- [x] Implement and validate a versioned automatic BPM summary from full selected-backend
  beat results, with explicit beat units, distant-interval checks and uncertainty handling.
  Preserve true fractional BPM, raw/local timing and manual/TAP overrides; do not use
  integer rounding as musical truth or a summary as variable-map playback authority.
  `selected-backend-bpm-v1` reuses G2 fitting with complete all-assigned OLS and separate
  robust/distant diagnostics, full local intervals, unverified quarter assumptions and
  explicit rational count maps. Integrate only into diagnostic background snapshots after
  native finish; all5 complete candidates and actual unchanged G2 PCM evaluated. No routing
  cutover, accepted timing, musical certification or human/device acceptance follows.
  See docs/selected-backend-bpm.md for policy, actual results and honest uncertainty.
- [x] Evaluate longer representative middle-region selection/weighting for pad-load BPM
  against complete-track and distant-window evidence. Freeze the policy before held-out
  validation; cover sparse intro/outro, an ambiguous middle, variable/fractional tempo
  and explicit quarter-note counts across comparable transient spans (for example snares).
  Preserve full raw inference, source/grid/loop origins, selected-region provenance and
  uncertainty; reject unsupported peak pairing and half/double-tempo assumptions.
  Freeze `representative-middle-region-v1`: complete-source middle20-80/thirds,
  original indices/counts, equal-weight retained OLS,24positions/30s/60%span/20%edges,
  <=10%/2consecutive exclusions and G2 affine feasibility. Prefer viable middle,
  then longest/count/earliest third. All5 real candidates retain unsupported global
  status/no eligible region rather than tuning gates; real600s metadata120 retains
  separate zero measured slope/raw-lattice sensitivity and earlier G2 feature bounds.
  Debug/Release hardware-free actual native jobs retain complete snapshot arrays,
  resolved fitter-PYD/source-EXE identities, native finish and readmission. Regression
  fixtures are numerical contracts, not new musical references or audible acceptance.
- [x] Measure every frozen complete track's admission, raw response and final publication,
  time and actual live-process memory. Preserve full raw output even on publication failure;
  explicit size/admission rejection remains an acceptance failure, not a truncated success.
  Historical B2a resource gate failed: three native staging rejections and two oversize worker-only
  publication probes. T03 key-name validation was corrected and the release rerun retired
  naturally. Musical labels/correction comparison and default cutover remain pending.
- [ ] Compare corrected legacy results as evidence, not a model vote; record pass/fail and
  bounded remediation. Do not claim sample accuracy from 20-ms detections or 70-ms F1.
- [ ] After B1b and B2 acceptance pass, make `final0`/minimal the default for NEW beat analysis;
  remove qm from normal selected-backend routing without a hidden unavailable-model fallback.
- [ ] Preserve legacy saved analysis, manual BPM/offsets/markers and accepted maps; restore valid
  cached new analysis without models and do not trigger automatic reanalysis on project load.
- [ ] Test load/restore/manual analysis parity, unavailable status, successful key with missing
  beat worker, unknown key with valid beats and atomic retained-result handling end to end.
- [ ] Update architecture, key-detection/setup docs, native typing and dependency statements;
  state that base Torch/Demucs removal remains slice 7. Document explicit rollback routing.
- [ ] Run full project build/Rust/Python/lint/type checks and strict validation of this change
  plus affected map changes; verify current live launch/render behavior remains unchanged.
- [ ] Record accepted exact model/environment hashes and completed default cutover. Until this
  task passes, report replacement as incomplete, even if the adapter benchmark works.
