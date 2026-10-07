# Persist source-verified accepted constant timing

## Why

Saved projects currently retain only legacy BPM/grid metadata. An accepted native
record needs complete durable evidence and a new source-bound adoption after
restart; a historical ticket or matching BPM cannot establish current timing.

## What changes

Persist the supported COMPLETE native QM accepted record in SampleAnalysis with
its full canonical identity, exact binary64 period, signed independent origin and
separate acceptance policy/provenance. Save verifies actual project source bytes
and full owned channel-mean PCM/rate/extent/source zero. Restore captures fresh
native source/request/intent ownership, verifies evidence off-thread, and uses
the existing adoption guard and callback acknowledgement. Explicit Manual, Tap
and Legacy intent remains durable and takes priority over historical acceptance.

## Non-goals and realtime constraints

No automatic musical acceptance/default cutover, global START/STOP integration,
general accepted publication/derived refresh orchestration, physical-loop proof,
audible compensation or immutable copy-first/ABA guarantee. Unsupported evidence
schemas cannot be promoted. JSON, source hashing, evidence reconstruction and
heavy PCM work remain outside the callback; its existing bounded adoption and
off-thread retirement mechanisms stay authoritative.
