## Why

The retained RubberBand canonical pitch prerequisites fail the fixed attack
criteria. A different algorithm needs a small falsifiable test before any live
SYNC integration. The current SourcePlayback inverse and acoustic criteria stay
authoritative; the new experiment replaces only the private pitch consumer.

## What Changes

- Pin actual Signalsmith Stretch and Linear sources, licenses and file hashes.
- Add an opt-in test feature with a small exclusive opaque C ABI owner.
- Measure one default preset at q1/p2 against the existing four canonical kick
  cases and its own q1/p1 control, retaining the entire declared output capture.

## Non-goals

No productive backend, public API, source trajectory, scheduler, clock, callback,
admission guard, map qualification, dynamic pitch or broad matrix changes. No
musical, device, hearing or whole-SYNC acceptance is inferred.

## Realtime

Construction, processing, diagnostics and destruction remain off realtime in an
explicit test feature. This experiment provides no callback allocation/deadline
qualification. Ordinary Portable and Release builds must not compile or link
the experimental C++ module.
