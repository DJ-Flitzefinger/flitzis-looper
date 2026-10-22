## Why

Complete original BeatThis capture and the initial explicit-label builder exist.
The builder creates a fresh map of 2–64 labels each time; it cannot progressively
extend a selected map or repair non-affine interior labels without losing earlier
sections. This blocks practical full-track review even though larger immutable
geometry is structurally supported.

The selected map already embeds its complete captured observations. A bounded
edit and readback on that same map can reuse the existing source-custody, worker,
artifact, publication and closed-Undo paths. The affected edit/parse path also
needs simultaneous-memory admission before restoring or copying a large map.

## What Changes

- Read bounded observations and accepted rows from a selected captured map,
  including after transient Raw discard, Copy and fresh restore.
- Extend either end or replace one bounded interior window with explicit labels.
- Preserve fixed endpoints, untouched intervals, source corrections and all Raw
  provenance; invalidate only newly formed intervals to uncertain/unknown.
- Extend closed Undo and shared checked memory admission through real edit work.
- Add modular English review controls through the existing selection/edit path,
  with source/revision/capture/range/draft gesture fences and Portable readiness.

## Non-goals

No automatic quarter labels, implicit trust/meter/phase, detector default cutover,
new analysis lease/pipeline/clock, audio admission, fully missed-beat arbitrary
sample insertion, full B2/musical or Human acceptance. Existing unsupported MoveBeat
insertion guards and all remaining program gates remain.

## Realtime

Raw indexing, JSON, edits, hashing, budgeting and retirement remain off realtime.
The callback still borrows immutable qualified geometry through existing owners.
