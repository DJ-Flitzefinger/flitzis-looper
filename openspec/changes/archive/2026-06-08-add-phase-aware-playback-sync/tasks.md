## 1. Specification And Planning
- [x] 1.1 Create the OpenSpec proposal, design, tasks, and spec deltas for phase-aware playback sync.
- [x] 1.2 Update relevant planning documentation without implementing production feature code.

## 2. Rust Phase Helpers
- [x] 2.1 Add deterministic transport helper tests for bar phase at arbitrary target output frames.
- [x] 2.2 Add mixer helper tests for computing a phase-aligned initial sample frame from pad BPM, phase anchor, and active loop region.
- [x] 2.3 Ensure missing or invalid pad BPM/anchor/loop metadata falls back to the existing loop-start frame.
- [x] 2.4 Treat any audio-thread allocation, logging, disk I/O, blocking operation, or Python/GIL access as a blocker before merge.

## 3. Quantized Playback Source-Frame Policy
- [x] 3.1 Keep scheduled event target frames as output-time scheduler data.
- [x] 3.2 Preserve effective loop-start source frames for quantized `PlaySample` starts and retriggers.
- [x] 3.3 Preserve effective loop-start source frames for quantized `PlaySampleExclusive` stop-all-then-play transitions.
- [x] 3.4 Preserve immediate trigger behavior when trigger quantization is disabled.
- [x] 3.5 Preserve scheduler-full rejection without evicting events or partially changing playback.

## 4. Explicit Transport Phase Anchor
- [x] 4.1 Add a fixed-size control message for requesting transport phase anchoring from a selected pad.
- [x] 4.2 Keep the request explicit instead of publishing it automatically when BPM lock is enabled or the anchor pad changes.
- [x] 4.3 In Rust, set the transport downbeat anchor from the active anchor pad when valid phase data is available.
- [x] 4.4 Fall back to existing BPM-lock tempo matching when the anchor pad is inactive or lacks valid metadata.
- [x] 4.5 Add deterministic Rust tests for active-anchor success and missing-anchor fallback.

## 5. Validation
- [x] 5.1 Run `openspec validate add-phase-aware-playback-sync --strict`.
- [x] 5.2 Run focused Rust tests for transport/mixer/audio-stream phase behavior.
- [x] 5.3 Run full uv-managed Rust and Python validation before implementation is considered complete.

## 6. Loop-Start Invariant Repair
- [x] 6.1 Remove phase-aware source-frame offsets from normal quantized `PlaySample` starts.
- [x] 6.2 Remove phase-aware source-frame offsets from normal quantized `PlaySampleExclusive` transitions.
- [x] 6.3 Keep transport phase helpers and `AnchorTransportPhaseFromPad` as explicit sync behavior only.
- [x] 6.4 Stop publishing automatic BPM-lock phase-anchor requests from Python master-BPM recomputation.
- [x] 6.5 Add regression coverage proving quantized triggers start from the effective loop start and do not catch up inside the loop.
- [x] 6.6 Re-run strict OpenSpec validation and the full uv-managed Rust/Python validation sequence.

## 7. Stage 5 Master-BPM Bridge
- [x] 7.1 Update the phase-sync contract so accepted performance master BPM is shared by transport-grid timing and BPM-lock tempo matching.
- [x] 7.2 Keep pad-derived phase anchoring explicit and separate from master-BPM updates.
