# OpenSpec archival disposition — 2026-11-13

Both changes were present in the published Torch baseline `1590e4c3` and were
introduced after the October 30 archive. This disposition applies to their
records at `09adc068`. The original tasks, recorded failures and unresolved
outcomes retain their status.

## Current device and listening acceptance

On November 13, 2026, the project owner directly confirmed that the device and
listening checks had been completed and that the current Looper worked
perfectly, with no problems. This is the owner's acceptance of the current
Looper. That confirmation did not specify a build, measurement, device
configuration or test matrix. The archived runs and checklists retain their
original results and scope.

## configure-audio-output

**Audio-output configuration and restoration delivered.**

- `5575b43e` added configurable native ASIO and WASAPI, requested and observed
  settings, stopped Apply, source restoration and rollback. `91a89242` delivered
  cross-rate restoration for assigned stem pairs, including Auto. This supersedes
  the initial fixed-rate restriction described in the archived proposal.
  `91983718` added restoration of speed, BPM Lock and PAD selection after reopen.
- The published Torch baseline has a dated release and headless startup preflight.
  The preflight ended before App launch, audio device acquisition or inference.
  The six Wet and six Off software fixtures used injected output. The historical
  checklist retains its unchecked physical ASIO at 96 kHz and 96 frames,
  six-PAD device comparison, listening, and inference-overlap entries.
- The [current output specification](../../specs/audio-output/spec.md) carries the
  shipped behavior. The original [proposal](2026-11-13-configure-audio-output/proposal.md),
  [design](2026-11-13-configure-audio-output/design.md) and
  [checklist](2026-11-13-configure-audio-output/tasks.md) retain 15 completed and
  five unchecked tasks.

## improve-performance-isolation

**Partially delivered. The broad implementation program is retired.**

- Delivered work includes prepared starts and ordering, detached stem conversion,
  shared heavy-job admission and owned reader retirement, approximate BPM
  fallback, call-owned KeyNet sessions and the one-shot workload report. The
  current background, analysis, playback and UI specifications retain the
  supported contracts.
- Stock BS ran on CPU and CUDA with six software-paced PADs at 48 kHz and
  512 frames in `90d5b30e`. A separate scoped run in `c0968734` verified real
  CUDA failure and fresh CPU fallback. The [96-frame software mixer measurement](2026-11-13-improve-performance-isolation/design.md#original-steady-render-thread-service-at-96-frames)
  exceeded the nominal 1 ms interval on all measured shift-bearing callbacks in
  the four- and six-Wet cases. The dated 2/4/6-PAD latency, deadline and overlap
  matrix and up-to-ten-PAD stress retain their original incomplete status.
  The earlier failed rows, distinct watchdog result, unexplained registry outcome
  and the original physical-output and listening checklist entries remain in the
  [checklist](2026-11-13-improve-performance-isolation/tasks.md#67-combined-regression-and-final-architecture-review).
- The supported Torch paths remain available. Guarded Native BeatThis paths and
  experimental stem paths survive in source. The Native HTDemucs CPU release
  lacks the required graph and worker approval records. Full Torch replacement,
  Intel/AMD provider coverage and platform, quality and distribution parity remain
  unresolved. A future runtime transition requires its own focused change and
  evidence before any productive cutover.
- The original [proposal](2026-11-13-improve-performance-isolation/proposal.md),
  [design and measurements](2026-11-13-improve-performance-isolation/design.md) and
  [checklist](2026-11-13-improve-performance-isolation/tasks.md) retain 245 completed
  and 30 unchecked tasks. Historical memory limits remain evidence of their runs.
  Current Windows host admission follows observed resources and OS pressure.
