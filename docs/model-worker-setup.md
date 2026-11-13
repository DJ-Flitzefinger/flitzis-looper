# Model worker setup

Complete the [source setup](development.md#setup) first. The [Windows Portable
installer](portable-windows.md) provisions and checks its own copies.

## Beat This automatic BPM

From the repository root on Windows x64:

```powershell
uv run --no-sync python scripts/setup-runtime.py
uv run --no-sync python scripts/setup-runtime.py --check
```

The setup command provisions the verified installation under `models/beat-this/`
and keeps resumable transfers and setup scratch under `.cache/runtime-setup/`.
The check verifies the existing selection without acquiring assets or running
BPM inference. Normal startup and analysis use the selected resources offline.
Missing or invalid resources are reported through the [BPM path](architecture/bpm-analysis.md).

The [CPU lock](../workers/beat_this/uv.lock) uses CPython 3.12.13, Beat This
1.1.0 and Torch/TorchAudio 2.8.0 CPU for Windows x64. Fresh source and Portable
installs default to CPU. The [CUDA lock](../workers/beat_this_cuda/uv.lock)
uses Torch/TorchAudio 2.8.0+cu128. To request it in a source checkout:

```powershell
uv run --no-sync python scripts/setup-runtime.py --profile cuda-auto-v2
```

The Torch CUDA profile keeps logmel and minimal postprocessing on CPU, runs the
float32 model on CUDA when usable and permits a fresh CPU model attempt after a
qualifying CUDA failure. The completed receipt reports what ran. The
[artifact manifest code](../src/flitzis_looper/analysis/artifacts.py) pins
`final0`, 81,058,141 bytes, SHA-256
`8c328b45f59d8dd3dff219253ff6a8d6482be57d0133a29140e2febbf8eb8331`.

For independent staging, use the lower-level CLI with absolute paths:

```powershell
$workerRoot = Join-Path (Get-Location) 'models/beat-this'
$scratch = Join-Path (Get-Location) '.cache/runtime-setup'
uv run --no-sync python -m flitzis_looper.analysis.setup --install-dir $workerRoot --scratch-dir $scratch --stage-only
```

This retains a verified `installation-<UUID>` without selecting it.
For offline staging, `--checkpoint-file` and `--offline`
require the pinned interpreter and wheels to be cached. Activation uses
`--activate-installation` with the exact retained directory and revalidates
its manifest and Native capability before atomically switching `current.json`.
Close the App before changing the selection. Keep the prior pointer and
matching Native available for rollback.

An explicit Windows x64 Native package uses the pinned dynamic `final0` graph,
libsoxr and the matching ORT 1.24.4 DLLs. With the same install and scratch
arguments, pass `--stage-native-cpu-package <package-directory>` or
`--stage-native-cuda-package <package-directory>` instead of `--stage-only`.
The default CUDA policy is strict. `--native-cuda-policy auto` permits the
packaged same-graph CPU fallback after a qualified CUDA inference fault.
Activate the exact retained installation separately. The
[setup implementation](../src/flitzis_looper/analysis/setup.py) and
[Native package verification](../src/flitzis_looper/analysis/native_beat_worker.py)
define the accepted manifests, files and runtime capabilities.

## Optional stem models and codecs

```powershell
uv run --no-sync python scripts/setup-runtime.py --with-stems
ffmpeg -version
ffprobe -version
```

Use FFmpeg with FFprobe on the runtime path. On Windows use the shared build
that also supplies the DLLs needed by TorchCodec. Portable bundles its own
copy. The application's [main lock](../uv.lock) selects Windows CUDA 13.0
Torch 2.12.0 and TorchAudio 2.11.0 wheels and TorchCodec 0.13.0. The separate
Beat This worker does not share these packages.

The extra setup flag acquires the pinned Demucs `htdemucs` and BS-RoFormer
MUSDB18HQ assets, verifies their bytes and loads each model on CPU. The
[Demucs pins](../src/flitzis_looper/controller/model_pins.py) and
[BS-RoFormer pins](../src/flitzis_looper/controller/bs_roformer_assets.py)
are the authority for filenames, checksums and source provenance. The default
stem checkpoint path is `models/torch/hub/checkpoints/`. An explicit
`FLITZIS_MODEL_CACHE_DIR` selects a separate model cache subject to path
ownership checks. Generate and Settings use existing local assets.

BS-RoFormer is the default for fresh settings. A saved Demucs choice remains
valid, and switching the setting affects future generation requests. Verified
saved sets can restore with neither model installed. A qualifying CUDA failure
retries the selected model on CPU and records the reason. Asset, codec,
cancellation and host-resource errors keep their own outcomes.
See [stem execution and adoption](architecture/stems.md).

Setup resumes interrupted acquisitions and checks complete pinned digests before
activation. Model-load and availability checks establish readiness. Actual
inference receipts establish execution and fallback device.
