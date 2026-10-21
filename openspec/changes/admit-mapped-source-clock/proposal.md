## Why

The reported long vocal drifts against repeating short loops despite a correct
common start. Scalar BPM and a smoothed rate integrate their remaining error.
Existing source-qualified variable beat maps and their editor do not yet drive
the productive source clock, actual-read history or prepared launch path.

## What Changes

- Carry explicitly admitted immutable mapped trajectories through the existing
  ordinary/global launch, source reader, voice and preparation ownership paths.
- Address source samples from the authoritative master beat at their intended
  output frame, using the shared canonical inverse rather than integrated rates.
- Preserve fixed copied cursor/read receipts and separate qualified owning pins;
  retire displaced owners through the existing off-thread retirement queue.
- Keep exact musical loop length distinct from rounded physical markers.
- Expose the genuinely implemented tuple envelope honestly while the remaining
  Native pitch, continuous adoption and automatic-map acquisition are integrated.

This change creates an explicit audio-admission exception to the editor-only
restriction of `share-editor-beatmap-coordinates`. Selecting or editing a map
alone still grants no audio permission and does not move stored markers.

## Non-goals

No second master clock, scheduler, audio renderer or worker pool; no periodic
phase-correcting seeks, scalar local-BPM glide presented as mapped SYNC, vocal-
peak chasing, automatic acceptance of legacy beat arrays, one-shot EOF behavior,
silent Key Lock changes, new master BPM writes or claim of whole-SYNC acceptance.
The complete requested feature and all existing acoustic/device/resource gates
remain open until their actual integration and evidence are delivered.

## Realtime

All map parsing, identity/coverage admission, allocations, file custody and owner
creation remain off realtime. Realtime may use only bounded pure Rust queries on
already admitted immutable geometry, copied scalar segments and existing atomic
fences. It MUST NOT perform I/O, Python/GIL calls, blocking, allocation or final
destruction of the retained map/source owners.
