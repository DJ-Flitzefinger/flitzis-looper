# Immutable source beatmap artifacts

## Why
The supported 64 MiB map envelope cannot be embedded in the existing 8 MiB
project image, 16 MiB journal or 32 MiB Copy envelope. Verification already uses
the original bounded native cold workers; durable out-of-line storage and fresh
source-qualified restoration are missing prerequisites for compact selection.

## Changes
Add typed immutable map sidecars, compact portable receipts and source-bound
store/restore work to that same native job and private completion path. Retain
actual sealed file custody and the existing ProjectAssets pin for output readers.

## Non-goals
This change does not enable project selection, Config/Copy/Bank persistence,
automatic collection, migration of selected maps, a live variable-rate map, a
second parser/job pool or inference. Their full existing obligations remain.
All file, hash, parse and retirement work stays off realtime and the UI thread.
