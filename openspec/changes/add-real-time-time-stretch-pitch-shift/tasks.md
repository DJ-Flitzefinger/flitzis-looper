## 1. Discovery / Alignment
- [ ] 1.1 Align BPM lock + speed slider behavior with legacy semantics
- [ ] 1.2 Align Key lock behavior with legacy semantics (preserve pitch vs tempo)

## 2. Message Protocol + State Plumbing
- [ ] 2.1 Add new `ControlMessage` variants for BPM lock, Key lock, master BPM, and per-pad BPM metadata
- [ ] 2.2 Wire Python controller to publish lock state + master BPM to Rust when toggled/changed
- [ ] 2.3 Wire Python controller to publish per-pad BPM updates to Rust when analysis/manual overrides change

## 3. Real-time DSP Implementation (Rust)
- [ ] 3.1 Add `signalsmith_dsp` dependency and a minimal wrapper for the Stretch processor
- [ ] 3.2 Implement per-voice stretch/pitch processing in the mixer with fixed-capacity buffers (no allocations)
- [ ] 3.3 Compute per-voice `tempo_ratio` and `transpose_semitones` from global state + pad metadata
- [ ] 3.4 Add parameter smoothing to avoid artifacts on slider drags
- [ ] 3.5 Define and document initial DSP configuration (block/interval, split computation)

## 4. UI / UX Updates
- [ ] 4.1 Replace placeholder BPM display with an effective BPM derived from master/global tempo state
- [ ] 4.2 Ensure BPM lock / Key lock controls reflect state and are stable-identifier friendly

## 5. Validation
- [ ] 5.1 Rust unit tests for key parsing and tempo/transpose math
- [ ] 5.2 Python tests for BPM lock anchoring master BPM to selected pad
- [ ] 5.3 Audio smoke test: slider changes update audible tempo without crash/panic
- [ ] 5.4 Run `cargo test` and `pytest`
