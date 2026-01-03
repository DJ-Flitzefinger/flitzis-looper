## 1. Requirements & design alignment
- [ ] 1.1 Implement DJ-style knob mapping: Kill (−∞) at minimum and +6 dB at maximum
- [ ] 1.2 Standardize crossover frequencies to `300 Hz` (low/mid) and `3.5 kHz` (mid/high)

## 2. Rust: isolator DSP (real-time)
- [ ] 2.1 Implement LR4 (24 dB/oct) low-pass and high-pass biquad coefficient helpers
- [ ] 2.2 Implement per-voice isolator EQ state (two cascaded biquads per LP/HP stage)
- [ ] 2.3 Update mixer EQ application to split → gain → sum (low/mid/high)
- [ ] 2.4 Add Rust unit tests for coefficient finiteness and runtime stability
- [ ] 2.5 Add Rust unit test for unity reconstruction (all gains unity)

## 3. Python: control surface and validation
- [ ] 3.1 Expand per-pad EQ clamp/validation range to support kill
- [ ] 3.2 Update sidebar EQ knob ranges and labeling to reflect kill behavior
- [ ] 3.3 Update controller tests that assert clamping behavior

## 4. Integration validation
- [ ] 4.1 Run `cargo test`
- [ ] 4.2 Run `pytest`
- [ ] 4.3 Manual sanity check: verify audible bass kill on a pad with low band at minimum
