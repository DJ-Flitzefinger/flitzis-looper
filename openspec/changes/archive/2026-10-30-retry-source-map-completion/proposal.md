## Why

A content placement publication can start after the loader's first owner lookup
but before the original accepted-map job resolves its private completion. The
second lookup reports pending placement; the current dispatcher loses that event,
leaving an otherwise completed selection or restore pending indefinitely.

## What Changes

Retain the same genuine accepted-map completion in the existing held-event queue
for the next bounded poll when its private resolution reports the exact placement
pending sentinel. Preserve the pending job, reservation, prior selection and
effective geometry until genuine resolution succeeds or ownership becomes stale.

## Non-goals

No new job, queue, completion authority, map format, musical qualification or audio
adoption. No retry of unrelated errors or weakening of stale/cancel checks.

## Realtime

The change is control-side event routing only. No callback, Native binary, worker,
allocation or retirement contract changes.
