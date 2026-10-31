# Improve trigger latency and background-work isolation

## Why

With Key Lock and BPM Lock enabled, slowed non-master PADs can feel late on
retrigger and START/STOP. Stem generation or loading during playback can coincide
with audible glitches. Robust playback takes priority over background throughput.

The initial source audit identifies an explicit 8192-frame preparation lead, a
single selected-PAD endpoint reserve, global stem ownership locks spanning disk
work, Python sample-conversion loops and uncoordinated child CPU thread budgets.
These are concrete risks, not measured proof of every reported symptom.

## What changes

- Measure production input/capture, requested target, actual effect and rendered
  attack separately; aim for at most 30 ms software trigger-to-significant-output
  response on the declared ready 2/4/6-PAD workload, including slowed Key Lock.
  The Human clarified on 2026-10-31 that 30 ms is advisory, not an acceptance
  threshold. Seek the shortest practical latency with preserved quality and
  proportionate complexity; a small miss does not require further redesign.
- Prepare reusable start resources before presses, retaining exact source and
  operation guards, independent mutable voices and atomic group effects.
- Remove slow stem preparation from shared control locks and Python/GIL waits.
- When precise automatic BPM estimation fails, provide a simpler approximate
  source-derived BPM with a visible inaccuracy warning, as directly requested on
  2026-10-31. Preserve precise evidence separately from this fallback status.
- Port justified bounded stem conversion arithmetic to Rust, releasing the GIL
  while preserving existing byte-level conversion and publication contracts.
- Budget background CPU/thread use and retain mandatory supported NVIDIA CUDA
  acceleration with same-model CPU fallback; CUDA support is not discretionary.
- Investigate AMD-capable GPU providers and integrate a practical, equivalent
  path when supported; retain provider-independent requests and CPU fallback.
- Include Intel/AMD integrated GPUs and automatic hardware-aware selection per
  workload, using measured total cost and resource headroom rather than assuming
  a GPU is always faster. Preserve functional fallback to CPU.
- Audit PCM ownership, process memory, worker concurrency and offline GPU
  opportunities; implement only changes supported by before/after evidence.
- Keep Rust domain/processing code independent of Python-specific bindings so
  further migration, reduced Python/PyTorch footprint and future Windows/Linux/macOS packaging
  are easier. Attempt practical PyTorch removal/native model-runtime replacement
  with CPU/CUDA and quality parity; preserve every supported function and interaction.
- Update existing architecture/development documentation with the implementation
  and its measured limits. Keep historical evidence and unpassed gates intact.

## Non-goals

No restoration of retired SYNC/phase-following features, plugin hosting, new FX,
automatic private-data migration or deletion. No mandatory NVIDIA hardware,
callback GPU synchronization or unsupported universal device-latency guarantee.
Broad Rust migration and larger engine redesign are authorized when useful.
Complete native parity, CPU/CUDA release packages and honest Windows/Linux/macOS
evidence remain required before productive runtime cutover; partial trial steps
do not satisfy them. Preserve the single transport and source authority.

## Delivery

Twenty-eight coherent steps are recorded in [tasks](tasks.md). STEP18's real native
BeatThis chunk experiment is followed by STEP19's complete-source CPU prototype.
STEP20 adds original input-envelope parity, narrow failure diagnostics and bounded
actual CPU termination. Unsuccessful CUDA initialization remains explicit with
cause UNKNOWN. STEP21 adds explicit runtime admission and STEP22 ends unused
standalone host-buffer lifetimes. STEP23 adds custody of explicitly declared ORT
package files. STEP24 adds opt-in Windows standalone loader policy and actual
observed backing-file identity without a closure or CUDA claim. STEP25 implements
an independent BS complex spectral frontend and reconstruction without model
inference. STEP26 implements the independent wholetrack reflection, tail padding,
batching, fades and rolling overlap-add around a synchronous predictor boundary.
STEP27 retains every native CUDA/freshCPU/quality/producer/platform/distribution
gate before cutover; FINAL28 retains ALL former FINAL27 requirements.
Each important step runs
in its own chat, finishes its appropriate checks and independent reviews, then
commits and normally pushes to `performance-upgrade` before its successor starts.
This assignment does not merge or push these changes to `main`.

This proposal and its deltas describe planned behavior. The initial audit changes
no runtime code and establishes no performance, device or listening acceptance.
