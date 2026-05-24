# Flitzis Looper

Gen3 development branch for DJ Flitzefinger's Scratch Looper.

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

## Stem Generation Setup

Stem generation uses Demucs offline in a background worker. Before using **Generate Stems** for
the first time, install the external stem prerequisites:

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

If FFmpeg is installed but the app process cannot find it, point the app at the FFmpeg `bin`
folder before starting:

```powershell
$env:FLITZIS_FFMPEG_DIR="C:\path\to\ffmpeg\bin"
uv --no-cache run --no-sync python -m flitzis_looper
```

More detail: [docs/stem-generation-setup.md](docs/stem-generation-setup.md).
