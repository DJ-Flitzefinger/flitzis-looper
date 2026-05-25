## 1. Specification And Design
- [x] 1.1 Add OpenSpec deltas for the neutral internal Rust DSP foundation boundary.
- [x] 1.2 Record the Stage 8 DSP/FX and later EQ replacement architecture plan in repository docs.
- [x] 1.3 Run official strict OpenSpec validation for this change.

## 2. First Rust Foundation Slice
- [ ] 2.1 Add a narrow internal Rust DSP module with fixed-size neutral node/chain state.
- [ ] 2.2 Add typed fixed-size DSP parameter identity and smoothing helpers without exposing new UI controls.
- [ ] 2.3 Integrate the neutral per-pad chain so current audio output and current EQ behavior remain unchanged.
- [ ] 2.4 Keep all allocation, topology preparation, plugin scanning, file I/O, JSON, Python/GIL access, logging, blocking waits, neural inference, and unbounded work out of the callback.

## 3. Tests
- [ ] 3.1 Add focused Rust tests for neutral pass-through.
- [ ] 3.2 Add focused Rust tests for smoothing target progression and clamping/rejection behavior.
- [ ] 3.3 Add focused Rust tests for prepare/reset behavior and fixed-size state assumptions.

## 4. Documentation And Handoff
- [ ] 4.1 Update architecture and message-passing docs with the implemented foundation result.
- [ ] 4.2 Update local `codex-meta/handoff/next-step.md`.

## 5. Validation
- [ ] 5.1 Run focused uv-managed Rust tests for the changed DSP/mixer modules.
- [ ] 5.2 Run the required uv-managed validation sequence for behavior or shared audio-state changes.
