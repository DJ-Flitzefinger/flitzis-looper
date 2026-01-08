## 1. Implementation
- [x] 1.1 Add persisted per-pad loop settings to `ProjectState` (start/end seconds, auto-loop enabled, bar count) with safe defaults.
- [x] 1.2 Extend controller transport to:
  - [x] 1.2.1 Compute default loop region (4 bars, default onset from analysis) and reset behavior.
  - [x] 1.2.2 Apply loop region to playback triggers for the selected pad.
  - [x] 1.2.3 Support live updates while playing.
- [x] 1.3 Extend Rust audio engine to support looping within a per-pad region:
  - [x] 1.3.1 Add control message(s) for setting per-pad loop region.
  - [x] 1.3.2 Update voice render to wrap within [start,end).
  - [x] 1.3.3 Emit per-pad playback position for UI playhead display (low-rate).
- [x] 1.4 Add waveform editor window in ImGui:
  - [x] 1.4.1 Open/close state driven by sidebar **Adjust Loop** (selected pad).
  - [x] 1.4.2 Controls: play/pause, reset, zoom, pan, auto-loop toggle + bar +/-.
  - [x] 1.4.3 Mouse: wheel zoom; middle-drag pan; left click set loop start; right click set loop end (only when auto-loop off).
  - [x] 1.4.4 Visuals: waveform (mono), loop region shaded light yellow, start line blue, end line red, playhead marker.
  - [x] 1.4.5 Render waveform with ImPlot (from `imgui_bundle`) using a cached envelope at normal zoom.
  - [x] 1.4.6 At extreme zoom, render individual samples and allow sample-accurate marker placement.
- [x] 1.5 Add/extend tests:
  - [x] 1.5.1 Python unit tests for loop-region defaults, auto-loop math, snapping behavior.
  - [x] 1.5.2 Rust unit tests for loop wrap math and message handling (where possible).

## 2. Validation
- [x] 2.1 Run Python checks: `ruff`, `mypy`, `pytest`.
- [x] 2.2 Run Rust checks: `cargo test` (and `cargo fmt`/`cargo clippy` if available in repo workflow).
- [ ] 2.3 Manual smoke: run app, load a loop, open waveform editor, adjust loop live, verify playhead/markers.
