# Flitzis Looper

Flitzis Looper is the Gen3 development branch of DJ Flitzefinger's live
performance scratch looper. The application combines a Python/Dear ImGui Bundle
control surface with a Rust/CPAL realtime audio engine exposed to Python through
PyO3 as `flitzis_looper_audio`.

The project is built around a clear runtime split:

- Python owns UI rendering, project persistence, settings, input-mapping edit
  UX, sample/stem orchestration, and background jobs.
- Rust owns the live audio path: output-frame transport, scheduling, mixer
  state, voice playheads, loop wrapping, playback-rate application, Key Lock,
  prepared-stem rendering, parameter application, per-pad DSP, metering, and
  audio-thread telemetry.

## Current Capabilities

- 6 x 6 performance pad grid with 6 banks, backed by 216 engine sample slots.
- Async sample loading, decoding, resampling, project-local source caching, and
  automatic BPM/key/beat-grid analysis.
- Loop playback with editable loop regions, 1/64-note snapping, waveform editor,
  grid offset, play/pause/stop controls, and playhead display.
- Multi Loop mode and one-at-a-time exclusive triggering.
- Rust-owned transport timeline, trigger quantization, fixed-capacity scheduler,
  and explicit transport phase anchoring from a playing pad.
- Global speed, BPM Lock, Key Lock, master BPM, per-pad BPM metadata, per-pad
  gain, per-pad metering, and per-pad 3-band DJ isolator EQ.
- Offline Demucs stem generation through a Python background backend, project
  stem cache metadata, prepared-stem validation/publication, full-mix/all-stems
  mode, and selected-pad stem mask controls.
- Keyboard and MIDI Learn with a Rust MIDI input layer outside the audio
  callback, in-memory mapping snapshots, direct dispatch for discrete
  audio-safe commands, and controller-owned dispatch for parameter actions.

## Runtime Signal Path

```text
Python UI / controllers / persistence / background workers
-> PyO3 AudioEngine API
-> bounded Rust command ring + bounded Rust parameter ring
-> CPAL audio callback
-> TransportTimeline + TransportScheduler
-> RtMixer voice slots
-> full-mix or prepared-stem source selection
-> source-frame loop wrap and voice playhead
-> playback-rate / BPM Lock / Key Lock processing
-> per-pad Rust DSP chain with DJ isolator node
-> per-pad gain, trigger velocity, master volume
-> metering and audio-to-control telemetry
-> system audio output
```

The callback does not perform file I/O, JSON access, Python/GIL work, UI work,
plugin loading, neural inference, logging, blocking waits, or unbounded work.

## Repository Layout

```text
src/flitzis_looper/        Python UI, controllers, models, persistence, input mapping
src/flitzis_looper_audio/  Python type stubs and package wrapper for the Rust module
rust/src/                  Rust audio engine, message types, DSP, scheduler, loader
docs/                      Architecture and setup documentation
openspec/                  Baseline specs and active OpenSpec changes
src/tests/                 Python and native integration tests
```

Key architecture documents:

- [docs/audio-engine.md](docs/audio-engine.md)
- [docs/message-passing.md](docs/message-passing.md)
- [docs/audio-state-ownership.md](docs/audio-state-ownership.md)
- [docs/audio-loop-source-stem-alignment.md](docs/audio-loop-source-stem-alignment.md)
- [docs/dsp-fx-foundation-plan.md](docs/dsp-fx-foundation-plan.md)
- [docs/input-mapping-dsp-parameter-policy.md](docs/input-mapping-dsp-parameter-policy.md)
- [docs/stem-generation-setup.md](docs/stem-generation-setup.md)

## Setup

Run these commands from the repository root:

```powershell
uv --no-cache sync
$env:UV_NO_CACHE='1'; uv --no-cache run maturin develop
```

Start the app:

```powershell
uv --no-cache run --no-sync python -m flitzis_looper
```

## Validation

Common checks from the repository root:

```powershell
uv run maturin develop
uv run cargo check --manifest-path rust/Cargo.toml
uv run cargo test --manifest-path rust/Cargo.toml
uv run pytest
uv run ruff check src
uv run mypy src
```

Use `uv run cargo ...` for Rust checks so PyO3 and maturin use the project
Python environment consistently.

## Stem Generation Setup

Stem generation is offline/background work. The audio callback only mixes
already prepared buffers. Before using **Generate Stems** for the first time,
install the external prerequisites:

```powershell
winget install --id Gyan.FFmpeg.Shared -e
where.exe ffmpeg
where.exe ffprobe
uv --no-cache run --no-sync python -c "from demucs.pretrained import get_model; get_model('htdemucs'); print('htdemucs model installed')"
```

Then verify the full stem environment:

```powershell
uv --no-cache run --no-sync python -c "from pathlib import Path; import subprocess, sys; from flitzis_looper.controller.stem_generation import demucs_cache_environment; env=demucs_cache_environment(Path.home()/'.cache'/'torch'/'hub'/'checkpoints'); subprocess.run(['ffprobe','-version'], env=env, check=True); subprocess.run(['ffmpeg','-version'], env=env, check=True); subprocess.run([sys.executable,'-c','import demucs, torch, torchaudio, torchcodec.encoders'], env=env, check=True); print('Stem prerequisites OK')"
```

If FFmpeg is installed but the app process cannot find it, point the app at the
FFmpeg `bin` folder before starting:

```powershell
$env:FLITZIS_FFMPEG_DIR="C:\path\to\ffmpeg\bin"
uv --no-cache run --no-sync python -m flitzis_looper
```

More detail: [docs/stem-generation-setup.md](docs/stem-generation-setup.md).
