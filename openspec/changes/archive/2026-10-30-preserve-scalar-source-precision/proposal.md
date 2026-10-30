## Why

Long-file waveform, seek and playhead interfaces narrow source seconds to binary32,
losing individual loaded frames. Scalar display, snapping and automatic loop ends
also use separate arithmetic, while a rounded BPM edit buffer can silently replace
the effective tempo without a deliberate edit.

## What Changes

- Reuse the pure Rust source-grid arithmetic for a binary64 scalar control evaluator.
- Route visible lines, snapping and automatic loop endpoints through that projection.
- Preserve binary64 source seconds through waveform queries/X, seek commands and
  playhead telemetry; keep amplitudes binary32 and physical addresses integer frames.
- Preserve effective BPM on untouched edits and use its authoritative resolver for
  MIDI runtime metadata and invalidation signatures.

## Non-goals

No new tempo estimator, model cutover, variable map, persistence schema migration,
native BPM/rate representation redesign, musical loop-wrap change, SYNC/KEY feature,
GPU installation or full Rust port. The automatic 120.001289 BPM slope remains a
separate known defect. Native live BPM/rate still use their existing binary32 values.

## Realtime Constraints

Projection/waveform work runs on the control side. Seek and playhead messages remain
fixed-size scalars. The callback performs no allocation, blocking, IO, logging,
analysis, inference or Python/GIL access.

## Impact

Waveform editor, loop-region, pad-manual-bpm and input-mapping contracts; native API
typing, source address helpers and maintained architecture/UI/native documentation.
The proposed variable-map editor change remains pending and separate.
