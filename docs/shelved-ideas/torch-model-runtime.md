# Former Torch model runtime

Looper used separate Python model workers for automatic BPM and offline stem
separation. The 2026-11-14 cutover replaced those routes with isolated native
ONNX workers to retire the Python tensor runtime, wheel-specific CUDA stack,
codec ABI checks and duplicate worker environments. Beat This, BS-RoFormer
MUSDB18HQ and HTDemucs moved in separate qualified steps. The operator accepted
Beat This and BS-RoFormer in the app and HTDemucs with its separate local ONNX
starter. Their earlier numerical and execution failures remain attached to
their original runs and gates.

The [current architecture](../architecture.md) and [model worker setup](../model-worker-setup.md)
describe the supported runtime. The recovery material here is historical.

## Data flow and model routes

- **Beat This BPM.** The loader handed the isolated worker a sealed, complete
  mono PCM source. The Python worker used the pinned `beat-this==1.1.0`
  `final0` model, soxr frontend and minimal postprocessor. CPU float32 used
  profile `legacy-cpu-v1`. The separately staged `cuda-auto-v2` worker could
  report successful CUDA execution and allow a new CPU attempt after a
  qualifying CUDA failure and actual retirement. The installation's
  `current.json` pointed at a verified installation generation. The host
  retained scalar BPM selection, QM corroboration or approximate fallback,
  and independent KeyNet analysis. This route became the native Beat This
  `final0` ONNX worker with ORT 1.24.4, libsoxr and an installation-owned
  CUDA cache keyed to the verified worker, model, package, driver and GPU.
- **Demucs `htdemucs`.** The controller captured source and target geometry,
  device policy, shifts and overlap, and ran the stock Demucs 4 model through
  `flitzis_looper.demucs_streaming` in an owned child. The adapter installed
  scoped cancellation, input tools and PCM16 FFmpeg output around the
  upstream CLI. Source decode used FFmpeg/FFprobe with the stock alternate
  TorchAudio conversion path. The normal PCM16 WAV writer piped bounded
  blocks to FFmpeg and retained the original rounding and clipping policy.
  The TorchCodec import preflight still gated the former runtime, and
  non-PCM16 explicit outputs delegated to the stock writer. Settings
  defaulted to one shift and 0.5 overlap, within 1–20 and 0.25–0.95.
  The native replacement uses separate CPU/CUDA graphs, a qualified Windows
  worker and FFmpeg first-stream decode. The ONNX transition had a temporary
  TorchAudio/Julius decoder fallback in `htdemucs_codec.py` after a missing
  or failed FFmpeg command. That decoder did not attest the FFmpeg producer.
- **BS-RoFormer MUSDB18HQ.** The host used a small vendored Python package
  with a byte-for-byte network from Music-Source-Separation-Training v1.0.12.
  FFmpeg decoded 44.1 kHz stereo. The worker used 485100-frame chunks, overlap
  factor two, batches up to two, reflection or short-tail zero padding, and
  a CPU overlap-add ring. First and last fade adjustments belonged to
  individual chunks. The model ran CPU float32 or CUDA AMP, and the worker
  wrote four full PCM16 WAVs. Its replacement uses the same pinned inputs,
  a native STFT and overlap-add owner, a CPU FP32 and a CUDA AMP graph, and
  the locally derived external `weights.bsnn` file.
- **KeyNet.** The production route was already native CQT and ONNX Runtime.
  Its standalone model converter belonged to offline regeneration.

In all stem routes the controller owned source leases and private attempts,
mapped `other` to Melody and generated the fifth, non-vocal Instrumental
artifact. Alignment, target frame extent, verified disk-set commit, persisted
selection and effective PAD OwnACK belonged to the common publication path.
A result's producer and its actual device were distinct from eligibility and
requested settings. Completed saved sets could restore from their verified
artifacts without inference.

## Dependencies, resource policy and startup

The main CPython 3.14 lock used `demucs>=4.0.1,<5`, the local
`flitzis-bs-roformer==0.1.0` package, Torch 2.12.0, TorchAudio 2.11.0 and
TorchCodec 0.13.0. On Windows the explicit `pytorch-cu130` uv index selected the
Torch and TorchAudio wheels. The BS package pinned Einops 0.6.1,
Rotary-Embedding-Torch 0.3.5 and Beartype 0.22.9. Beat This used separately
locked Windows x64 CPython 3.12.13 environments, Torch and TorchAudio
2.8.0+cpu or 2.8.0+cu128 with the respective dedicated indexes. Both used
Beat This 1.1.0, NumPy 2.2.6, soxr 0.5.0.post1, Einops 0.8.1 and
Rotary-Embedding-Torch 0.8.9. The historical locks identify all transitive
packages and hashes.

Stem generation ran away from the audio callback. A background Torch probe
reported CUDA availability, identity and available VRAM. Admission used the
captured policy and current resources, then a completed child receipt could
confirm actual execution and inform calibration. CPU-only skipped CUDA
probing. An exact, classified CUDA failure allowed a separately owned CPU
attempt after process tree and readers retired. Missing assets, codec and
host failures, cancellation and unclassified termination had their own
outcomes. Windows jobs and the Linux subtree broker supervised child
processes. Historic diagnostics that could not establish a producer or an
actual device did not replace those proofs.

Source setup downloaded and verified assets through `scripts/setup-runtime.py`
and the lower-level Beat This setup module. Source stem inputs defaulted to
`models/torch/hub/checkpoints/`, with a BS-RoFormer subdirectory and an explicit
`FLITZIS_MODEL_CACHE_DIR` override. The child used `TORCH_HOME` for the Hub
layout. Managed local Release bound its prepared main interpreter, Native
extension, DLLs, codecs, model assets and isolated Beat This worker with
workspace receipts outside Git.

The Windows source BAT files and `INSTALL-LOOPER-PORTABLE.bat` used the
matching source checkout and runtime. Portable staged its own interpreter,
wheel tree, Torch DLLs, shared FFmpeg/FFprobe and isolated Beat This resources.
Its start check imported the declared runtime packages, checked DLL origins
and tested the codec import before opening the app. Model preparation verified
the local stock checkpoints with CPU loads. Update and repair staged a
candidate and journaled selection and data custody. Recovery could reuse the
previous runtime and the older worker pointer. The separate development bundle
builder inventoried the runtime payload and licenses.

## Pinned upstream inputs

| Historical input | Bytes | SHA-256 |
| --- | ---: | --- |
| Beat This 1.1.0 `final0.ckpt`, source revision `b95c8ab0c58c2d9fcfd40508ae8dffbc05ac4f5c` | 81,058,141 | `8c328b45f59d8dd3dff219253ff6a8d6482be57d0133a29140e2febbf8eb8331` |
| HTDemucs `955717e8-8726e21a.th` | 84,141,911 | `8726e21a993978c7ba086d3872e7608d7d5bfca646ca4aca459ffda844faa8b4` |
| BS-RoFormer `config_bs_roformer_384_8_2_485100.yaml` | 4,566 | `d8afb980318d0c08b9c2e24a7adc00d4f3150320c127a7e4de861800d1321939` |
| BS-RoFormer `model_bs_roformer_ep_17_sdr_9.6568.ckpt` | 527,385,512 | `3e9daecd70aaed5b5a0d1f861cc4d77eaa45afb3fc6301b1cf32c1be0f5868fb` |

The HTDemucs and BS-RoFormer checkpoint identities still govern local ONNX
weight derivation. Their pinned upstream bytes are acquired directly and
verified before use. They are outside source distributions and installable
bundles. The source-code licenses and the rights for model weights are
distinct.

## Git recovery references

- `1590e4c3e85bfbbafd63521f083068e1631214c1` is the restored, working
  pre-cutover Torch source baseline. `milestone-backup` at
  `70aec34fad60440a780be1d4aa0e50953be41878` preserves the later documented
  Torch baseline. Check the exact commit before reproducing historical runs.
- Read those commits' `pyproject.toml`, `uv.lock`, `workers/beat_this/` and
  `workers/beat_this_cuda/` for the environment locks and
  `vendor/bs_roformer/NOTICE.md` for the former network inventory.
  `src/flitzis_looper/analysis/artifacts.py` records Beat This provenance.
  `src/flitzis_looper/controller/model_pins.py` and `bs_roformer_assets.py`
  record stem-source pins. `scripts/portable/` and `scripts/setup-runtime.py`
  show the corresponding startup and installation contracts.
- `f7f51438fbee387143f4461fae6b57947deee4ee` moved Beat This to ONNX.
  `2622324b` cut over BS-RoFormer and `30cb8541` recorded its live acceptance.
  `9864ec9fa80820246891cbc86d916334b381b209` enabled HTDemucs ONNX and
  `b5c0aef1e618cf71890b10c6d2784f8f1fcf3751` paired source with its
  independently built worker. The last commit still contains the old
  environment, reference tooling and temporary alternate codec.
- At `b5c0aef1`, `scripts/beat_this_trial_reference.py`,
  `scripts/bs_*_trial_reference.py`, `scripts/demucs_*_trial_reference.py`,
  `scripts/export_beat_this_trial.py` and `scripts/export_htdemucs_trial.py`
  retain the historical comparison and export entry points. The
  [archived isolation design](../../openspec/changes/archive/2026-11-13-improve-performance-isolation/design.md)
  records scoped evidence, including failed and unknown gates. The temporary
  plan is available at `b5c0aef1:docs/plans/torch-replacement.md`.

For a reproduction, create a separate checkout at the verified historical
commit and provision its exact locked interpreters, Native build, FFmpeg
and worker dependencies with the setup tools at that revision. Use a
separate scratch and data root, verified upstream checkpoints or immutable
verified backups, and a copy of any test project. Keep retained old
runtime receipts paired with the exact binaries and model installations
they described. Verify preflight, worker identity and readers before
running historical inference. A Portable installer BAT fetches the current
remote main by design, so use a fixed historical checkout's matching
scripts and resource layout for a deliberate reproduction.
