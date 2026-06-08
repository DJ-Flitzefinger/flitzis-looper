## 1. Module scaffolding

- [ ] 1.1 Create `rust/src/audio_engine/analysis/keyfinder/` directory and `mod.rs`
- [ ] 1.2 Add `rustfft` as a direct dependency in `Cargo.toml`
- [ ] 1.3 Wire `keyfinder` module into `analysis.rs` module tree

## 2. Constants and profiles

- [ ] 2.1 Port 72 chroma band frequency table to `constants.rs`
- [ ] 2.2 Port Krumhansl-Schmuckler major/minor tone profiles to `constants.rs`
- [ ] 2.3 Port octave weights to `constants.rs`
- [ ] 2.4 Add pipeline constants (FFTFRAMESIZE, HOPSIZE, BANDS, DIRECTSKSTRETCH)
- [ ] 2.5 Write unit tests verifying constants against known values

## 3. Window functions

- [ ] 3.1 Implement Blackman window function in `window.rs`
- [ ] 3.2 Implement Hamming window function in `window.rs`
- [ ] 3.3 Write unit tests for window function output values

## 4. Low-pass filter

- [ ] 4.1 Implement FIR LPF coefficient design via IFFT in `lowpass_filter.rs`
- [ ] 4.2 Implement circular convolution for LPF application
- [ ] 4.3 Write unit tests verifying LPF coefficients against libkeyfinder output

## 5. Chroma transform (DSK)

- [ ] 5.1 Implement Direct Spectral Kernel construction in `chroma_transform.rs`
- [ ] 5.2 Implement DSK application (FFT bins → 72-band chroma vector)
- [ ] 5.3 Write unit tests with known FFT input → expected chroma output

## 6. Spectrum analyser and chromagram

- [ ] 6.1 Implement windowed FFT → chroma vector per frame in `spectrum_analyser.rs`
- [ ] 6.2 Implement multi-hop chromagram storage and collapse in `chromagram.rs`
- [ ] 6.3 Write unit tests for spectrum analyser output

## 7. Tone profile and key classifier

- [ ] 7.1 Implement `ToneProfile` with circular rotation in `tone_profile.rs`
- [ ] 7.2 Implement cosine similarity classification in `key_classifier.rs`
- [ ] 7.3 Implement silence detection (zero-profile comparison)
- [ ] 7.4 Implement key enum → string name mapping
- [ ] 7.5 Write unit tests with known chroma vectors → expected keys

## 8. KeyFinder orchestration

- [ ] 8.1 Implement audio preprocessing (mono reduction, LPF, downsampling) in `keyfinder.rs`
- [ ] 8.2 Implement `detect_key()` public API orchestrating the full pipeline
- [ ] 8.3 Add `KeyDetection` result struct and `KeyError` error types
- [ ] 8.4 Write integration test with known audio → expected key

## 9. Integration with analysis.rs

- [ ] 9.1 Update `analysis.rs` to call `detect_key()` for key detection
- [ ] 9.2 Keep `stratum_dsp` for BPM until BPM port is ready
- [ ] 9.3 Verify existing tests pass with new key detection

## 10. Validation and polish

- [ ] 10.1 A/B test key results on diverse track set (compare with stratum_dsp)
- [ ] 10.2 Handle edge cases (silence, single-note, extreme dynamics)
- [ ] 10.3 Run `cargo clippy` and `cargo fmt` on new modules
- [ ] 10.4 Add docstrings to all public functions
