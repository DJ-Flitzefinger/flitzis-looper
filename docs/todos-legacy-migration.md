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
- [x] As a performer, changing the global speed audibly changes loop playback speed (varispeed DSP).
- [x] As a performer, I can enable **BPM lock**, so multiple loops stay tempo-aligned to a chosen master BPM.
- [x] As a performer, I can set the **master BPM** explicitly.

### 7) Key lock (independent pitch/speed behavior)
- [x] As a performer, I can enable **key lock** so tempo changes do not change musical pitch.
- [x] As a performer, key lock can be toggled without breaking the performance workflow.

### 7a) Gen3 transport timeline and quantized scheduling
- OpenSpec change (active): `add-rust-transport-timeline`
- OpenSpec change (active): `add-phase-aware-playback-sync`
- [x] As a performer, pad starts can be quantized to a Rust-owned global beat/bar timeline.
- [x] As a performer, immediate pad triggering remains available when quantization is disabled.
- [x] As a performer, quantized one-at-a-time pad switches stop the old pad and start the new pad on the same scheduled output frame.
- [x] As a performer, loops can align to downbeat/beatgrid metadata produced by analysis.
- [x] As a developer, the audio callback keeps using fixed-capacity real-time-safe data structures for scheduling.

Current Gen3 state: bounded per-pad timing anchors derived from analysis downbeats/beats are
published to Rust audio-thread state. Low-level quantized scheduled playback now uses the Rust
transport target-frame bar phase plus those anchors to choose phase-aware initial sample frames for
normal starts, retriggers, and MultiLoop-disabled exclusive transitions. BPM-lock now publishes a
fixed-size selected-pad phase-anchor request after a valid master BPM is set; Rust anchors the
transport downbeat from that active pad when BPM/timing metadata is available and otherwise keeps
existing tempo matching. UI/controller trigger-quantization controls now expose immediate,
next-beat, and next-bar triggering through fixed-size Rust mode updates.

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
- OpenSpec change (active): `add-offline-stem-cache`
- OpenSpec change (active): `add-stem-performance-controls`
- [x] As a performer, I can **generate stems** for a pad (vocals/melody/bass/drums/instrumental).
- [x] As a performer, the UI indicates **stem availability** per pad and gives feedback while stems are generating.
- [x] As a performer, I can **toggle stems** on/off during playback.
- [x] As a performer, I can **delete generated stems** for a pad without unloading the audio file.
- [ ] As a performer, I can **momentarily solo/mute** stems for performance gestures.
- [x] As a performer, I can quickly revert to the full mix ("stop stems" behavior).
- [x] As a performer, when multiple loops are active, I can choose which pad's stems I'm controlling.
- [x] As a performer, stems remain **synchronized** with the loop.

Gen3 constraint: stem generation must be offline/cache-based and only available for pads
that are not currently playing. The audio callback must never run stem separation, neural
network inference, or disk I/O; it may only mix already prepared audio buffers. The active
planning slice now has the first cache metadata/source-version model and background-task gating
in place. Production stem generation now uses a replaceable Python-side backend boundary with
Demucs as the first adapter. Demucs runs only on the background generation path, maps `other.wav`
to project `melody.wav`, derives `instrumental.wav` from aligned non-vocal stems, aligns final WAV
artifacts to the loaded full-mix shape, and stores model files outside the repository and outside
project samples in Demucs' standard Torch Hub checkpoint cache. The Looper does not start model
download from the Generate Stems UI; if the expected checkpoint is missing, it reports
`no Model installed`. Demucs also requires working `ffprobe` and `ffmpeg`; inaccessible tools are
reported as `FFmpeg/ffprobe unavailable`. TorchCodec is required by the current Torchaudio output
path and is preflighted as `TorchCodec unavailable` when native libraries cannot load. CUDA/GPU use
is optional and falls back to CPU from the background worker. The default Demucs quality settings
are `--shifts 10` and `--overlap 0.5`; they are bounded request parameters exposed by the
bottom-right Settings overlay with supported ranges of shifts 1 through 20 and overlap 0.25
through 0.95, then copied into the next backend generation request. FFmpeg lookup uses the process
`PATH`, an explicit
`FLITZIS_FFMPEG_DIR`, or local WinGet `Gyan.FFmpeg*` package installs before reporting FFmpeg
unavailable.
Prepared stem-buffer validation/publication now sends fixed-size Rust control messages with shared
immutable handles for inactive current pads, and the callback stores accepted handles in bounded
state. The mixer can render complete prepared stem sets through the same voice playhead and loop
path as full-mix playback, with full-mix fallback for missing or invalid stem data. Durable per-pad
full-mix/all-stems mode plumbing now exists: project
persistence defaults to full mix, the controller can publish all-stems mode for a current prepared
source version, and Rust renders prepared stems only when the accepted source-version hash matches
the selected all-stems mode. The selected-pad sidebar now shows stem status, routes Generate Stems
through controller/background-task gating, exposes full-mix/all-stems mode selection only when
current prepared stems exist, and adds Delete Stems directly under Generate Stems. Unload Audio
also deletes the tracked pad stem cache. The
bottom bar now adds selected-pad `V`/`D`/`M`/`B`/`I`/`A` mask buttons where `I` means Drums +
Melody + Bass and `A` means Vocals + Drums + Melody + Bass, without using `instrumental.wav` as a
direct preset layer. Component clicks from `I` or `A` enter custom mode with only the clicked
component active, and custom masks matching the preset combinations do not light `I` or `A`
implicitly. `I` and `A` share one exclusive preset group that remembers the last `V`/`D`/`M`/`B`
component state; switching between presets preserves that state, and clicking the active preset
again restores it. Right-clicking `V`/`D`/`M`/`B` sets a non-momentary custom solo state for that
component without adding a separate mute feature. The pad grid now shows compact stem status
indicators from existing controller/session snapshots without render-loop file I/O. Momentary
per-stem solo/mute controls remain future work. The Settings overlay now
persists bounded Demucs shifts and overlap values in project state and does not perform cache or
model work from the render loop.

### 13) Persistence (config/state)
- [x] As a user, my bank/pad assignments persist across restarts.
- [x] As a user, per-pad settings persist (BPM, loop points, auto-loop settings, intro settings, gain, EQ).
- [x] As a user, global settings persist (e.g., master volume).
- [x] As a user, startup handles missing/moved files gracefully and keeps the UI usable.
- [x] As a user, samples that I load will be copied to `./samples` in the current working directory.

### 14) Reliability + performance expectations (behavioral)
- [x] As a performer, long-running actions (e.g., BPM detection, stem generation) do not freeze the UI.
- [x] As a performer, triggering pads remains responsive and predictable during performance.
- [x] As a user, the application shuts down cleanly and reliably persists my configuration.

## Primary legacy sources consulted
- `old-project/README.md`
- `old-project/docs/architecture.md`
- `old-project/docs/source-tree-analysis.md`
- `old-project/flitzis_looper/` (UI, core, audio modules)
