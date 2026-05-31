## Why

Key detection currently returns `"unknown"` after the `stratum-dsp` removal, leaving the looper without musical key information for pads. The system needs an accurate, self-contained key detector that operates on the background analysis thread without touching the real-time audio callback.

## What Changes

- Add a CNN-based key detection pipeline (MusicalKeyCNN / Korzeniowski & Widmer) running on a Rust background thread via the `ort` ONNX runtime
- Implement CQT spectrogram preprocessing in Rust using the `cqt-rs` crate (with `rustfft` fallback if needed)
- Share preprocessing steps (mono mixing, resampling) with the existing BPM detection pipeline to avoid redundant work
- Bundle the pre-converted `keynet.onnx` model with the application
- Replace the `"unknown"` key placeholder in `analyze_sample()` with actual CNN-based key detection results
- Modify the `audio-analysis` spec to remove the `stratum_dsp` dependency and specify CNN-based key detection

## Capabilities

### New Capabilities
- `musical-key-cnn`: CNN-based musical key detection using the KeyNet model (ONNX via `ort`). Covers CQT preprocessing, model inference, Camelot-to-key-name mapping, and integration into the analysis pipeline.

### Modified Capabilities
- `audio-analysis`: Replace `stratum_dsp` key detection reference with CNN-based detection. The spec currently requires `stratum_dsp` for analysis; this change removes that requirement for key detection (BPM/beat-grid remain qm-dsp-based).

## Impact

| Area | Change |
|------|--------|
| `rust/crates/analysis/` | New `key_detection/` module (CQT, ort wrapper, key mapping); add `ort` and `cqt-rs` dependencies |
| `rust/crates/looper/src/audio_engine/mod.rs` | `analyze_sample()` wires `detect_key()` into the pipeline, replacing `"unknown"` placeholder |
| `rust/crates/analysis/Cargo.toml` | New dependencies: `ort`, `cqt-rs` |
| `openspec/specs/audio-analysis/spec.md` | Remove `stratum_dsp` requirement, specify CNN-based key detection |
| Application bundle | Ship `keynet.onnx` (~2-4 MB) alongside binary |
| GIL / real-time | No impact — all work runs on background analysis thread, never on audio callback or Python GIL |
