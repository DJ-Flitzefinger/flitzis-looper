## 1. Dependencies and Module Structure

- [ ] 1.1 Add `ort` dependency to `rust/crates/analysis/Cargo.toml`
- [ ] 1.2 Add `cqt-rs` dependency to `rust/crates/analysis/Cargo.toml`
- [ ] 1.3 Create `rust/crates/analysis/src/key_detection/` module directory with `mod.rs`, `cqt.rs`, `inference.rs`, `mapping.rs`
- [ ] 1.4 Register `key_detection` module in `rust/crates/analysis/src/lib.rs` and re-export public API (`detect_key`, `KeyResult`, `KeyError`)
- [ ] 1.5 Verify `cargo check` passes with new dependencies

## 2. Key Name Mapping

- [ ] 2.1 Implement Camelot index (0–23) to key string conversion in `mapping.rs` (all 24 keys: minor 0–11, major 12–23)
- [ ] 2.2 Add unit tests for all 24 Camelot indices verifying correct key strings (e.g., 7→"Am", 19→"C")

## 3. CQT Preprocessing

- [ ] 3.1 Implement CQT computation in `cqt.rs` using `cqt-rs` with parameters: n_bins=105, bins_per_octave=24, fmin=65, hop_length=8820
- [ ] 3.2 Apply log1p magnitude compression and remove last frequency bin to produce `(104, T)` tensor
- [ ] 3.3 Verify CQT output matches `librosa.cqt` reference by running Python preprocessing on a known audio buffer and comparing values within floating-point tolerance
- [ ] 3.4 If `cqt-rs` output does not match librosa, implement fallback CQT using `rustfft` (bank-of-filters approach)
- [ ] 3.5 Add unit tests: known input → expected tensor shape, edge cases (short audio, silence)

## 4. ONNX Inference Wrapper

- [ ] 4.1 Implement lazy-loaded `ort::Session` in `inference.rs` using `std::sync::OnceLock` for caching
- [ ] 4.2 Implement tensor creation from CQT `Vec<f32>` → `ort::Tensor<f32>` with shape `(1, 1, 104, T)`
- [ ] 4.3 Implement `run_inference()` that executes the session and extracts `(1, 24)` logits
- [ ] 4.4 Implement `argmax` on logits to produce predicted class index (0–23)
- [ ] 4.5 Handle model load failures gracefully (return `KeyError` without panicking)
- [ ] 4.6 Add unit tests: dummy tensor → valid class index, model-not-found error handling

## 5. Public API — `detect_key()`

- [ ] 5.1 Implement `detect_key(samples: &[f32], sample_rate_hz: u32) -> Result<KeyResult, KeyError>` in `key_detection/mod.rs`
- [ ] 5.2 `KeyResult` struct with fields: `key_name: String`, `confidence: f32` (softmax of top prediction)
- [ ] 5.3 `KeyError` enum with variants: `InsufficientData`, `SilentAudio`, `ModelError`, `CqtError`
- [ ] 5.4 Wire together: CQT → inference → argmax → key mapping → `KeyResult`
- [ ] 5.5 Add integration test: end-to-end `detect_key()` on a known audio buffer produces expected key

## 6. Parallel Analysis Pipeline Integration

- [ ] 6.1 Refactor `analyze_sample()` in `rust/crates/looper/src/audio_engine/mod.rs` to decode → mono → resample once, then clone the mono buffer for both pipelines
- [ ] 6.2 Resample mono buffer to 44100 Hz using `rubato` when source rate differs (shared step before fork)
- [ ] 6.3 Launch BPM pipeline and key detection pipeline concurrently using `std::thread::scope` (scoped threads to avoid ownership issues with the mono buffer)
- [ ] 6.4 Join both threads and assemble `SampleAnalysis` from both results
- [ ] 6.5 Ensure key detection failures return `"unknown"` without affecting BPM/beat-grid results
- [ ] 6.6 Verify total analysis time is bounded by the slower pipeline (measure with a timer in debug builds)

## 7. Model Distribution

- [ ] 7.1 Place `keynet.onnx` in the project (e.g., `assets/models/keynet.onnx` or alongside the binary)
- [ ] 7.2 Implement model path resolution: check relative to executable, then relative to project root
- [ ] 7.3 Add runtime validation: verify model file exists and is loadable on first `detect_key()` call

## 8. Testing and Validation

- [ ] 8.1 Run `cargo test` and `cargo clippy` — all tests pass, no warnings
- [ ] 8.2 A/B test key detection on 20+ diverse tracks: compare Rust output with Python `predict_keys.py` reference
- [ ] 8.3 Benchmark analysis time on representative tracks (target: <2s total including CQT + inference)
- [ ] 8.4 Test edge cases: silence, very short audio (<1s), mono input, stereo input, various sample rates (44100, 48000, 96000)
- [ ] 8.5 Verify no allocations or blocking occur on the audio callback thread (code review)
