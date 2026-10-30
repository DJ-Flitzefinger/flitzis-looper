## Why

A Release user can arrange a stable three-pad remix but cannot extend its vocal
loop beyond roughly 99 bars: live resident PCM and retained old/new ranges are
incorrectly compared with the 1-GiB transient conversion limit. The independent
512-MiB default resident budget can reject the same ordinary sparse remix next.
The latest Human instruction requires admission based on the actual request and
available machine memory, rather than a hypothetical fully loaded 216-pad case.

## What Changes

- Make measured host-headroom admission the default for live resident PCM.
- Retain an explicitly selectable, persisted aggregate manual allowance.
- Account exact unique owners and outstanding allocation promises through the
  existing ledger; resolve immutable reuse before charging new allocation.
- Separate final live PCM from bounded conversion/streaming scratch on the sealed
  complete-reader paths. Preserve genuine legacy conversion bounds.
- Preserve prior usable audio, own ACK, cancellation and last-reader retirement
  when available memory cannot admit an actual new request.

This later change supersedes the fixed default residency and resident-as-scratch
interpretation in `adopt-pad-owned-pcm-residency`; its worker, actual scratch,
timing/analysis, capacity, ownership and realtime constraints remain binding.

## Non-goals

No callback disk streaming, complete-file RAM conversion, new worker pool, voice
eviction, automatic partial loop, SYNC implementation, one-shot EOF playback or
claim of multi-GB/music/device acceptance. Those real followups remain required.

## Realtime

OS observations, ledger locks, checked geometry, reservations and file reads run
only on the existing control/offline paths. Callback ownership and the renderer
do not acquire a new allocation, query, lock or file access.
