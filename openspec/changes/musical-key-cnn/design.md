## Context

After removing `stratum-dsp`, key detection in `analyze_sample()` returns `"unknown"`. The BPM/beat-grid pipeline (qm-dsp port) works correctly and runs on a Rust background thread. We need to fill the key detection gap with a method that is accurate, permissively licensed, and fits the existing architecture.

MusicalKeyCNN (Korzeniowski & Widmer, 2018) is a CNN-based key detector achieving 73.51% weighted MIREX score — near commercial parity with Mixed In Key (75.70%). The pretrained model (`keynet.onnx`, already converted from `.pt`) is MIT licensed and runs fast on CPU. The ONNX runtime `ort` provides a Rust wrapper around the official ONNX Runtime, and `cqt-rs` provides Constant-Q Transform computation in Rust.

The existing `analyze_sample()` in `rust/crates/looper/src/audio_engine/mod.rs` already decodes audio to mono `f32` and runs the qm-dsp BPM pipeline. Key detection should share the decoded audio buffer rather than re-decode.

## Goals / Non-Goals

**Goals:**
- Accurate key detection (CNN-based, >70% MIREX weighted) replacing the `"unknown"` placeholder
- Pure Rust execution on the background analysis thread — no GIL, no audio callback impact
- Shared preprocessing: reuse mono audio from the BPM pipeline, avoid redundant decoding/resampling
- Self-contained: model bundled with application, no external download at runtime
- Graceful fallback: return `"unknown"` on any failure without crashing analysis

**Non-Goals:**
- ONNX conversion (already done manually)
- Key change tracking over time (single key per track only)
- GPU acceleration (CPU-only inference)
- Streaming/incremental key detection (whole-track analysis only)
- Replacing the BPM/beat-grid pipeline (qm-dsp remains unchanged)

## Decisions

### Decision 1: `ort` over `tract` for ONNX inference

**Choice:** Use `ort` (Pykeio/ort) — wrapper around official ONNX Runtime C API.

**Rationale:**
- Full ONNX operator coverage (~95%+) vs. tract's ~85%
- Better performance via native ONNX Runtime optimizations
- Actively maintained (v2.0.0-rc)
- We already link shared libraries (Rubberband), so the ONNX Runtime dependency is acceptable

**Alternatives considered:**
- `tract` (Sonos): Pure Rust, no shared lib linking. Rejected due to lower operator coverage and less active maintenance. All KeyNet ops are supported by both, but `ort` provides a safety margin for future model changes.

### Decision 2: `cqt-rs` for CQT preprocessing

**Choice:** Use `cqt-rs` crate for Constant-Q Transform computation.

**Rationale:**
- Purpose-built for CQT, likely matches librosa output
- Avoids reimplementing well-known DSP
- If numerical mismatch with librosa is found, fall back to custom `rustfft`-based implementation

**Alternatives considered:**
- Custom `rustfft` implementation: More control but more code to maintain. Kept as fallback.
- Python librosa subprocess: Avoids Rust CQT but adds process overhead and Python dependency. Rejected — defeats the purpose of Rust integration.

### Decision 3: Shared mono buffer between BPM and key detection

**Choice:** `analyze_sample()` decodes to mono `f32` once, then passes the same buffer to both the qm-dsp BPM pipeline and the CNN key detection pipeline.

**Rationale:**
- Audio decoding (symphonia) is the most expensive step — avoid doing it twice
- BPM pipeline already produces mono `f32` from `map_channels()`
- Key detection needs mono audio at 44100 Hz; if source sample rate differs, resample once and share

**Implementation:** Decode → mono → (fork) → BPM (f64) + Key (resample to 44100 if needed → CQT → ONNX)

### Decision 4: Lazy-loaded cached `ort::Session`

**Choice:** Initialize the ONNX session once (lazy, on first `detect_key()` call) and reuse it for all subsequent analyses.

**Rationale:**
- Session creation involves model loading and graph optimization — expensive one-time cost
- Analysis runs on a single background thread, so no concurrent session access
- Session is thread-safe for sequential `run()` calls

**Implementation:** `once_cell::sync::Lazy` or `std::sync::OnceLock` to hold the session globally.

### Decision 5: CQT parameters match librosa reference exactly

**Choice:** `n_bins=105`, `bins_per_octave=24`, `fmin=65 Hz`, `hop_length=8820`, then `log1p(|CQT|)` and remove last frequency bin → `(1, 104, T)`.

**Rationale:**
- Must produce identical input to the model as the Python preprocessing pipeline
- The model was trained on these exact parameters
- Any deviation degrades accuracy

### Decision 6: Module structure under `analysis/key_detection/`

**Choice:** New sub-module `key_detection/` inside the `flitzis-looper-analysis` crate.

```
rust/crates/analysis/src/
├── key_detection/
│   ├── mod.rs          # Public API: detect_key() -> Result<KeyResult, KeyError>
│   ├── cqt.rs          # CQT computation (cqt-rs wrapper or rustfft fallback)
│   ├── inference.rs    # ort Session wrapper + tensor creation + run
│   └── mapping.rs      # Camelot index -> key string conversion
```

**Rationale:**
- Keeps key detection self-contained and testable
- Separates concerns: CQT (DSP), inference (ML), mapping (business logic)
- Fits alongside existing BPM modules (`detection_function.rs`, `tempotrack.rs`, etc.)

## Risks / Trade-offs

| Risk | Severity | Mitigation |
|------|----------|------------|
| `cqt-rs` output doesn't match librosa numerically | **High** | Verify with unit tests comparing against known librosa output. Fall back to custom `rustfft` implementation if mismatch exceeds tolerance. |
| `ort` linking fails on some platforms | **Medium** | ONNX Runtime is well-supported on Linux/macOS/Windows. Test on all target platforms during integration. |
| Model file not found at runtime | **Medium** | Bundle `keynet.onnx` with the application. Use compile-time check or runtime validation with clear error message. |
| Inference too slow for large tracks | **Low** | CNN inference is <1s on CPU for typical tracks. CQT is the bottleneck — profile and optimize if needed. |
| Memory pressure from large CQT spectrograms | **Low** | 3-min track at hop=8820 → ~150 time frames × 104 bins × 4 bytes ≈ 62 KB. Negligible. |
| Accuracy regression vs. Python reference | **Medium** | A/B test on 20+ tracks comparing Rust output with Python `predict_keys.py`. Investigate any mismatches. |

### Trade-off: ONNX dependency vs. pure-Rust algorithm

Choosing `ort` adds a shared library dependency (~10-20 MB) but saves significant implementation effort and guarantees numerical correctness (the ONNX Runtime is well-tested). A pure-Rust CNN implementation would eliminate the dependency but introduce substantial risk of bugs and divergence from the reference model.

### Trade-off: CQT vs. FFT-based chroma

The CQT provides logarithmic frequency resolution matching musical pitch, which is why the CNN achieves high accuracy. An FFT-based approach (like libkeyfinder's DSK) would be simpler but less accurate. The CQT is the correct choice for CNN-based detection.

## Migration Plan

No user-facing migration is needed. The change replaces `"unknown"` with actual key values. Existing projects with `"unknown"` keys will show detected keys on next analysis. Manual key overrides remain unaffected.

### Rollback

If issues arise, the `detect_key()` function can return `"unknown"` immediately, restoring pre-change behavior without code removal.

## Open Questions

1. **CQT numerical match:** Does `cqt-rs` produce output within floating-point tolerance of `librosa.cqt` with matching parameters? Needs verification during Phase 1.
2. **Model file distribution:** Should `keynet.onnx` be bundled alongside the binary or embedded as a compile-time resource? Initial preference: alongside in `assets/models/`.
3. **Sample rate handling:** If source audio is not 44100 Hz, should we resample before CQT (required for correctness) or accept the quality hit of CQT at native sample rate? Decision: always resample to 44100 Hz using `rubato` (already available).
