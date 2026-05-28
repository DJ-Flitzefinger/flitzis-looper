## 1. Planning And Specification
- [x] 1.1 Add a dedicated proposal and design for the 3-band DJ isolator EQ replacement.
- [x] 1.2 Add OpenSpec deltas for replacing the hardwired per-pad EQ on top of the Rust DSP foundation.
- [x] 1.3 Record non-goals and realtime callback constraints explicitly.
- [x] 1.4 Run official strict OpenSpec validation for this change.

## 2. First Implementation Slice
- [x] 2.1 Add a fixed-size Rust per-pad isolator DSP node with normalized low/mid/high targets.
- [x] 2.2 Route existing per-pad EQ setter behavior to typed DSP parameter identities and Rust-owned smoothing.
- [x] 2.3 Render the isolator through the per-pad DSP chain and remove the old hardwired EQ path from live audio processing.
- [x] 2.4 Preserve selected-pad EQ controls, middle-click neutral reset, durable project restore, and existing mapping action semantics.
- [x] 2.5 Keep plugin hosting, deck/group/master chains, new FX modules, real-time stem separation, and broad rewrites out of scope.

## 3. Focused Tests
- [x] 3.1 Add Rust DSP tests for neutral transparency, full-kill band behavior, bounded boost, finite output, and sample-rate preparation.
- [x] 3.2 Add Rust smoothing tests for coalesced target changes reaching the isolator without zipper-prone immediate jumps.
- [x] 3.3 Add Rust mixer tests proving the old hardwired EQ path is not double-processing audio after replacement.
- [x] 3.4 Confirm focused Python controller/UI/input-mapping tests are not required because the compatibility bridge and action mapping did not change.

## 4. Documentation And Handoff
- [x] 4.1 Update repository architecture docs with the validated isolator replacement plan.
- [x] 4.2 Update local `codex-meta/handoff/next-step.md` with the next small implementation step.

## 5. Validation
- [x] 5.1 Run official strict OpenSpec validation for this change after spec edits.
- [x] 5.2 Run focused uv-managed Rust tests for DSP/mixer changes.
- [x] 5.3 Confirm focused Python tests were not required for compatibility glue changes; full pytest passed.
- [x] 5.4 Run the broader uv-managed validation sequence for behavior or shared audio-state changes.

## 6. Focused Review/Audition
- [x] 6.1 Review the implemented isolator against deterministic low/mid/high representative
  sine tones.
- [x] 6.2 Confirm the Rust DSP-chain ownership and all-band boost cap are suitable for the
  current OpenSpec direction.
- [x] 6.3 Record that the current low/high kill response is not archive-ready and needs one
  focused tuning follow-up before OpenSpec acceptance/archive.
- [x] 6.4 Run official strict OpenSpec validation after the review-note update.

## 7. Focused Low/High Kill Tuning
- [x] 7.1 Replace the residual `dry - low - high` reconstruction with tuned fixed-size
  Linkwitz-Riley-style band splitting for non-equal isolator gains.
- [x] 7.2 Preserve exact neutral transparency, all-band kill silence, and uniform all-band
  `+6 dB` boost through the equal-gain dry path.
- [x] 7.3 Add representative low/high band-center kill thresholds while preserving other-band
  audibility, Rust-owned smoothing, and bounded finite output.
- [x] 7.4 Keep plugin hosting, unrelated FX, deck/group/master chains, real-time stem separation,
  live loop-edit crossfades, UI redesign, and broad rewrites out of scope.
- [x] 7.5 Run official strict OpenSpec validation, focused Rust DSP/mixer tests, and the broader
  uv-managed validation sequence.

## 8. Acceptance And Archive
- [x] 8.1 Review the tuned implementation, active spec delta, design notes, tasks, and repository
  docs for archive readiness.
- [x] 8.2 Run official strict OpenSpec validation before archive.
- [x] 8.3 Run focused uv-managed Rust isolator tests covering the archived behavior surface.
- [x] 8.4 Archive the change with the official OpenSpec CLI and update the baseline
  `per-pad-eq3` spec.
