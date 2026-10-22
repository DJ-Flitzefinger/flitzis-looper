## Why

Complete detector captures and explicit labels already create source-qualified
maps whose coverage correctly remains uncertain. The current viewport hides all
accepted anchors when the loop reference is unsupported, and sample correction
requires supported interpolation even for an exact existing anchor. This forces
the performer to assert coverage before inspecting and correcting its positions.

## What Changes

- Return bounded existing anchor positions independently of loop-reference support.
- Draw those positions as neutral inspection marks, separately from musical grid lines.
- Let the performer select either inspected span endpoint for sample correction.
- Allow exact existing-anchor sample positioning without promoting coverage, count,
  meter or phase; retain the refusal for inserting an unsupported interior beat.
- Reuse the existing immutable edit, Undo, original source, artifact and selection owners.

## Non-goals

No automatic pulse-to-quarter inference, musical qualification, full-track proposal
builder, detector default cutover or new playback admission. No scalar fallback,
new grid interpolation, source-marker movement, master change or hearing acceptance.

## Realtime

Inspection and correction remain on the existing control/worker paths. The callback
gains no I/O, Python/GIL, allocation, blocking work or new ownership transition.
