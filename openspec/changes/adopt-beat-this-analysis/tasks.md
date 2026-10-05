## B1a: First implementation slice, no weights or default switch

- [ ] Freeze typed PCM/request/component-result contracts and existing job ownership integration.
- [ ] Add the non-realtime immutable full-track PCM export/worker boundary; do not use viewport
  waveform summaries. Bound queue, transfer chunks, sizes and off-thread resource retirement.
- [ ] Implement lazy adapter preflight and explicit unavailable/corrupt/mismatched-checkpoint
  outcomes with an injected worker test double; no model acquisition during these tests.
- [ ] Reuse request IDs/progress/stale-result rejection. Add bounded cancellation, timeout,
  process teardown and off-thread PCM cleanup; stale-result rejection alone does not stop work.
  Reject late responses after unload, source replacement, timeout or cancellation. Publish
  validated components atomically.
- [ ] Separate bounded beat-process termination from non-preemptible Rust KeyNet calls. Track
  in-flight key work as retiring; cap key/retirement slots and retained PCM with backpressure.
  Test stalled key work: whole-request cancellation/resource release remains incomplete until
  both branches actually settle, while stale publication and further unbounded work are blocked.
- [ ] Test no double decode, correct mono/time origin, rate metadata and terminal independent
  beat/key outcomes; valid audio remains loadable/playable when optional analysis is absent.
- [ ] Keep current new-analysis default unchanged and mark this slice as adapter-only.

## B1b: Explicit setup and real reference inference

- [ ] Implement separately invoked worker setup and locked compatible runtime; pin Beat This
  1.1.0, `final0`, minimal postprocessor, FP32 front end/configuration and full provenance.
- [ ] Acquire the selected model only through the explicit setup operation; establish the
  accepted manifest/checksum, verify SHA-256 and install atomically. Block inference downloads.
- [ ] Derive 22.05-kHz Beat This and 44.1-kHz KeyNet inputs from shared mono, preserving origin
  and tails. Retain KeyNet computation and error behavior; keep runtime provenance per branch.
- [ ] Test offline CPU operation, missing worker/model, corrupted model, wrong hash, cancellation,
  worker crash, stale response, cache restore without optional runtime and independent outcomes.
- [ ] Measure full-track cold/warm time, installed/model/cache size, RAM/VRAM, cancellation and
  concurrent playback impact with real selected-model inference and record raw results.

## B2: Quality acceptance and separate default cutover

- [ ] Freeze the private-track corpus, annotation uncertainty, held-out correction-burden gates,
  critical-downbeat criteria and local memory/time limits before tuning. Record raw results.
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
