# Stem Generation Setup

Flitzis Looper generates stems offline with Demucs. The Rust audio callback never runs Demucs,
FFmpeg, disk I/O, model loading, Python/GIL access, or neural inference.

## What `uv sync` Installs

The Python runtime dependencies for stem generation are declared in `pyproject.toml` and locked in
`uv.lock`:

- `demucs`
- `torch`
- `torchaudio`
- `torchcodec`

For a fresh checkout, run:

```powershell
uv --no-cache sync
$env:UV_NO_CACHE='1'; uv --no-cache run maturin develop
```

## Install FFmpeg

Demucs needs both `ffmpeg.exe` and `ffprobe.exe`. On Windows, the recommended install is:

```powershell
winget install --id Gyan.FFmpeg.Shared -e
```

Open a new PowerShell after installation and verify:

```powershell
where.exe ffmpeg
where.exe ffprobe
ffmpeg -version
ffprobe -version
```

The Looper resolves FFmpeg in this order:

1. the current process `PATH`
2. `FLITZIS_FFMPEG_DIR`
3. local WinGet `Gyan.FFmpeg*` package installs

If the app cannot find FFmpeg even though PowerShell can, start it with an explicit FFmpeg folder:

```powershell
$env:FLITZIS_FFMPEG_DIR="C:\Users\user\AppData\Local\Microsoft\WinGet\Packages\Gyan.FFmpeg.Shared_Microsoft.Winget.Source_8wekyb3d8bbwe\ffmpeg-8.1.1-full_build-shared\bin"
uv --no-cache run --no-sync python -m flitzis_looper
```

Use your actual FFmpeg `bin` folder. It must contain both `ffmpeg.exe` and `ffprobe.exe`.

## Install The Demucs Model

The Looper does not download the model from the UI. Install the default model once from the
project environment:

```powershell
uv --no-cache run --no-sync python -c "from demucs.pretrained import get_model; get_model('htdemucs'); print('htdemucs model installed')"
```

The expected Windows checkpoint path is:

```text
C:\Users\<YOUR_NAME>\.cache\torch\hub\checkpoints\955717e8-8726e21a.th
```

If this file is missing, **Generate Stems** reports:

```text
no Model installed
```

## Verify Stem Prerequisites

Run this from the repository root after `uv sync`, FFmpeg install, and model install:

```powershell
uv --no-cache run --no-sync python -c "from pathlib import Path; import subprocess, sys; from flitzis_looper.controller.stem_generation import demucs_cache_environment; env=demucs_cache_environment(Path.home()/'.cache'/'torch'/'hub'/'checkpoints'); subprocess.run(['ffprobe','-version'], env=env, check=True); subprocess.run(['ffmpeg','-version'], env=env, check=True); subprocess.run([sys.executable,'-c','import demucs, torch, torchaudio, torchcodec.encoders'], env=env, check=True); print('Stem prerequisites OK')"
```

## CUDA

CPU stem generation works and is the default fallback. CUDA is used automatically only when
PyTorch reports CUDA availability:

```powershell
uv --no-cache run --no-sync python -c "import torch; print(torch.__version__); print(torch.version.cuda); print(torch.cuda.is_available()); print(torch.cuda.device_count())"
```

If this prints `False`, the current environment is CPU-only. To use an NVIDIA GPU, install a
CUDA-enabled PyTorch/Torchaudio build that is compatible with the current TorchCodec version. The
separate CUDA Toolkit is normally not required for packaged PyTorch wheels; a compatible/current
NVIDIA driver is required.

## Expected Runtime Errors

- `no Model installed`: run the Demucs model install command above.
- `FFmpeg/ffprobe unavailable`: install FFmpeg or set `FLITZIS_FFMPEG_DIR`.
- `TorchCodec unavailable`: rerun `uv --no-cache sync`; if this persists, rebuild the environment.
