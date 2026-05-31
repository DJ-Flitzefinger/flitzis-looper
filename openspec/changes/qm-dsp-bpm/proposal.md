# Proposal: Replace stratum-dsp BPM Pipeline with qm-dsp Port

## Why

`stratum-dsp` provides a solid autocorrelation-based BPM detector but lacks Viterbi-based beat tracking, downbeat/bar detection, and the ability to follow gradual tempo changes. The Queen Mary University `qm-dsp` pipeline — used by Mixxx — is a battle-tested, academically-grounded alternative. Replacing stratum-dsp with a Rust port gives us a superior beat grid pipeline, full ownership of the code, and reduced external dependencies.

## What Changes

- Port the qm-dsp tempo tracking pipeline (DetectionFunction + TempoTrackV2 + DownBeat) to pure Rust under `rust/src/audio_engine/analysis/`.
- Replace `stratum_dsp::BeatGrid` with an owned `BeatGrid` struct containing `beats`, `downbeats`, and `bars`.
- Use `rustfft` (already available transitively) for FFT operations, eliminating the need to port qm-dsp's C++ FFT wrapper.
- Remove the `stratum-dsp` crate dependency (key detection may be temporarily retained during transition).
- Replace the 9 existing stratum-dsp analysis unit tests with equivalent tests for the new pipeline.
- **BREAKING**: Internal `BeatGrid` type changes from `stratum_dsp::BeatGrid` to a local struct. Public API surface (`beats`, `downbeats`, `bars` fields) remains identical.

## Capabilities

### New Capabilities

- `qm-dsp-bpm-detection`: Onset detection function (Complex Spectral Difference), Viterbi HMM beat period estimation, dynamic programming beat tracking, and downbeat/bar detection — all pure Rust.

### Modified Capabilities

- `audio-analysis`: Replaces the `stratum_dsp` backend with the qm-dsp port. Adds explicit downbeat/bar detection. The requirement to produce BPM, key, and beat grid remains the same, but the detection algorithm and internal representation change.

## Impact

| Area | Impact |
|------|--------|
| `rust/src/audio_engine/analysis.rs` | Replaced by new module tree (`analysis/mod.rs`, `detection_function.rs`, `phase_vocoder.rs`, `tempotrack.rs`, `downbeat.rs`, `math_utils.rs`) |
| `rust/src/messages.rs` | `BeatGrid` becomes a local struct; `SampleAnalysis` struct stays the same |
| `Cargo.toml` | Remove `stratum-dsp`, add explicit `rustfft` |
| `src/flitzis_looper/audio/analysis.py` | No changes — Python-side consumes same dict structure |
| Tests | 9 existing stratum-dsp analysis tests replaced with new qm-dsp port tests |
| Real-time safety | No impact — analysis runs on background threads, never touches GIL or audio callback |
| Licensing | flitzis-looper is GPL; qm-dsp is GPLv2+, fully compatible |

## Non-Goals

- Key detection porting (retain stratum-dsp or defer to a separate change).
- Track segmentation, chromagram, MFCC, or other non-tempo qm-dsp modules.
- VST/LV2/CLAP plugin hosting or external DSP plugin support.

## GIL Avoidance & Real-Time Constraints

Analysis runs entirely in background Python/Rust threads. The Rust code contains no GIL access, file I/O, or blocking operations. No changes touch the CPAL audio callback or ring-buffer hot path.
