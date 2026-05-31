## Why

Replace `stratum_dsp`'s key detection with a pure Rust port of **libkeyfinder** (Ibrahim Sha'ath's KeyFinder), the same library used by Mixxx as its default key detection backend. This eliminates the `stratum_dsp` dependency for key detection, gives us full control over the algorithm, and enables eventual complete removal of `stratum_dsp` from the project.

## What Changes

- **Add** `rust/src/audio_engine/analysis/keyfinder/` module with a pure Rust port of libkeyfinder (~700 lines of C++ re-implemented)
- **Modify** `analysis.rs` to use the libkeyfinder port for key detection instead of `stratum_dsp`
- **Update** `audio-analysis` spec to replace the `stratum_dsp` requirement with the libkeyfinder-based key detection
- **Remove** `stratum_dsp` dependency from `Cargo.toml` (once both BPM and key detection are ported)
- **Add** `rustfft` as a direct dependency (currently transitive via `stratum_dsp`)

## Capabilities

### New Capabilities

### Modified Capabilities
- `audio-analysis`: Replace the key detection backend from `stratum_dsp` to the libkeyfinder port. The key detection algorithm changes from stratum-dsp's opaque internal implementation to libkeyfinder's chromagram-based approach with Krumhansl-Schmuckler tone profiles and cosine similarity.

## Impact

- **Code**: `rust/src/audio_engine/analysis.rs` (key detection call site), new `analysis/keyfinder/` module tree
- **Dependencies**: `stratum_dsp` removed, `rustfft` promoted to direct dependency
- **API**: Internal `detect_key()` function replaces `stratum_dsp::analyze_audio()` for key detection; `SampleAnalysis.key` format unchanged
- **Real-time safety**: Key detection runs on the background analysis thread (not the audio callback), no GIL or file I/O involved
