## 1. Module scaffolding

- [x] 1.1 Create `rust/src/audio_engine/analysis/` directory and `mod.rs` with public `analyze_sample()` stub
- [x] 1.2 Create empty module files: `math_utils.rs`, `window.rs`, `phase_vocoder.rs`, `detection_function.rs`, `tempotrack.rs`, `downbeat.rs`
- [x] 1.3 Add `rustfft` as an explicit dependency in `Cargo.toml`

## 2. Math utilities and window functions

- [x] 2.1 Implement `math_utils.rs`: `adaptive_threshold()`, `principal_arg()`, `mean()`, `normalize()`
- [x] 2.2 Implement `window.rs`: Hann window generation with pre-allocation
- [x] 2.3 Add unit tests for math utilities with known inputs/outputs

## 3. Phase vocoder (FFT + phase tracking)

- [x] 3.1 Implement `PhaseVocoder` struct with `rustfft`-backed RFFT
- [x] 3.2 Implement magnitude and phase extraction from FFT output
- [x] 3.3 Implement phase unwrapping using `principal_arg`
- [x] 3.4 Add unit tests for PhaseVocoder with synthetic signals

## 4. Detection function (Onset Detection Function)

- [x] 4.1 Implement `DetectionFunction` struct with pre-allocated buffers
- [x] 4.2 Implement `process()` method: frame audio → Hann window → FFT → Complex Spectral Difference
- [x] 4.3 Implement adaptive whitening (even if disabled by default)
- [x] 4.4 Add unit tests for DetectionFunction with known audio patterns

## 5. Tempo tracking (Viterbi HMM + DP beat tracking)

- [x] 5.1 Implement Rayleigh/Gaussian weighting curve generation
- [x] 5.2 Implement Resonator Comb Filter (RCF) bank with autocorrelation
- [x] 5.3 Implement Viterbi decoding over RCF probability matrix
- [x] 5.4 Implement `calculate_beats()` — DP beat tracking with backtracking
- [x] 5.5 Implement Butterworth filter for ODF smoothing (`filter_df`)
- [x] 5.6 Add unit tests for TempoTrackV2 with synthetic rhythmic signals

## 6. Downbeat detection

- [x] 6.1 Implement `DownBeat` struct with spectral difference method
- [x] 6.2 Implement `find_downbeats()` to identify bar-start beats
- [x] 6.3 Implement bar grouping from downbeat positions
- [x] 6.4 Add unit tests for DownBeat with known time signatures

## 7. Analysis config and public API

- [x] 7.1 Implement `AnalysisConfig` struct with Mixxx-matching defaults
- [x] 7.2 Implement `BeatGrid` struct with `beats`, `downbeats`, `bars` fields
- [x] 7.3 Wire `analyze_sample()` to run the full pipeline: DetectionFunction → TempoTrackV2 → DownBeat
- [x] 7.4 Ensure `analyze_sample()` returns `SampleAnalysis { bpm, key, beat_grid }` matching existing API

## 8. Integration and migration

- [x] 8.1 Update `rust/src/messages.rs`: replace `stratum_dsp::BeatGrid` with local `BeatGrid`
- [x] 8.2 Wire the new `analysis` module into the existing `analyze_sample_async()` call site
- [x] 8.3 Verify Python-side interop: `BeatGrid` dict serialization still produces `{beats, downbeats, bars}`
- [x] 8.4 Remove old `analysis.rs` file (or rename for reference during testing)

## 9. Dependency cleanup

- [x] 9.1 Remove `stratum-dsp` from `Cargo.toml`
- [x] 9.2 Update any remaining imports that reference `stratum_dsp`
- [x] 9.3 Verify `cargo check` and `cargo clippy` pass

## 10. Testing

- [x] 10.1 Replace or update existing 9 stratum-dsp analysis unit tests with qm-dsp equivalents
- [x] 10.2 Add edge case tests: silent audio, single-sample buffer, extreme BPM ranges
- [x] 10.3 Run `uv run pytest` to verify Python integration tests pass (758 passed)
- [ ] 10.4 Manual validation: analyze several diverse audio files and verify BPM/beat grid results
