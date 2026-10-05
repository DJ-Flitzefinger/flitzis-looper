# Optional Beat This reference worker

B1b provides an explicitly installed diagnostic worker. Automatic loading, normal
Analyze, saved grids and live timing still use the existing behavior. B2 must
accept quality/resources before changing the new-analysis default.

## Locked Windows runtime

The separate `workers/beat_this` project pins Python 3.12.13, Beat This 1.1.0,
Torch/torchaudio 2.8.0 CPU, NumPy 2.2.6, soxr 0.5.0.post1, einops 0.8.1 and
rotary-embedding-torch 0.8.9. `uv.lock` pins the complete dependency closure and
distribution hashes. This initial environment supports Windows x64 only. It is
independent of the application's Python 3.14 environment; optional Beat This
setup does not remove the base application's existing Torch/Demucs dependencies.

The worker runs CPU FP32 with one Torch computation thread and one interop
thread. Every request gets a fresh isolated process. A warm rerun benefits from
filesystem/package caching, not a retained loaded model. There is no CUDA model
execution or model VRAM allocation in this configuration.

## Explicit setup

Run from the repository root. Keep the runtime/model outside the Git repository
and use absolute paths. For this workspace, the following uses local sibling
directories:

```powershell
$workspacePath = (Resolve-Path ..).Path
uv run python -m flitzis_looper.analysis.setup `
  --install-dir "$workspacePath\analysis-runtime\beat-this" `
  --scratch-dir "$workspacePath\scratch\b1b\setup"
```

This is the operation that can download the locked interpreter/packages and the
selected checkpoint. Normal startup, loading, restoration and analysis never
invoke it. `uv` uses its normal package/interpreter caches. Setup logs stay in
the requested scratch directory.

For offline provisioning, supply `--checkpoint-file <absolute-final0-path>`
and `--offline`; the pinned interpreter and wheels must already be cached.
Missing cache contents fail explicitly. A supplied checkpoint is still hashed
against the accepted manifest; its filename conveys no trust.

Setup verifies a staged installation and publishes an atomic `current.json`
pointer to a unique installation directory. Failed setup leaves the prior
selected installation intact. Existing versions remain available to processes
that already own them; setup does not delete a running worker's files.

## Accepted artifact and provenance

| Field | Value |
| --- | --- |
| Checkpoint | `final0`, 81,058,141 bytes |
| SHA-256 | `8c328b45f59d8dd3dff219253ff6a8d6482be57d0133a29140e2febbf8eb8331` |
| Acquisition | Explicit author-host HTTPS acquisition on 2026-10-05; independently rehashed locally |
| Configuration | Beat This 1.1.0, minimal postprocessor, CPU FP32 |
| Frontend | `beat-this-1.1.0-soxr-hq-logmel-v1` |
| Environment identity | SHA-256 of the shipped `uv.lock`, checked against installed package versions |

The accepted digest identifies the bytes inspected by this project; it is not
an author-published signed checksum. The source URL comes from the pinned
[upstream loader](https://github.com/CPJKU/beat_this/blob/b95c8ab0c58c2d9fcfd40508ae8dffbc05ac4f5c/beat_this/inference.py).
Upstream declares both code and published weights MIT in its
[license statement](https://github.com/CPJKU/beat_this/blob/b95c8ab0c58c2d9fcfd40508ae8dffbc05ac4f5c/README.md#license),
with the [license text](https://github.com/CPJKU/beat_this/blob/b95c8ab0c58c2d9fcfd40508ae8dffbc05ac4f5c/LICENSE).
The upstream training-data caveat remains relevant; this manifest does not
establish training-corpus rights or absence of benchmark overlap.

The installed receipt records the accepted artifact, source/license links,
script/project hashes, exact runtime package versions, Beat This Python source
hashes, interpreter launcher/configuration and distribution metadata hashes.
Preflight rejects changed installation provenance and missing/corrupt weights.
It does not rehash every Torch/NumPy native library on each request; runtime
dependency integrity starts with the locked distribution hashes at installation.
The worker rechecks package versions and hashes the same local file handle it
loads with `torch.load(weights_only=True)`. It never invokes upstream
shortname/URL checkpoint resolution or `torch.hub` code acquisition. Worker
Python socket operations are denied as an additional guard; this is not an
OS-level network sandbox.

## Diagnostic use

On a background/control path, obtain
`setup.load_worker_configuration(install_dir, scratch_dir)`, then pass
`model=configuration.model` and `adapter=BeatWorkerAdapter(configuration)` to
`OfflineAnalysisService.start`. Retain the job/service until actual retirement.
Configuration loading reads local provenance only; it does not install or start
the worker. See [Offline analysis](offline-analysis.md) for ownership and limits.

The worker reads complete shared mono at the actual loaded rate, performs
full-buffer soxr HQ conversion directly to 22050 Hz and uses the pinned centered
log-mel frontend. KeyNet independently uses the native 44100-Hz branch. There
is no trimming, fitted time shift or cascade through the key input. Resampled
length follows the reference's rounded length; a centered STFT can add a final
logit at the exclusive source end. All logits remain evidence, while detections
outside the original half-open source extent are omitted. Inputs with at most
512 resampled frames fail because reference reflect padding cannot process them.

## Verification

Run application checks as documented in [Development](development.md). The
worker's separate reference/frontend tests run with its pinned interpreter:

```powershell
uv run --project workers/beat_this --locked pytest
```

This development command explicitly provisions the test environment if needed;
it is never an application inference path. Frontend fixtures compare complete
first/last impulses, silence and fractional-frequency tones at 22050, 44100 and
48000 Hz against the installed upstream frontend. Real checkpoint inference,
resource observations and remaining acceptance gates are recorded in
[B1b evidence](beat-this-reference-evidence.md).

Set `BEAT_THIS_CHECKPOINT` to the absolute verified checkpoint path to include
the optional real-model parity test. Without it that test explicitly skips; a
skip is not evidence of model parity. The reference run sets this variable and
compares raw logits and minimal postprocessing over a chunk boundary.
