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

Thirty-eight coherent steps are recorded in [tasks](tasks.md). STEP18-24 retain the
native BeatThis/runtime/loader experiments and their explicit failed CUDA and
incomplete producer limits. STEP25 implements independent BS spectral arithmetic;
STEP26 adds whole-track reflection, padding, batching, fades and rolling overlap.
STEP27 implements the experimental complete learned CPU network and strict inert
weight export, with original/native completion failures retained. STEP28 attributes
fallible allocations and shortens dead-buffer lifetimes. STEP29 reconstructs one
learned stem mask at a time into the complete batch destination. Full process
admission and practical learned/original parity remain unproved. STEP30 owns later
Rust allocation backing; STEP31 retains prepared I/O and typed error reporting
outside the sealed execution allocator. Complete allocation-sequence/fragmentation
and native process bounds remain required before a real learned retry. STEP32
retains one spectral execution workspace across complete serial samples, removing
repeated FFT/inverse preparation from the sealed path while keeping its retained
preNN process cost explicit. STEP33 closes the complete prepared-route arena-only
recipe against the SAME production allocator with sparse headers, actual target
metadata and exact free-list restoration. Native/process closure and fresh
sufficient ORIGINAL admission remain required before NN. STEP34 removes four
learned CRT transcendental paths using existing pinned Rust math, with operator
controls only. STEP35 removes implicit destructor writes from actual diagnostic
output owners while retaining explicit flush/sync and whole-private-output
invalidation. STEP36 moves the six retained spectral working-vector owners into
the SAME arena after consuming preselected FFT plans/windows, with a completely
new arena recipe and original exact-bit spectral controls. Selected plans/windows
and complete native/process backing remain open. STEP37
retains ALL former native parity duties, including mandatory CUDA/freshCPU,
complete Demucs, provenance, platforms, quality and distribution; FINAL38 retains
the ENTIRE former FINAL37, without omissions.
Each important step runs
in its own chat, finishes its appropriate checks and independent reviews, then
commits and normally pushes to `performance-upgrade` before its successor starts.
This assignment does not merge or push these changes to `main`.

This proposal and its deltas describe planned behavior. The initial audit changes
no runtime code and establishes no performance, device or listening acceptance.
