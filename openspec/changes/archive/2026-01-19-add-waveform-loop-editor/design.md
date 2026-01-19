## Context
The current system plays and loops each pad over the full sample buffer. The UI has a placeholder **Adjust Loop** button in the selected-pad sidebar (`src/flitzis_looper/ui/render/sidebar_left.py`). Audio analysis yields a beat grid with beats and downbeats in seconds.

This change introduces a waveform editor UI and a persisted per-pad loop region. It also requires audio playback support for looping within a region and reporting playback position so the editor can display a playhead.

## Goals / Non-Goals
- Goals:
  - Per-pad waveform editor window in ImGui
  - Performant mono waveform rendering with zoom/pan using ImPlot
  - Persist loop settings per pad
  - Auto-loop by bars (4/4: 1 bar = 4 beats) with beat snapping
  - Live editing during playback
  - Sample-accurate marker placement at extreme zoom
- Non-Goals:
  - Stereo/multi-channel waveform editing UI
  - Intro region before loop (future)
  - Complex beat-grid visualization

## Decisions
- **Loop representation**: store loop start/end in **seconds** (project state), derived from the project-local cached WAV (which is resampled to the engine output sample rate). Seconds map naturally to beat-grid times.
  - When loop markers are set via sample-level interactions, the stored time SHALL be quantized to an integer sample index at the cached WAV sample rate (i.e., `n / sample_rate_hz`).
- **Default onset**: use `downbeats[0]` as default loop start when present; fallback to `beats[0]`, else `0.0`. This is intentionally provisional.
- **Auto-loop**:
  - When enabled, loop end is derived from loop start + `bars` * (4 beats) using the effective BPM (manual BPM override first, else analysis BPM).
  - When enabled, loop boundaries snap to the nearest beat times.
  - When BPM is unavailable, auto-loop cannot compute musical duration; the UI disables bar controls and uses the full sample length as the loop end.
- **Free mode**: when auto-loop is off, loop start/end are free (no snapping), but still support sample-accurate placement when zoomed sufficiently.
- **Waveform rendering**: use ImPlot (available via `imgui_bundle`) to render the waveform.
  - At normal zoom levels, render a cached min/max envelope (downsampled) for performance.
  - At extreme zoom, render individual sample points/segments so single samples are visible.
- **Waveform data source**: render from the cached project WAV on disk (`./samples/...`) to avoid exposing internal sample buffers across PyO3.
- **Playback position**: expose a low-rate (e.g., ~10 Hz) per-pad playhead time/position from the audio engine to the UI. This is expected to be implemented as an additional audio-thread → UI message.

## Alternatives Considered
- Store loop points as sample frames instead of seconds.
  - Rejected because beat-grid times are in seconds and would require constant conversions in UI.
- Provide direct access to the Rust sample cache for waveform rendering.
  - Rejected for now to keep the FFI surface small and avoid large transfers.

## Risks / Trade-offs
- Adding playback-position messages increases message traffic; mitigate by emitting at a low, fixed rate.
- Live loop updates can cause audible discontinuities if the current frame position is outside the new region; mitigate by clamping/wrapping to the region start.
- Waveform rendering can become expensive at extreme zoom; mitigate by caching and using drawing primitives with clipping.

## Migration Plan
- Introduce new persisted fields in `ProjectState` with defaults that preserve current behavior (loop region unset or defaulted to full sample).
- Ensure pads without analysis/BPM behave safely (no crashes; UI controls degrade gracefully).

## Open Questions
- None remaining for the initial proposal; onset semantics may be revised later.
