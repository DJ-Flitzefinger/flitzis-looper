## Context
- Samples are loaded asynchronously on a Rust worker thread and published to the audio thread via a ring buffer.
- The UI receives worker status via `LoaderEvent` messages polled each frame.
- The Python `ProjectState` is intended as persistent state; `SessionState` is runtime-only.

## Goals / Non-Goals
- Goals:
  - Detect BPM, musical key, and beat grid for each loaded sample using `stratum_dsp`.
  - Run analysis off the audio thread (worker thread only).
  - Provide progress updates that integrate with the existing per-pad progress indicator.
  - Store analysis results in app state intended for persistence.
  - Provide a manual "Analyze audio" action.
- Non-Goals:
  - Using beat grid to quantize playback or sync loops yet.
  - Perfect/continuous progress reporting from `stratum_dsp` (if not supported by its API).
  - Cross-sample/global tempo inference.

## Decisions
### 1) Analysis engine and defaults
- Use `stratum_dsp::analyze_audio(samples, sample_rate, AnalysisConfig::default())`.
- Defaults are taken from `AnalysisConfig::default()` as documented in `stratum_dsp::config::AnalysisConfig`.

### 2) Audio representation for analysis
- `stratum_dsp` expects mono `f32` samples (normalized).
- The loader currently decodes to interleaved `f32` and may resample to the output sample rate.
- Decision:
  - Convert the post-resampling audio to mono (e.g., average channels) before analysis.
  - Use the post-resampling sample rate (the engine output sample rate) when calling `analyze_audio`.
  - Let `stratum_dsp` handle normalization/trimming per its config defaults.

### 3) Where analysis runs
- Analysis runs on the existing Rust worker thread used for sample loading.
- Analysis is part of the load pipeline and occurs automatically after the resampling work completes.
- For manual re-analysis ("Analyze audio"), spawn a background operation that performs analysis only (no decode/resample/channel mapping and no re-publication of sample data).
- Manual re-analysis is not allowed while the pad is currently loading.

### 4) Progress reporting strategy
- `stratum_dsp` does not expose a progress callback in its public quick-start API.
- Decision:
  - Emit progress for analysis as a coarse step with a dedicated stage label (e.g., `"Analyzing (bpm/key/beat grid)"`).
  - Integrate analysis into the total pipeline percent using weighted ranges.
  - If analysis duration is unknown, emit two progress points: start of analysis (local 0.0) and end (local 1.0).

### 5) Data model for storing results
- Store per-pad analysis results in `ProjectState` so they can be persisted.
- Suggested minimal shape per pad:
  - `bpm: float | None`
  - `key: str | None` (musical notation, e.g., `"C#m"` from `stratum_dsp::Key::name()`)
  - `beat_grid: {"beats": [float], "downbeats": [float]} | None` (times in seconds)
- Rationale for reduced beat grid:
  - Beats + downbeats are sufficient for upcoming waveform overlays, onset suggestion, and beat alignment.
  - `bars` can be derived later if needed and would add additional serialized payload.
- Clear analysis results when a pad is unloaded or replaced.

### 6) UI display
- Pads: show BPM + key in the top-right corner when available.
- Sidebar: show BPM + key for the selected pad, and provide an "Analyze audio" action.

## Risks / Trade-offs
- CPU time: analysis may be expensive for long samples; running it on the worker thread avoids audio glitches but may increase load time.
- Persistence size: storing beat times in JSON can grow project files; the chosen reduced representation (beats + downbeats only, in seconds) keeps this bounded and still supports planned UI features.

## Migration Plan
- Add new fields to `ProjectState` with backwards-compatible defaults so existing serialized state can still be loaded.
- When older projects have missing analysis fields, treat results as unknown and allow manual analysis.

## Open Questions
- None.
