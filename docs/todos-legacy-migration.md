# Legacy migration TODOs (feature parity with `old-project`)

## Goal
Establish feature parity between the current project and the legacy `old-project` application.

This document is intentionally **high-level**: it captures **user-visible functionality / user stories** that exist in `old-project` and are **currently missing** from the current project. It is meant as input for follow-up OpenSpec specs.

## Current baseline (what exists today)
From the current codebase:

- A Rust `AudioEngine` exists (CPAL-based) with a minimal Python API: load sample by `id`, trigger playback with `velocity`, and stop playback by `id`.
- A Python UI entrypoint exists (`python -m flitzis_looper`) and boots a fixed-size Dear PyGui window with the performance layout (6×6 pad grid + bank selector row).

Everything below tracks parity against `old-project`; unchecked items are currently missing.

## Missing parity features (epics + user stories)

### 1) Performance UI: grid + banks + pad interactions
- OpenSpec changes (archived): `add-performance-ui-grid`, `add-performance-pad-interactions`
- [x] As a performer, I can see a **6×6 pad grid** where each pad represents a loop slot.
- [x] As a performer, I can switch between **multiple banks** (legacy: 6 banks) and see which bank is active.
- [x] As a performer, switching banks updates the visible pad assignments/labels for that bank.
- [x] As a performer, I can **trigger/retrigger** a pad to start playback from the loop start.
- [x] As a performer, I can **stop** a pad quickly (legacy: right-click behavior).
- [x] As a performer, I can open a **pad context menu** (legacy: middle-click) to access per-pad actions.
- [x] As a performer, the theme (colors, sizes) and UI structure (layout) will closely resemble the old Tk interface.

### 2) Loop playback (not just one-shot samples)
- OpenSpec changes (archived): `add-pad-loop-playback`
- [x] As a performer, a loaded pad plays as a **loop** (continuous repeat), not a one-shot sample.
- [x] As a performer, triggering a currently-playing pad **restarts** it in a predictable way.
- [x] As a performer, I can stop playback for an individual pad.
- [ ] As a performer, I can stop all currently-playing pads (UI action missing; `AudioEngine.stop_all()` exists).

### 3) Multi-loop mode (polyphonic looping)
- OpenSpec changes (archived): `add-multi-loop-mode`
- [x] As a performer, I can enable **multi-loop mode** to play multiple pads simultaneously.
- [x] As a performer, when multi-loop mode is disabled, triggering a pad stops other playing pads (legacy “one-at-a-time” behavior).
- [x] As a performer, the UI clearly indicates which pads are currently active.

### 4) Pad content management (load/unload)
- OpenSpec changes (archived): `add-pad-loop-playback`, `update-pad-labels-with-filenames`
- [x] As a performer, I can **load an audio file onto a pad** from the UI.
- [x] As a performer, I can **unload** a pad to free its slot.
- [x] As a performer, loaded pads show the loaded audio **filename** (basename) instead of just an index.
- [x] As a performer, pad labels include **BPM state/indicator** (in addition to filename).

### 5) BPM detection, manual BPM, and tempo workflow
- [x] As a performer, I can **auto-detect BPM** for a loaded pad.
- [x] As a performer, I can **re-detect BPM** for a pad.
- [x] As a performer, I can **set BPM manually** for a pad.
- [x] As a performer, I can set BPM via a **TAP BPM** workflow.
- [x] As a performer, the UI shows a clear **BPM display** while loops are active.

### 6) Global speed control + BPM lock
- OpenSpec changes (archived): `add-global-speed-control`
- [x] As a performer, I can adjust a global **speed control** (legacy: ~0.5× to 2.0×).
- [x] As a performer, I can **reset** speed to the default (1.0×).
- [ ] As a performer, changing the global speed audibly changes loop playback speed (varispeed DSP).
- [ ] As a performer, I can enable **BPM lock**, so multiple loops stay tempo-aligned to a chosen master BPM.
- [ ] As a performer, I can set the **master BPM** explicitly.

### 7) Key lock (independent pitch/speed behavior)
- [ ] As a performer, I can enable **key lock** so tempo changes do not change musical pitch.
- [ ] As a performer, key lock can be toggled without breaking the performance workflow.

### 8) Loop range editing (waveform editor)
- [ ] As a performer, I can open a **waveform editor** for a pad.
- [ ] As a performer, I can set **loop start** and **loop end** points.
- [ ] As a performer, I can preview playback while editing loop points.
- [ ] As a performer, I can **apply** changes or **undo** changes and exit the editor.

### 9) Auto-loop by bars + intro region
- [ ] As a performer, I can enable an **auto-loop** mode where loop length is defined in musical bars.
- [ ] As a performer, I can change the bar count (legacy presets like 4/8/16/32/64, plus custom increments).
- [ ] As a performer, I can define an **intro region** that plays before the loop begins repeating.

### 10) Mixing controls: master + per-pad
- [x] As a performer, I can control **master volume**.
- [x] As a performer, I can adjust **per-pad gain**.
- [ ] As a performer, I can reset master volume quickly.

### 11) Per-pad EQ and metering
- [x] As a performer, I can adjust **per-pad 3-band EQ** (low/mid/high).
- [x] As a performer, I can view a per-pad **level meter** while audio is playing.

### 12) Stems: generation, indicators, and performance mixing
- [ ] As a performer, I can **generate stems** for a pad (vocals/melody/bass/drums/instrumental).
- [ ] As a performer, the UI indicates **stem availability** per pad and gives feedback while stems are generating.
- [ ] As a performer, I can **toggle stems** on/off during playback.
- [ ] As a performer, I can **momentarily solo/mute** stems for performance gestures.
- [ ] As a performer, I can quickly revert to the full mix (“stop stems” behavior).
- [ ] As a performer, when multiple loops are active, I can choose which pad’s stems I’m controlling.
- [ ] As a performer, stems remain **synchronized** with the loop.

### 13) Persistence (config/state)
- [ ] As a user, my bank/pad assignments persist across restarts.
- [ ] As a user, per-pad settings persist (BPM, loop points, auto-loop settings, intro settings, gain, EQ).
- [ ] As a user, global settings persist (e.g., master volume).
- [ ] As a user, startup handles missing/moved files gracefully and keeps the UI usable.
- [ ] As a user, samples that I load will be copied to `./samples` in the current working directory.

### 14) Reliability + performance expectations (behavioral)
- [x] As a performer, long-running actions (e.g., BPM detection, stem generation) do not freeze the UI.
- [x] As a performer, triggering pads remains responsive and predictable during performance.
- [ ] As a user, the application shuts down cleanly and reliably persists my configuration.

## Primary legacy sources consulted
- `old-project/README.md`
- `old-project/docs/architecture.md`
- `old-project/docs/source-tree-analysis.md`
- `old-project/flitzis_looper/` (UI, core, audio modules)
