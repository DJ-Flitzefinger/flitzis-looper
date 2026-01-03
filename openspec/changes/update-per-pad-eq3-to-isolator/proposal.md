# Change: Update per-pad 3-band EQ to DJ isolator

## Why
The current per-pad EQ uses shelving/peaking filters with a limited gain range, which makes it difficult to fully “kill” a band (e.g., remove bass entirely) in a DJ-style performance workflow.

## What Changes
- Replace the current shelving/peaking per-pad EQ DSP with a crossover-based 3-band “isolator” EQ (Linkwitz–Riley 4th-order / 24 dB per octave).
- Ensure each band control can reach a true kill (effective gain of `0.0`) at its minimum setting.
- Keep the external control surface concept the same: three per-pad controls (low/mid/high) adjusted from the left sidebar and applied in the real-time mixer.
- Standardize the UI control range to a DJ-style isolator: **Kill (−∞)** at minimum through **+6 dB** boost at maximum.
- Standardize crossover points to mirror common DJ mixers: ~`300 Hz` (low/mid) and ~`3.5 kHz` (mid/high).

## Impact
- Affected specs (delta): `per-pad-eq3`.
- Affected code (expected, implementation stage): Rust EQ DSP (`rust/src/audio_engine/eq3.rs`, `rust/src/audio_engine/mixer.rs`), message/control plumbing for EQ updates, and Python UI/constants/validation for the expanded dB range.
- Performance and real-time safety: the new EQ MUST remain allocation-free and non-blocking inside the audio callback.
