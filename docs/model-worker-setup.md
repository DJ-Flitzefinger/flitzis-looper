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
On deep Windows paths the worker uses a compact installation cache or
`%LOCALAPPDATA%/LoBtJit`, keyed to the physical installation path.

Keep the earlier worker installation and its pointer while testing a new
installation.

## Optional stem models and codecs

On Windows x64, make a supported shared FFmpeg build with FFprobe available.
Use the runtime PATH or set `FLITZIS_FFMPEG_DIR` to its bin directory. Portable
binds its own copies. Prepare and check the stem installations from the
repository root:

```powershell
uv run --no-sync python scripts/setup-runtime.py --with-stems
uv run --no-sync python scripts/setup-runtime.py --with-stems --check
```

Setup verifies the pinned [HTDemucs checkpoint](../src/flitzis_looper/controller/model_pins.py)
and [BS-RoFormer configuration and checkpoint](../src/flitzis_looper/controller/bs_roformer_assets.py).
The default input cache is `models/stem-checkpoints`. `FLITZIS_MODEL_CACHE_DIR`
selects an explicit cache subject to path ownership checks. Model acquisition
resumes interrupted transfers and verifies the complete files against their
pinned digests. Setup uses tracked graph topology and derives the learned data
locally. Checkpoints and derived weights stay in the local model cache.

- **BS-RoFormer MUSDB18HQ** uses an installed native worker under
  `models/bs-roformer-onnx/` with the
  [CPU and CUDA graph topology](../workers/bs_roformer/README.md). Its CPU path
  uses Intel oneMKL and OpenMP libraries from pinned Intel wheels. The
  installation retains their licenses and notices in `mkl-notices/`.
  CUDA preparation reuses the cache for the matching GPU, driver, worker, graph
  and runtime.
- **HTDemucs** uses an immutable ONNX installation paired with the locally
  built worker and its C runtime. Source resolves the worker from its own Cargo
  build output. Portable pairs each installed Release with its sealed worker
  and CRT. Managed Release environments can bind an independently built worker
  through their verified runtime configuration.

Setup checks both model loads on CPU before selecting verified resources.
Generation and Settings use the installed resources. Verified saved stem sets
restore without model inference, including when a separator is unavailable.
BS-RoFormer is the default for fresh settings. Selecting HTDemucs preserves
its configured shifts and overlap for subsequent jobs. See the
[stem architecture](architecture/stems.md) for provider qualification,
fallback and assignments.
