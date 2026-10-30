## Why

An interrupted Pair recovery can acquire its durable writer and then fail while
reserving retirement or restoring intent. Later polls currently have no initialized
phase to advance, leaving a recoverable transient failure permanently pending.

## What Changes

Retain a bounded initialization phase that retries with the same selected history,
writer and successfully reserved owners. Complete initialization before ordinary
recovery handoff, and retain existing current-intent and source replacement fences.

## Non-goals

No new transaction recapture, overwritten current intent, weakened journal or
source acceptance, automatic device work or realtime allocation/I/O. Recovery is
the existing off-realtime startup control path.
