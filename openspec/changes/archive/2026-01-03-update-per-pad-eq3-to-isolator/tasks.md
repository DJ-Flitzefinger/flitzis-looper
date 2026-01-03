## 1. Requirements & design alignment
- [x] 1.1 Implement DJ-style knob mapping: Kill (−∞) at minimum and +6 dB at maximum
- [x] 1.2 Standardize crossover frequencies to `300 Hz` (low/mid) and `3.5 kHz` (mid/high)

## 2. Rust: isolator DSP (real-time)
- [x] 2.1 Implement LR4 (24 dB/oct) low-pass and high-pass biquad coefficient helpers
- [x] 2.2 Implement per-voice isolator EQ state (two cascaded biquads per LP/HP stage)
- [x] 2.3 Update mixer EQ application to split → gain → sum (low/mid/high)
- [x] 2.4 Add Rust unit tests for coefficient finiteness and runtime stability
- [x] 2.5 Add Rust unit test for unity reconstruction (all gains unity)

## 3. Python: control surface and validation
- [x] 3.1 Expand per-pad EQ clamp/validation range to support kill
- [x] 3.2 Update sidebar EQ knob ranges and labeling to reflect kill behavior
- [x] 3.3 Update controller tests that assert clamping behavior

## 4. Integration validation
- [x] 4.1 Run `cargo test`
- [x] 4.2 Run `pytest`
- [x] 4.3 Manual sanity check: verify audible bass kill on a pad with low band at minimum
