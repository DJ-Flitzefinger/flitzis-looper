# Stem Generation Setup

Flitzis Looper generates stems offline with Demucs htdemucs or BS-RoFormer
MUSDB18HQ. Settings provides a quick Stem Separator choice and return to Demucs.
Selection applies to future jobs; existing prepared sets and model-free restoration
keep their source/timing eligibility. Fresh or missing separator choices use
BS-RoFormer MUSDB18HQ; an explicitly saved choice, including Demucs, remains
authoritative. The Rust
callback never runs separators, FFmpeg, disk I/O, model loading, GIL/UI or inference.

## What `uv sync` Installs

Current [stem architecture](architecture/stems.md) separates source-leased offline
generation, immutable disk/config selection, component residency and effective
own-ACK adoption. Source-backed sets use the shared material root's `stems` and
aligned `.pcm-cache` areas with one verified joint commit. FULL MIX with preload
off need not retain unused stem windows. Model-free verified restoration needs
no new model download or inference. Historical programs are retained evidence,
not a pending implementation queue or current storage contract.

The Python runtime dependencies for stem generation are declared in
`pyproject.toml` and locked in `uv.lock`:

- `demucs`
- `torch`
- `torchaudio`
- `torchcodec`
- local `flitzis-bs-roformer` 0.1.0 from `vendor/bs_roformer`

The Windows lock pins Torch **2.12.0+cu130**, TorchAudio **2.11.0+cu130** and
TorchCodec **0.13.0**, with the Torch/TorchAudio wheels from the explicit official
CUDA 13.0 index. The worker pins Einops 0.6.1, Rotary-Embedding-Torch 0.3.5 and
Beartype 0.22.9. Beartype replaces upstream's historical 0.14.1 to support Python
3.14; no model-network change accompanies it. Training dependencies and other
model families are not installed. [TorchAudio's stable ABI](https://docs.pytorch.org/audio/main/installation.html)
supports Torch 2.11 and later. [Official Torch releases](https://pytorch.org/get-started/previous-versions/)
provide the selected Windows/Python 3.14 CUDA wheel.

For a fresh checkout, run:

```powershell
uv sync --locked
uv run --no-sync maturin develop --locked
uv run --no-sync python scripts/setup-runtime.py --with-stems
```

Normal `uv sync` uses its package cache. Downloaded models belong to this clone's
ignored `models/` directory: Stem checkpoints use `models/torch/hub/checkpoints/`,
and required automatic-BPM resources use `models/beat-this/`. Setup scratch and
retained transfers use `.cache/runtime-setup/`. An explicit `FLITZIS_MODEL_CACHE_DIR`
keeps precedence, including managed developer C caches and Portable's independent
installation Root. Model paths inside samples, recordings or repository code are
rejected rather than redirected to a global temporary directory. No checkpoint,
private audio or generated separation is committed.

## Install The Exact BS-RoFormer Model

The productive model is ZFTurbo's [v1.0.12 MUSDB18HQ release](https://github.com/ZFTurbo/Music-Source-Separation-Training/releases/tag/v1.0.12),
commit `aef04b2e52fb3beaf25e333199f5a7236e628e7b`. Its release asset config is
different from the similarly named single-vocals training example in the repository.
Install explicitly once; Generate Stems and Settings never download models:

```powershell
uv run --no-sync python scripts/setup-runtime.py --with-stems
```

| Release asset | Bytes | SHA256 |
| --- | ---: | --- |
| `config_bs_roformer_384_8_2_485100.yaml` | 4566 | `d8afb980318d0c08b9c2e24a7adc00d4f3150320c127a7e4de861800d1321939` |
| `model_bs_roformer_ep_17_sdr_9.6568.ckpt` | 527385512 | `3e9daecd70aaed5b5a0d1f861cc4d77eaa45afb3fc6301b1cf32c1be0f5868fb` |

The explicit installer resumes retained transfers and checks full size/hash before atomic replacement. The
adapter and worker verify identities before deserialization; the worker hashes
and loads weights from the same retained file handle with `weights_only=True`
and strict state matching. Missing/corrupt files fail with an offline model error.
No substitute model is selected. Unmodified MIT network files and attribution are
bound in [the vendored dependency](../vendor/bs_roformer/NOTICE.md).

The exact network uses dimension 384, depth 8, four stereo stems in order
drums/bass/other/vocals, STFT 2048/hop 441, at 44.1 kHz. Productive inference keeps
the release's 485100-frame chunks, overlap 2, batch size at most 2 and CUDA AMP;
CPU uses FP32. No loudness normalization is enabled. Long inputs decode to private
disk PCM, and a rolling overlap-add ring writes complete outputs. Fade windows
are independently constructed per chunk; non-finite output fails instead of being
silently replaced. Short final chunks use the upstream reflection/zero-padding
policy. Independent per-chunk windows correct the release's shared batch-window
mutation; exact release overlap-add numerical parity is not claimed. The conservative
live PCM bound is **183367800 bytes**, excluding model
weights, neural activations, decoder internals and process RSS. It is not a
measured allocator peak. Shared project alignment uses capped streaming blocks
and the previous scalar PCM16 rounding and pre-quantization instrumental sum.

## Install FFmpeg

Demucs needs both `ffmpeg.exe` and `ffprobe.exe`. On Windows, the recommended
install is:

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
$env:FLITZIS_FFMPEG_DIR="C:\Tools\ffmpeg\bin"
uv --no-cache run --no-sync python -m flitzis_looper
```

Use your actual FFmpeg `bin` folder. It must contain both `ffmpeg.exe` and `ffprobe.exe`.

## Install The Demucs Model

The Looper does not download the model from the UI. Install the default model
once from the project environment:

```powershell
uv run --no-sync python scripts/setup-runtime.py --with-stems
```

The expected checkpoint path, relative to the repository root, is:

```text
models/torch/hub/checkpoints/955717e8-8726e21a.th
```

If this file is missing, **Generate Stems** reports:

```text
no Model installed
```

## Verify Stem Prerequisites

Run this from the repository root after `uv sync`, FFmpeg install, and model install:

```powershell
uv --no-cache run --no-sync python -c "from pathlib import Path; import subprocess, sys; from flitzis_looper.controller.stem_generation import demucs_cache_environment; from flitzis_looper.controller.stem_generation import default_demucs_model_cache_dir; env=demucs_cache_environment(default_demucs_model_cache_dir()); subprocess.run(['ffprobe','-version'], env=env, check=True); subprocess.run(['ffmpeg','-version'], env=env, check=True); subprocess.run([sys.executable,'-c','import demucs, torch, torchaudio, torchcodec.encoders'], env=env, check=True); print('Stem prerequisites OK')"
```

## CUDA

CPU stem generation works and is the same-model fallback. CUDA is used
automatically only when PyTorch reports CUDA availability:

```powershell
uv --no-cache run --no-sync python -c "import torch; print(torch.__version__); print(torch.version.cuda); print(torch.cuda.is_available()); print(torch.cuda.device_count())"
```

The pinned Windows wheels include CUDA 13.0; a separate toolkit is not required.
An installed CPU wheel, unavailable GPU or incompatible driver may still print
`False`. CUDA 13.x requires the [NVIDIA driver compatibility family](https://docs.nvidia.com/deploy/cuda-compatibility/minor-version-compatibility.html)
580 or newer. The integration workstation has an RTX 5090 Laptop GPU, compute
capability 12.0, 24463 MiB VRAM and driver 616.92. Wheel installation and
`is_available()` alone do not prove actual model initialization or separation.
GPU kernels and real output are checked separately from human/device acceptance.

## Selection And Rollback

Choose **BS-RoFormer MUSDB18HQ** in Settings before requesting generation on a
loaded source, including during playback or pause under its source lease.
New-set audio adoption has separate state-specific guards. To return, choose
**Demucs htdemucs**; its saved shifts/overlap values
are retained. Running jobs retain their original choice. Existing complete sets
can restore with either model missing and do not need inference again. Both paths
use source tickets, immutable private generations, leases and complete-set
integrity for disk eligibility; effective audio requires its own Native ACK.
Two worker slots and 32 queued requests bound
separator admission; the native cold/preparation limits remain separate.
The unchanged Demucs CLI retains whole-track neural input/output PCM; the bounded
shared artifact writer does not establish a total Demucs inference-memory bound.

CUDA failure retries the same selected model once on CPU and reports the reason;
it never switches BS-RoFormer to Demucs automatically. CUDA wheels themselves can
execute CPU operations. For a broken install, rerun `uv sync --locked`, then
`uv run --no-sync maturin develop`; verify both model/tool prerequisites. Returning
to Demucs is a model choice, not a dependency removal or cache reset. This selector
does not change the required BeatThis automatic-BPM worker setup.

## Expected Runtime Errors

Source-backed sets use the shared material root; the implemented legacy
`samples/stems/#<pad>/` fallback is not the sole current destination. Workers write private
`.generation-<uuid>` directories and complete sets publish as immutable
`.ready-<uuid>` directories. Project metadata selects an exact set. Replacing
or deleting stems revokes eligibility first; contained native cleanup waits for
assignment, separator-job and native PCM readers before deleting that generation.
An old cleanup cannot remove a newer set, another assignment's readers or unknown
files. See [stem architecture](architecture/stems.md) for current ownership.

- `no Model installed`: run the Demucs model install command above.
- `no Model installed: BS-RoFormer MUSDB18HQ`: run its explicit installer above.
- `Model integrity check failed`: reinstall the exact pinned assets explicitly.
- `Stem queue full (2 workers, 32 queued jobs)`: wait for a job to finish and retry.
- `FFmpeg/ffprobe unavailable`: install FFmpeg or set `FLITZIS_FFMPEG_DIR`.
- `TorchCodec unavailable`: rerun `uv --no-cache sync`; if this persists, rebuild the environment.

## Direct live/long-source extension

The [E11 plan](../openspec/changes/archive/2026-10-30-adopt-pad-owned-pcm-residency/evidence/pre-rust-extension-20261011.md) requires streamed full source/five-WAV/PCM/mono alignment and fresh full warm verification without a total-duration/1-GiB extent cap. X11-LONG implements these complete-storage paths through bounded blocks and immutable disk generations; simultaneous scratch, separately admitted analysis and actual loop/ALL live budgets remain distinct. The [validation report](../openspec/changes/archive/2026-10-30-adopt-pad-owned-pcm-residency/evidence/long-source-streaming-validation.md) records actual long/Boris/RF64 evidence and its final acceptance state. Demucs inference/model/activation peaks are not bounded by postprocessing claims.

The bounded [L1 identity delivery](../openspec/changes/archive/2026-10-30-extend-shared-stem-generation/evidence/prepared-stem-publication.md#l1-effective-generation-identity--2026-10-11) resolves complete actual producer provenance before control-owned duplicate admission. Equal known fingerprints join one physical inference or reuse a verified completed pair; current verified selection disables duplicate generation without render-time hashing. Optional durable v3 provenance preserves historical UNKNOWN and model-free restore. Settings/material ABA fences and independent source/ticket interests prevent obsolete jobs from selecting newer content.

X11-LIVE-STEMS implements source-leased offline generation during playback, joint
disk/config commit and separately guarded new-set adoption in the supported
residency domains. Each candidate needs its own current source, window, voice,
history and Native acknowledgement; older assignments/readers retain immutable
predecessors until actual last use. Ordinary direct active component publication
remains guarded. The [prepared publication report](../openspec/changes/archive/2026-10-30-extend-shared-stem-generation/evidence/prepared-stem-publication.md)
records the bounded software proofs and remaining domain, resource and Human
acceptance limits; these are not a combined installed-inference/device/hearing
acceptance claim. X11-BULK's all216 loaded slots/all banks, bounded fair2-worker/
32-queue feeding and own-interest cooperative cancellation remain separate
requirements. Existing model installation and model-free validated restore remain unchanged.
