# Change: Add waveform loop editor

## Why
Performers need to adjust per-pad loop start/end precisely, using audio-derived onsets and musical bar lengths, without stopping playback.

## What Changes
- Add a per-pad **Waveform Editor** window (ImGui) opened from the selected-pad sidebar via **Adjust Loop**.
- Add a persisted per-pad **loop region** model (start/end, auto-loop on/off, bar count).
- Extend playback so a pad can loop within a configured region and be updated live while playing.
- Render a performant mono waveform preview with zoom/pan, loop markers, and a playback position marker.

## Impact
- Affected specs:
  - `performance-ui` (new sidebar action)
  - `performance-pad-interactions` (trigger semantics with loop region)
  - `play-samples` (looping semantics)
  - `project-persistence` (new per-pad persisted fields)
  - New: `waveform-editor`, `loop-region`
- Affected code (expected):
  - Python UI: `src/flitzis_looper/ui/render/sidebar_left.py`, new waveform editor renderer, UI state/actions
  - Python models/persistence: `src/flitzis_looper/models.py` (`ProjectState` additions)
  - Python controller: `src/flitzis_looper/controller/transport.py` (loop parameters, trigger behavior)
  - Rust audio engine: `rust/src/audio_engine/*` (looping within region; playback position reporting)

## Non-Goals
- Stereo waveform view (use mono view only).
- Beat-grid overlays beyond snapping behavior (no beat labels/lines in this change).
- Intro-region playback (separate future capability; referenced in `docs/todos-legacy-migration.md`).
