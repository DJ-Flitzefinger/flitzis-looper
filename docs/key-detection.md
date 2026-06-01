# Key Detection

## Overview

Key detection in Flitzis-Looper uses a CNN-based approach, adapted from [MusicalKeyCNN](https://github.com/a1ex90/MusicalKeyCNN) by Korzeniowski & Widmer (2018). The original PyTorch model (`keynet.pt`) was [exported to ONNX](../scripts/convert-musical-key-cnn.py) for use with the Rust inference pipeline. The pipeline computes a Constant-Q Transform (CQT) spectrogram from mono audio, runs ONNX inference via `ort`, and maps the 24-class output to a musical key string using the Camelot Wheel.

**Module**: `flitzis-looper-analysis` → `key_detection`

## Architecture

```
mono f32 samples → CQT (105 bins, librosa-compatible) → log1p compression
  → ONNX inference (keynet.onnx, 24-class softmax)
  → argmax → Camelot index → key string (e.g. "Fm", "C#")
```

### CQT Parameters

| Parameter        | Value |
|------------------|-------|
| Sample rate      | 44100 Hz |
| n_bins           | 105   |
| bins_per_octave  | 24    |
| fmin             | 65 Hz |
| hop_length       | 8820 (~200 ms) |

The CQT implementation is a Rust port of librosa's pipeline (extracted from the `rosa` crate), producing numerically equivalent frequencies with magnitude tolerances that account for FFT backend differences. Magnitude-only projection is used (`|basis| @ |STFT|`), matching librosa's `phase=False`.

### ONNX Model

- **File**: `keynet.onnx`
- **Input shape**: `(1, 1, 105, T)` — batch, channels, frequency bins, time frames
- **Output shape**: `(1, 24)` — logits for 24 Camelot keys
- **Search paths**: exe dir → cwd → `assets/models/` → `models/` → `assets/`
- **Session caching**: lazy-loaded via `OnceLock<Mutex<Option<Session>>>`

The model was originally trained on the GiantSteps key dataset and exported from PyTorch with `torch.onnx.export` (opset 20, `dynamo=False`).

### API

```rust
pub fn detect_key(samples: &[f32], sample_rate: u32) -> Result<KeyDetectionResult, KeyError>
```

Returns a `KeyDetectionResult` containing:
- `key_name`: Camelot key string (e.g. `"Fm"`, `"C"`, `"D#m"`)
- `confidence`: softmax probability of the predicted class (0.0–1.0)
- `logits`: raw 24-class output from the model

## Evaluation

Evaluated on the **GiantSteps key dataset** (604 EDM tracks from Beatport.com) against manual key annotations. The dataset consists of ~2-minute low-fidelity audio previews annotated via user corrections on the GiantSteps platform.

### Results

| Method | Config/Implementation | Correct     | Notes |
|--------|-----------------------|-------------|-------|
| **MusicalKeyCNN** | Python (PyTorch + librosa) | 406/604 (67%) | Reference implementation, matches original paper (66.7%) |
| **MusicalKeyCNN** | Rust (ONNX + rosa CQT) | 386/604 (64%) | |
| **libkeyfinder** | | 347/604 (57%) | DSP-based (chroma + Krumhansl) |
| **stratum-dsp** | Default config + HPSS median-filter + Mode heuristic | 201/604 (33%) | Pure DSP, designed for EDM (chroma profiles) |

### Why the 3% gap (Rust vs Python)?

The gap stems from accumulated numerical differences across the pipeline:

- **Audio decoding**: `symphonia` (Rust) vs `libsndfile` (Python) — small MP3 decode differences
- **CQT computation**: `rosa`/`realfft` (Rust) vs `librosa`/`scipy.fft` (Python) — different FFT backends
- **Inference**: ONNX Runtime (Rust) vs PyTorch (Python) — small matrix operation differences

These only affect borderline tracks where the CNN's top-2 key logits are very close. On 527/604 tracks (87%), both implementations agree.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `ort` | ONNX Runtime inference |
| `realfft` | FFT for STFT computation |
| `rubato` | Phase vocoder resampling for octave decimation |
| `matrixmultiply` | BLAS-backed matrix operations |
| `num-complex` | Complex number support |

## Files

```
rust/crates/analysis/src/key_detection/
├── mod.rs              — public API (detect_key), result types
├── cqt.rs              — CQT computation, log1p compression
├── inference.rs        — ONNX session management, tensor creation, run_inference()
├── mapping.rs          — Camelot index (0–23) → key string
└── librosa_cqt/        — librosa-compatible CQT (ported from rosa crate)
    ├── mod.rs
    ├── cqt.rs          — VQT with octave decimation, per-filter wavelet construction
    ├── stft.rs         — Short-time Fourier transform
    ├── windows.rs      — Window functions (Hann, Hamming, etc.)
    ├── convert.rs      — Unit conversions (Hz, MIDI, note names)
    ├── dsp.rs          — Signal processing utilities
    ├── matrix.rs       — Matrix operations (matmul, views)
    └── tests.rs        — Unit tests adapted from rosa's cqt_test.rs
```

## Notes

- All errors return `KeyError` variants without panicking
- The ONNX session is cached globally — repeated calls on different audio reuse the same session
- The model handles variable-length input via GlobalAvgPool over the time dimension
- Key naming uses sharps for some keys (F#, C#, G#, D#, A#).
