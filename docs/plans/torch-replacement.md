# Replace the productive Torch runtime

This temporary plan expands the open [Torch TODO](../todos.md). The first
replacement effort left guarded Native paths and model components in source
and did not pass the cutover gates. The working Torch routes remain in use.
The device and listening acceptance for the current Looper is recorded in the
[OpenSpec archive index](../../openspec/changes/archive/2026-11-13-archive-index.md).
A replacement requires separate authorization, a focused OpenSpec change and
evidence against that baseline. This plan expires when all productive Torch
dependencies are removed and the replacement is verified.

## Productive routes and existing replacements

| Pipeline | Current selection and dependency | Native entry point and release status |
| --- | --- | --- |
| Beat This `final0` | The App binds an installation root. Its worker bridge verifies the digest of the installation selected by `current.json`. Normal Windows x64 source setup defaults to `legacy-cpu-v1` and supports the locked Python 3.12 Torch 2.8 CPU and `cuda-auto-v2` cu128 profiles. Portable prepares an installation-owned movable Python worker. | The App bridge already dispatches to an explicitly staged and activated Windows x64 Native CPU or NVIDIA CUDA package with a matching App extension. It uses the pinned dynamic `final0` ONNX graph, libsoxr and ORT 1.24.4/API 24. The CUDA package carries separate CUDA 12/cuDNN 9 and CPU runtime directories. Its default `cuda` policy is strict. `auto` allows one same-graph CPU retry after a qualified CUDA inference fault and full retirement of the failed attempt. [App binding](../../src/flitzis_looper/controller/loader.py), [source setup](../../scripts/setup-runtime.py), [worker bridge](../../src/flitzis_looper/analysis/neural_worker.py), [Native package and supervisor](../../src/flitzis_looper/analysis/native_beat_worker.py), [CUDA inventory](../../workers/beat_this/native_cuda/step23-package.json). |
| Demucs `htdemucs` | Saved Demucs selections use the stock model behind the streaming adapter. The App worker uses the Python 3.14 Torch stack with TorchAudio, a TorchCodec preflight, and FFmpeg/ffprobe for the production PCM path. | Windows has an explicit `FLITZIS_HTDEMUCS_NATIVE_CPU=1` route to the FFmpeg decode, Native CPU worker and normal aligned cache pipeline. The graph and executable approval records both contain `null`. Selection fails before decode or worker launch while approval is absent. There is no Native HTDemucs CUDA route. [Selection](../../src/flitzis_looper/controller/stem_separators.py), [Torch adapter](../../src/flitzis_looper/demucs_streaming.py), [preflight](../../src/flitzis_looper/controller/stem_generation.py), [Native adapter](../../src/flitzis_looper/controller/native_demucs_cpu.py), [graph approval](../../workers/htdemucs/native_cpu/release.json), [worker approval](../../workers/htdemucs/native_cpu/worker.json). |
| BS-RoFormer MUSDB18HQ | Fresh projects use this separator. The vendored worker loads the pinned configuration and checkpoint with strict weight matching, decodes through FFmpeg and runs CPU FP32 or CUDA AMP. | Rust components provide the stereo spectral frontend, track orchestration, the learned CPU network and a fixed-checkpoint exporter. The App has no Native BS worker selection. [Model choice](../../src/flitzis_looper/models.py), [asset pins](../../src/flitzis_looper/controller/bs_roformer_assets.py), [App adapter](../../src/flitzis_looper/controller/bs_roformer_generation.py), [vendored worker](../../vendor/bs_roformer/src/flitzis_bs_roformer/__main__.py), [track](../../rust/crates/stem-dsp/src/track.rs), [learned core](../../rust/crates/stem-dsp/src/learned.rs), [weight export](../../scripts/bs_learned_weights.py). |
| KeyNet | The App uses Rust CQT and ONNX Runtime with the tracked graph. The standalone exporter uses Torch when regenerating the model. | Keep the existing runtime path and verify its independent outcome, including when Beat This fails. [KeyNet inference](../../rust/crates/analysis/src/key_detection/inference.rs), [graph](../../assets/models/keynet.onnx), [offline converter](../../scripts/convert-musical-key-cnn.py). |

The stem adapters share provider policy and receipts. The guarded capability
probe still imports Torch. Replace that probe with one that identifies the
actual Native provider, driver, device and usable VRAM for the proposed worker.
Retain the backend's explicit CPU request, the App's automatic capability
and VRAM selection, and passive cost learning from complete comparable
executions with verified producer identity. A recognized CUDA failure may start a fresh CPU attempt of the same
chosen separator and model. Cancellation, invalid input or artifacts, host
resource refusal and other generic failures keep their existing terminal
semantics. [Probe](../../src/flitzis_looper/provider_probe.py),
[provider selection](../../src/flitzis_looper/controller/stem_provider.py),
[failure classification](../../src/flitzis_looper/inference_provider.py),
[execution receipts](../../src/flitzis_looper/stem_execution_protocol.py).

## Complete and qualify each worker

1. **Beat This.** Use the existing Native App bridge and dynamic `final0`
   export. Compare the original locked workers and the proposed Native workers
   on the same complete, sealed App-converted mono source, retaining SHA-256,
   sample rate, frames and full tail. Cover soxr, log-mel, true short and
   full chunk lengths, border aggregation, logits, minimal postprocessing,
   beat/downbeat positions and the App's scalar BPM, uncertainty and range
   decisions. Anchor quality to independent labeled inputs and musical
   comparisons as well as original-worker output. Keep KeyNet result merging,
   manual/Tap precedence and source-qualified timing publication. Qualify an
   actual Native NVIDIA run, a real CUDA device fault that reaches fresh CPU
   execution, CPU-only and mid-inference cancellation through the App.
   The archive records passing scoped CPU full-source cases and earlier CUDA
   load/crash attempts with unknown cause. Selectable Native CUDA and its
   receipt code still need release-level inference and fallback evidence.
   [Native worker](../../rust/crates/analysis/src/beat_worker.rs),
   [CUDA worker support](../../rust/crates/analysis/src/beat_worker_cuda.rs),
   [graph exporter](../../scripts/export_beat_this_trial.py),
   [analysis contract](../../openspec/specs/audio-analysis/spec.md),
   [CPU comparison](../../openspec/changes/archive/2026-11-13-improve-performance-isolation/design.md#step19-impact-complete-source-native-cpu-prototype),
   [remaining gates](../../openspec/changes/archive/2026-11-13-improve-performance-isolation/tasks.md#66-remaining-native-parity-before-cutover),
   [reference evidence](../../openspec/changes/archive/2026-10-30-evidence-index.md#evidence-groups-and-associated-changes).
2. **HTDemucs.** Finish the pinned B1 Scan64 CPU candidate before enabling its
   release records. Use the pinned original checkpoint. Run reference, trace,
   assembly, rewrite and full checker in separate owned phases. Bind
   both normalized inputs, learned spectral and waveform decoder outputs,
   complete reconstructed PCM, trace and graph bytes before approval.
   The prior producer completed those graph phases and the learned CPU core
   ran. Its direct PCM met tolerance in the scoped comparison while both
   decoder branches had out-of-tolerance values. Resolve those differences and
   rerun the whole learned path. Keep the separate High-DC failures visible and
   qualify the applicable reduction and FFT dispatch across supported input
   geometry. The independent resampling, hybrid reconstruction, split,
   overlap, shifts and weighted-bag code gives the physical host boundary.
   Compare the actual decoder, mono/stereo/channel mapping, complete resampled
   source, CLI and branch statistics, supplied matching shift offsets,
   segment/tail/crop, stem ordering, output clipping and final encoded PCM.
   Add a Native CUDA provider route for that same pinned HTDemucs model with
   full learned operator, branch, whole-track and signal-quality proof before
   switching the App default for Demucs. Seal the attempted original and the
   actual graph/runtime. Use the existing source and publication owner to
   align and commit the five artifacts. The current Native adapter has a
   separate codec and producer witness gap. Close it before granting reuse or
   calibration.
   [Export and checks](../../scripts/export_htdemucs_trial.py),
   [Native worker](../../rust/crates/analysis/src/demucs_worker.rs),
   [ORT adapter](../../rust/crates/analysis/src/demucs_inference.rs),
   [Native host kernels](../../rust/crates/stem-dsp/src/demucs.rs),
   [archive evidence](../../openspec/changes/archive/2026-11-13-improve-performance-isolation/tasks.md#offline-htdemucs-dual-branch-evaluation).
   The scoped learned comparison and retained branch failures are in
   `git show 1590e4c3:docs/development.md` under **Fixed HTDemucs learned-core
   export and CPU boundary**.
3. **BS-RoFormer.** Feed the existing learned CPU predictor with the exact
   pinned MUSDB18HQ weights and config, and integrate the spectral and track
   stages in an owned worker. Reuse the qualified FFmpeg decode contract and
   compare complete decoded geometry and stereo PCM. First compare learned
   masks for identical original features, then compare all four complete
   stem signals across 485,100-frame chunks, reflections, overlap two, batch
   two, short physical sources and final tails. Preserve independent fades,
   output crop and the alignment/cache contract. Add a CUDA network path
   from the same effective weights and compare against the actual original
   CUDA AMP route, including batch and tail behavior. Exercise the CPU build,
   successful Native CUDA and real CUDA-to-fresh-CPU fallback in the same model.
   The archived CPU arithmetic controls cover spectral and track operations.
   The original learned comparison failed during Torch import before inference,
   and both Native full-geometry learned attempts ended in allocation errors.
   Verify complete learned execution and resource use before treating these
   components as a separator.
   [Production inference](../../vendor/bs_roformer/src/flitzis_bs_roformer/inference.py),
   [decoder](../../vendor/bs_roformer/src/flitzis_bs_roformer/audio.py),
   [spectral core](../../rust/crates/stem-dsp/src/lib.rs),
   [learned attempt and export evidence](../../openspec/changes/archive/2026-11-13-improve-performance-isolation/design.md#step27-actual-export-controls-and-failed-execution-boundaries).

## Release and cutover gates

- **Source, quality and function.** Use identical captured full-source inputs
  with original and candidate executable identities and an independent quality
  corpus. Compare complete outputs at each affected stage, including tails,
  silence/failure, loop-rate playback alignment and long sources. Record signal
  and musical results, and obtain device/listening evidence for the resulting
  release separately from the current Human acceptance. Update the
  [analysis](../../openspec/specs/audio-analysis/spec.md),
  [stems](../../openspec/specs/stem-cache/spec.md) and
  [source alignment](../../openspec/specs/loop-source-stem-alignment/spec.md)
  contracts with the cutover. Keep source, content/request lifetime,
  registration, saved selection and effective audio OwnACK distinct. Exercise
  restore/reuse, both saved separators, Generate All, playback interaction and
  stale/cancel/late failure using the normal entry points. Verified selected
  historical results keep their recorded producer and device identities.
- **Runtime and custody.** Bind complete exporter, original model and weights,
  graph/operator code, frontend/postprocessing, decoder/encoder and actual
  worker and loaded runtime/provider image identities to versioned artifacts.
  Pin and seal the actual bytes and dependencies, including held FFmpeg and ORT
  images, until readers end. Keep unknown producer roles ineligible for
  completed-set cache reuse or cost learning. Replace both native and Torch-side
  witness assumptions where code changes. Check the existing shared background
  admission and actual process/tree/pipe/last-reader completion, including
  failed startup, timeout, late cancellation, GPU retirement before CPU and
  shutdown. Measure actual host peaks and respond to live OS pressure and
  allocation failure. Keep checked source/format and VRAM guards and keep this
  work off the realtime callback. Extend the existing worker and provider checks
  with actual execution controls.
  [Stem provenance](../../src/flitzis_looper/controller/stem_provenance.py),
  [background tasks](../../openspec/specs/background-tasks/spec.md),
  [Beat worker checks](../../src/tests/flitzis_looper/analysis/test_native_beat_worker.py),
  [stem provider checks](../../src/tests/flitzis_looper/controller/test_stem_provider.py).
- **Install, update and recover.** Qualify retrievable approved packages for the
  supported source and release setups and the self-contained Windows Portable
  path. Verify license/notice and redistribution provenance for the exact
  model, library and codec packages. Test clean CPU-only and CUDA installations,
  selected worker/runtime identity after move, download resume and digest
  verification, startup checks, repair, unchanged healthy Native reuse and
  update rollback that preserves private data and the previous runtime
  selection. Bind each actual OS/architecture and report untested platforms
  as unknown. Keep trial executables and private cache paths out of production
  setup assumptions. Update [source setup](../../scripts/setup-runtime.py),
  [Portable analysis](../../scripts/portable/prepare_analysis.py),
  [model readiness](../../scripts/portable/prepare_models.py),
  [launch and recovery](../../scripts/portable/launch.ps1),
  [dependency checks](../../scripts/portable/bootstrap.py),
  [Windows redistribution inventory](../../scripts/portable/windows-runtime-redistribution.json),
  [bundle build](../../scripts/portable/build_bundle.py) and
  [distribution requirements](../../openspec/specs/distribution-setup/spec.md).
- **Remove the productive dependency closure.** After all three routes and
  rollback are verified, switch the ordinary App and installer paths as one
  coherent delivery. Remove productive Torch/TorchAudio/TorchCodec, the
  Torch-dependent Demucs and vendored BS worker requirements, both Beat This
  Python runtime payloads, and their pins, loader checks, indexes and transitive
  DLL assumptions from the [App manifest](../../pyproject.toml),
  [App lock](../../uv.lock) and production setup/recovery. Keep only explicit
  reference or export tools that remain necessary in independently locked
  tooling with zero application/setup imports or redistributable Torch payload.
  Verify the dependency graph and actual loaded images through setup, playback,
  recording, analysis and both separators. Measure installed and downloaded
  footprint from the verified packages. Delete this plan, resolve the TODO and
  update the durable architecture and setup references to the shipped paths.
