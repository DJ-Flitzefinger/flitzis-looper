# Model worker setup

Complete the [source setup](development.md#setup) first. The [Windows Portable
installer](portable-windows.md) provisions and checks its own copies.

## Beat This automatic BPM

From the repository root on Windows x64:

```powershell
uv run --no-sync python scripts/setup-runtime.py
uv run --no-sync python scripts/setup-runtime.py --check
```

The setup command builds the isolated native Windows worker and selects a
verified installation under `models/beat-this/`. Resumable downloads and build
scratch use `.cache/runtime-setup/`. Setup verifies the [pinned graph, runtime
and license notices](../assets/models/beat-this/README.md). It uses ONNX Runtime
1.24.4/API 24 and the same dynamic `final0` graph on CPU and CUDA.

With a usable NVIDIA device, setup prepares the installation's on-disk CUDA
cache using three synthetic mono sources in order (30.25 s, 162 s, 8 s at
48 kHz). Readiness follows confirmed CUDA execution and worker retirement.
The device UUID, driver release, package, graph and warmup identity govern reuse.
After an update or cache loss, setup or the next background analysis prepares
the cache again. CPU execution is available when CUDA is unavailable. The
check command verifies the package and reports cache status without inference.

Keep the earlier worker installation and its pointer while testing a new
installation. The separate Python 3.12 [CPU](../workers/beat_this/uv.lock) and
[CUDA](../workers/beat_this_cuda/uv.lock) environments remain available through
the explicit lower-level `flitzis_looper.analysis.setup` command for reference
work. Windows x64 production uses the ONNX package.


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
