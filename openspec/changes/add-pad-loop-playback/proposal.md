# Change: Add looping pad playback + load/unload audio

## Why
Today, pads only play one-shot samples and there is no UI workflow to load/unload pad audio. For legacy feature parity, a performer needs to load audio onto a pad and have it repeat continuously until stopped.

## What Changes
- Add a single per-pad context-menu item whose label toggles between **Load Audio** (no audio loaded) and **Unload Audio** (audio loaded).
- When labeled **Load Audio**, selecting it opens a file picker filtered to `wav`, `flac`, `mp3`, `aif/aiff`, `ogg` and loads the selection into the pad’s sample slot.
- When labeled **Unload Audio**, selecting it stops playback (if active) and unloads the pad’s sample slot.
- Update sample playback so `AudioEngine.play_sample(...)` loops continuously by default (not one-shot) until stopped/unloaded.
- Add an `AudioEngine.unload_sample(id)` Python API and corresponding audio-thread message to remove sample buffers safely.

## Impact
- Affected specs: `performance-pad-interactions`, `load-audio-files`, `play-samples`
- Affected code (expected): `src/flitzis_looper/ui.py`, `src/flitzis_looper/app.py`, `rust/src/audio_engine.rs`, `rust/src/messages.rs`, `src/flitzis_looper_rs/__init__.pyi`, tests under `src/tests/`
- User-visible behavior: pads repeat continuously until stopped/unloaded; context menu supports loading/unloading audio.
