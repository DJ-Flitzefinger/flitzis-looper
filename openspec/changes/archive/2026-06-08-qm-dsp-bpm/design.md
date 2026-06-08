## Context

The current BPM/beat-grid pipeline uses `stratum-dsp` (Rust crate) via a single `analyze_audio()` call. It produces BPM, key, and a basic beat grid but lacks Viterbi-based beat tracking, downbeat/bar detection, and the ability to follow gradual tempo changes. The algorithm is a black box inside the crate.

The target is the Queen Mary University `qm-dsp` pipeline (used by Mixxx), which uses an onset detection function (ODF) → Viterbi HMM beat period estimation → dynamic programming beat tracking → downbeat detection pipeline. Reference C++ source is available in `priv/mixxx/lib/qm-dsp/`. Full algorithmic details are in `priv/bpm-detection.md`.

## Goals / Non-Goals

**Goals:**
- Reimplement the qm-dsp tempo tracking pipeline in pure Rust (no FFI)
- Produce BPM, beat positions, downbeat positions, and bar positions
- Maintain or improve analysis speed vs. stratum-dsp
- Preserve the existing `analyze_sample()` API surface
- Achieve numerical parity with qm-dsp on test audio

**Non-Goals:**
- Key detection (retain stratum-dsp or defer)
- Track segmentation, chromagram, MFCC, or other non-tempo qm-dsp modules
- VST/LV2/CLAP plugin hosting
- Real-time audio callback changes (analysis is background-only)

## Decisions

### Pure Rust re-implementation (not C++ FFI)

**Decision:** Port qm-dsp C++ to Rust rather than wrapping via `cxx`/`bindgen`.

**Rationale:** The core files total ~1,200 lines of C++. Pure Rust avoids FFI overhead, C++ build complexity (cmake, Qt), and gives full control for debugging and optimization. Modern Rust idioms (`Vec`, iterators, SIMD-friendly loops) provide a clean implementation.

**Alternatives considered:**
- `cxx` binding to qm-dsp: adds C++ build chain, harder to debug
- `bindgen` + raw FFI: unsafe, maintenance burden
- Paper-based implementation from scratch: higher risk of divergence from battle-tested qm-dsp

### Module structure under `audio_engine/analysis/`

**Decision:** Create a submodule tree rather than a single monolithic file:

```
analysis/
├── mod.rs                  # Public API: analyze_sample()
├── detection_function.rs   # ODF (Complex Spectral Difference)
├── phase_vocoder.rs        # FFT + phase tracking
├── tempotrack.rs           # Viterbi HMM + DP beat tracking
├── downbeat.rs             # Downbeat/bar estimation
├── math_utils.rs           # Adaptive threshold, principal_arg, etc.
└── window.rs               # Hann window generation
```

**Rationale:** Each module maps to a qm-dsp class. Clear ownership boundaries aid testing and future modifications.

### `f64` internally, `f32` at API boundaries

**Decision:** All intermediate computations use `f64`; public API uses `f32`.

**Rationale:** qm-dsp uses `double` throughout. Preserving numerical accuracy during Viterbi/DP is important for beat position precision. `f32` at the boundary matches existing Python interop expectations.

### `rustfft` for FFT (not porting qm-dsp FFT)

**Decision:** Use `rustfft` crate directly for real-to-complex FFT in the PhaseVocoder.

**Rationale:** qm-dsp's `FFTReal` wraps kissfft (simple radix-2/3/5). `rustfft` is already available transitively via stratum-dsp, is well-maintained, and supports SIMD auto-vectorization. The PhaseVocoder only needs RFFT + manual magnitude/phase extraction.

### Pre-allocated buffers, no allocation in per-frame processing

**Decision:** `DetectionFunction::new()` allocates all buffers; `process()` reuses them.

**Rationale:** Analysis runs in background threads but pre-allocation avoids GC pressure, fragmentation, and keeps hot loops allocation-free. For a 3-minute track (~512k frames), this matters.

### Configurable parameters via `AnalysisConfig`

**Decision:** All magic numbers become fields on `AnalysisConfig` with Mixxx-matching defaults.

**Rationale:** Enables tuning per-genre or per-user-preference in the future. Makes the implementation testable with known-good parameter sets.

### Transition strategy: keep stratum-dsp temporarily for key detection

**Decision:** Retain `stratum-dsp` as a key-only dependency until key detection is also ported.

**Rationale:** Reduces risk — the BPM pipeline can be validated independently. Key detection is a separate concern and can be addressed in a follow-up change.

## Risks / Trade-offs

| Risk | Mitigation |
|------|-----------|
| Quality regression vs. stratum-dsp on some tracks | A/B test on 50+ diverse tracks; keep stratum-dsp as fallback during transition |
| Porting bugs in numerical code (Viterbi, DP) | Compare outputs sample-by-sample against qm-dsp on known test audio; port unit tests |
| Performance regression (Rust vs. optimized C++) | Profile early; Rust SIMD auto-vectorization should be competitive or faster |
| Missing downbeat detection in initial port | DownBeat is P1; add after initial Viterbi + DP pipeline is validated |
| License: qm-dsp is GPLv2+ | flitzis-looper is GPL — fully compatible |

## Migration Plan

1. **Phase 1:** Create new `analysis/` module alongside existing `analysis.rs`. Wire it up behind a feature flag or internal toggle.
2. **Phase 2:** Validate new pipeline produces correct results on test audio. Run A/B comparison.
3. **Phase 3:** Switch `analyze_sample()` to use new pipeline by default. Remove old code.
4. **Phase 4:** Remove `stratum-dsp` from `Cargo.toml` (or keep for key only).
5. **Rollback:** Old `analysis.rs` can be restored if new pipeline shows regressions.

## Open Questions

- **AnalysisConfig exposure:** Internal only for now. Keep it simple; expose to Python only if/when user tuning is requested.
- **Key detection:** Separate change. Retain stratum-dsp for key during transition.
- **Regression test suite:** No dedicated regression suite — rely on unit tests and manual validation.
