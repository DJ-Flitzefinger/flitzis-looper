## Context

Currently, `stratum_dsp` provides key detection as a black-box: `analyze_audio()` returns a `key.name()` string. The internals are opaque, and we depend on an external crate for this functionality. The goal is to port libkeyfinder (Ibrahim Sha'ath's KeyFinder, used by Mixxx) to pure Rust, giving us full algorithm control and enabling eventual removal of `stratum_dsp`.

Reference sources for the C++ library are available at `./priv/libkeyfinder`. The algorithm is documented in `priv/key-detection.md` with detailed pipeline stages, constants, and module mapping.

## Goals / Non-Goals

**Goals:**
- Pure Rust re-implementation of libkeyfinder's key detection pipeline
- Drop-in replacement for key detection in `analysis.rs` with identical output format
- Self-contained module with only `rustfft` as external dependency
- Maintain accuracy comparable to or better than `stratum_dsp` for tonal music

**Non-Goals:**
- Progressive/streaming analysis (flitzis-looper loads entire tracks)
- Key change tracking over time (only single key per track needed)
- Factory/caching wrappers (Rust handles allocation differently)
- GUI or standalone application

## Decisions

**Module structure: `analysis/keyfinder/` subdirectory.** The port is ~700 lines of C++ across 10+ modules. Grouping under `analysis/keyfinder/` keeps the analysis module clean and isolates the port. Files: `constants.rs`, `tone_profile.rs`, `key_classifier.rs`, `chroma_transform.rs`, `lowpass_filter.rs`, `chromagram.rs`, `window.rs`, `spectrum_analyser.rs`, `keyfinder.rs`.

**FFT with `rustfft`.** libkeyfinder uses FFTW3 in C++; `rustfft` is already a transitive dependency via `stratum_dsp`. Promoting it to a direct dependency avoids introducing a new crate. API difference (plan-based vs. one-shot) is a minor implementation detail.

**Whole-file analysis only.** libkeyfinder supports progressive analysis via `progressiveChromagram()`. flitzis-looper loads entire tracks into memory, so we implement only `keyOfAudio()` (whole-file analysis). This simplifies the API and avoids streaming complexity.

**AudioData inlined.** The C++ `AudioData` class is a thin wrapper around `std::deque<double>`. In Rust, `Vec<f64>` with simple indices is sufficient. No separate module needed — inline the logic in `keyfinder.rs`.

**FIR low-pass filter pre-computed.** The LPF coefficients are designed at construction time via IFFT of an ideal brick-wall response, then windowed with Hamming. This is a one-time cost per sample rate. Coefficients are cached in the `LowPassFilter` struct.

**Key name format preserved.** The output key string format (`"Am"`, `"C#"`, `"Fmaj"`) must match the existing `stratum_dsp` output to avoid cascading changes. A mapping function converts libkeyfinder's enum keys to the expected string format.

## Risks / Trade-offs

**Accuracy regression** → Mitigation: A/B test on 50+ diverse tracks, compare results with `stratum_dsp`. libkeyfinder is Mixxx's default and has proven accuracy for tonal music.

**Porting bugs in DSK** → Mitigation: Verify intermediate chroma vectors against libkeyfinder C++ output for known inputs. Add unit tests with expected chroma values.

**Performance regression** → Mitigation: libkeyfinder processes a 3-minute track in ~1.6s (C++); Rust should match or beat this. Key detection runs on the background analysis thread, not the audio callback.

**Edge cases (silence, noise)** → Mitigation: libkeyfinder returns `SILENCE` for low-energy audio. Implement equivalent behavior with a silence threshold check.

**`stratum_dsp` removal timing** → Mitigation: `stratum_dsp` is kept until both BPM (qm-dsp port) and key detection are ready. The key detection port can be merged independently, with `stratum_dsp` removal as a follow-up change.

## Migration Plan

1. Create `analysis/keyfinder/` module with all ported files
2. Add `detect_key()` public function matching the existing API surface
3. Update `analysis.rs` to call `detect_key()` instead of `stratum_dsp` for key detection
4. Run existing tests to verify no regressions
5. A/B test key results on diverse track set
6. `stratum_dsp` removal deferred until BPM port is also complete

**Rollback**: Revert `analysis.rs` to use `stratum_dsp` for key detection. The `keyfinder/` module can remain as dead code until ready.

## Open Questions

- Should we expose a confidence score (cosine similarity value) in `KeyDetection`, or keep it internal?
- What silence threshold value to use for the `SILENCE` key detection?
