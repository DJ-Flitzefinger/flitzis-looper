## 1. Specification And Planning
- [x] 1.1 Create the OpenSpec proposal, design, tasks, and spec deltas for phase-aware playback sync.
- [x] 1.2 Update relevant planning documentation without implementing production feature code.

## 2. Rust Phase Helpers
- [x] 2.1 Add deterministic transport helper tests for bar phase at arbitrary target output frames.
- [x] 2.2 Add mixer helper tests for computing a phase-aligned initial sample frame from pad BPM, phase anchor, and active loop region.
- [x] 2.3 Ensure missing or invalid pad BPM/anchor/loop metadata falls back to the existing loop-start frame.
- [x] 2.4 Treat any audio-thread allocation, logging, disk I/O, blocking operation, or Python/GIL access as a blocker before merge.

## 3. Phase-Aware Quantized Playback
- [ ] 3.1 Pass scheduled event target frames into playback command execution or store an equivalent fixed-size phase descriptor.
- [ ] 3.2 Apply phase-aware initial frames for quantized `PlaySample` starts and retriggers.
- [ ] 3.3 Apply phase-aware initial frames for quantized `PlaySampleExclusive` stop-all-then-play transitions.
- [ ] 3.4 Preserve immediate trigger behavior when trigger quantization is disabled.
- [ ] 3.5 Preserve scheduler-full rejection without evicting events or partially changing playback.

## 4. BPM-Lock Transport Phase Anchor
- [ ] 4.1 Add a fixed-size control message for requesting transport phase anchoring from a selected pad.
- [ ] 4.2 Publish the request from Python/controller code when BPM lock is enabled or the anchor pad changes.
- [ ] 4.3 In Rust, set the transport downbeat anchor from the active anchor pad when valid phase data is available.
- [ ] 4.4 Fall back to existing BPM-lock tempo matching when the anchor pad is inactive or lacks valid metadata.
- [ ] 4.5 Add deterministic Rust tests for active-anchor success and missing-anchor fallback.

## 5. Validation
- [ ] 5.1 Run `openspec validate add-phase-aware-playback-sync --strict`.
- [x] 5.2 Run focused Rust tests for transport/mixer/audio-stream phase behavior.
- [x] 5.3 Run full uv-managed Rust and Python validation before implementation is considered complete.
