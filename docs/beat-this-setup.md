# Beat This worker profiles

The installed worker serves App BPM analysis and normal Analyze through the
Native neural-analysis job. Manual/TAP and cached results keep their existing
precedence. Source-map preparation and raw-review editor controls are removed.
Runtime/device checks do not certify musical accuracy.

The two closed profiles are `legacy-cpu-v1` and `cuda-auto-v2`. The legacy source
project and its scientific request/raw bytes remain unchanged. The CUDA profile
uses the same model and CPU frontend with a separate pinned Torch runtime.

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

The new `workers/beat_this_cuda` project pins Torch/torchaudio2.8.0+cu128 in a
single CUDA-capable Python3.12.13 environment. Its lock SHA-256 is
`0a662b8a7b4058899bc8eb0b775d186bc1e5acf6f101ec2186d3f998c84d0f30`.
It keeps CPU logmel preprocessing and minimal CPU postprocessing, transfers the
float32 model and inference tensor to CUDA0 when usable, and disables TF32.
CUDA availability/probe/placement/inference failure permits one fresh CPU model
attempt in that same worker. Other failures and cancellation do not trigger retry.
The completed receipt records actual device, GPU name, Torch/CUDA build and a
coded fallback reason. It does not promise bit-identical results across devices.
Package download and installation have no total elapsed-time limit, for either
profile. Slow Internet alone must not cancel the operation. Keep resumable
download bytes when recovering from an interrupted transfer; verify the pinned
hash before use. The user can still interrupt setup. Local environment inspection
retains its two-minute limit, independently of inference and audio-process guards.

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

The application binds this documented sibling installation for normal BPM
analysis. Missing or invalid installations are reported without silently falling
back to QM. Worker inference retains complete Beat This observations for the
stable-section estimator; it does not create playback maps.

For offline provisioning, supply `--checkpoint-file <absolute-final0-path>`
and `--offline`; the pinned interpreter and wheels must already be cached.
Missing cache contents fail explicitly. A supplied checkpoint is still hashed
against the accepted manifest; its filename conveys no trust.

Setup verifies a staged installation and publishes an atomic `current.json`
pointer to a unique installation directory. Failed setup leaves the prior
selected installation intact. Existing versions remain available to processes
that already own them; setup does not delete a running worker's files.

`--profile cuda-auto-v2` explicitly provisions the CUDA-capable profile; omission
retains the legacy CPU setup behavior. Switching the real App pointer requires
the matching protocol-capable Native and the existing closed-App/held-Native
guard. Retain the previous Native and pointer together for rollback; restoring
only one half is insufficient. A failed staged setup leaves the old pointer.
The active CUDA-capable environment also serves CPU fallback, so no second active
CPU environment is required. Keep the old CPU installation temporarily for paired
rollback rather than making a new full environment for each test.

To prepare a runtime while keeping the selected installation and Native intact,
add `--stage-only`. This retains a verified `installation-<UUID>` directory and
returns that new configuration directly. It neither reads nor writes `current.json`
and does not require the loaded Native to support CUDA. For example, append
`--profile cuda-auto-v2 --stage-only` to the setup command above. No App or audio
device is started. This is inactive preparation, not permission to replace a held
Native or start production analysis with an incompatible pair.

Later, after arranging the matching Native under the closed-App guard, select
that exact retained directory with:

```powershell
uv run python -m flitzis_looper.analysis.setup `
  --install-dir "$workspacePath\analysis-runtime\beat-this" `
  --scratch-dir "$workspacePath\scratch\b1b\setup" `
  --activate-installation "$workspacePath\analysis-runtime\beat-this\installation-<UUID>"
```

Activation rechecks the known manifest, files and checkpoint of a direct child of
the installation root. CUDA activation checks Native execution capability2 just
before the atomic pointer change. CPU rollback retains compatibility with the old
Native. Do not supply profile/checkpoint/offline options during activation; the
validated retained installation determines its profile. Python callers can use
`install_worker(..., stage_only=True)`, `load_staged_worker_configuration(...)`
and `activate_worker_installation(...)` without deriving a staged configuration
from the currently active pointer.

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
the legacy CPU `model=configuration.model` and `adapter=BeatWorkerAdapter(configuration)` to
`OfflineAnalysisService.start`. Retain the job/service until actual retirement.
Configuration loading reads local provenance only; it does not install or start
the worker. See [Offline analysis](offline-analysis.md) for ownership and limits.
The scientific protocol-one service remains a CPU reference. Protocol-two App
analysis uses the Native neural-analysis bridge, not that historical service.

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

### Local CUDA execution check, 2026-10-27

The known CUDA profile completed four authorized full-source mono inputs on an
RTX 5090 Laptop GPU and on forced CPU. Beat/downbeat positions and final tempo
estimates matched in all four cases. CPU logits matched the legacy CPU reference;
CUDA logits differed by at most 0.00007582. These are bounded comparisons on the
same FFmpeg reference PCM, not Native decoder or musical/hearing acceptance.

Two later full-source runs took 4.34/4.57 seconds on CUDA versus 20.53/20.37 seconds
on CPU, including supervision. The first CUDA run took 19.67 seconds versus 13.59
seconds on CPU; startup overhead matters. These observations are not a general
speed guarantee. Hiding the GPU produced a completed CPU fallback receipt.
Cancellation and the local worker deadline retired real owned processes; those
early-abort cases do not prove an already-running CUDA context was exercised.

Actual Demucs completed on CUDA and forced CPU with retained execution receipts.
BS-RoFormer device propagation has source/unit coverage only in this delivery.
Historical cached analyses/stems remain device-unknown until a new execution
provides a receipt. No existing cache is relabelled from CUDA availability alone.

The matching native Release and verified CUDA installation were delivered under
the closed-App guard with the previous Native/pointer pair retained for rollback.
The startup check passed without launching the App. A subsequent compiled,
headless public Unicode-FLAC import/OwnACK, reanalysis and restore test passed
with CUDA execution and the unchanged estimate 91.89220756855694 BPM. This tests
the Native bridge; it does not certify a listening session or SYNC alignment.
