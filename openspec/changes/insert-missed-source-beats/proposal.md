## Why

Captured detector observations can omit a real musical beat. The current selected
map editor can correct an existing uncertain anchor and review captured Raw rows,
but cannot express a new source-sample position in an unsupported interval. Reusing
MoveBeat insertion would inherit coverage/meter and its inverse assumptions.

## What Changes

- Add one explicit captured-map insert_manual_beat intent with fixed neighboring
  endpoints, current map/capture revisions and exact sample/quarter coordinates.
- Preserve every prior anchor and Raw observation; new intervals remain uncertain
  with unknown meter/phase until separately reviewed.
- Reuse the existing immutable edit, source verification, private completion,
  durable-before-selection and closed refinement Undo paths.
- Expose a focused English UI and require editor behavior capability4 throughout
  launch, Portable and test surfaces.

## Non-goals

No detector fabrication, automatic quarter/count/meter qualification, MoveBeat
guard relaxation, suffix relabeling, new Raw lease, audio backend, source pipeline,
master clock, effective SYNC or musical/Human acceptance.

## Realtime

Validation, preparation, inverse replay, persistence and retirement remain on the
existing cold paths. No callback allocation, I/O, inference, GIL or blocking work.
