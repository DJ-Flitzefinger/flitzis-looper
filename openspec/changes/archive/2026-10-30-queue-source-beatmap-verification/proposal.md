## Why
Source-bound map verification exists, but its direct native calls still require a
background caller. Project restoration needs bounded nonblocking admission and an
exact connection between the original map job and its genuine completion payload.
Sequential immutable map jobs can share a numeric source request.

## What Changes
Reuse the existing cold worker pool and offline map reservation for asynchronous
verification. Retain real queued/running custody until actual retirement and expose
bounded status. Resolve a genuine completion through its original native job and
private immutable payload, preserving cancellation and source-placement guards.

## Non-goals
No inline project map records, changed record/journal limits, new executor, map
registry, scalar timing effect, selected-map authority, live mapping or Rust port.
Durable compact references, blob publication and Model/Loader/Copy/restore remain
required integration work. Heavy parsing/hashing/retirement stays off realtime.
