## Observed failure

A deterministic two-pad, real cold-source/four-stem regression adopts a new finite window
after an eight-bar loop, deliberately leaves the control cache at its predecessor, then
publishes grid/BPM/grid edits. Before this repair, repeated reconciliation discards the
original physical receipt because its timing authority revision is old. The source
generation is still current, but the native window and cached window disagree permanently.
The ordinary pad capture consequently reports `current complete source/timing unavailable`.
Signed grid origins are valid; resetting the offset does not repair this lost mirror.

## Separation of authority

`reconcile_claimed_resident_storage` mirrors only an observed storage transfer. It retains
the original pending publication and adopted binding, content key/lifetime/cell, original
cache predecessor or exact already mirrored window, actual source generation/address/shape,
and current native window revision/address/extent/context. The same predicate runs before
and inside the fixed publication claim. A timing revision cannot reverse already adopted
storage. It still invalidates old launch/timing effects through their existing guards.

No source, loop, BPM, pitch, playback state or accepted timing is republished by this mirror.
Matching stem readers move only for the same complete-set identity. Replaced-source and
replaced-window receipts remain ineligible. All cache changes and destructors remain on
the control thread; the callback and its bounded work are unchanged.

## GUI admission failures

Pad and loop-editor actions share a small GUI boundary using the existing per-pad
readiness error. It clears an earlier gesture error before executing a new explicit gesture
so errors created inside admission remain visible. RuntimeError/ValueError failures return
to rendering without a new queue, timer or timestamp. Dependent loop-edit playback is
skipped. Playhead and pause projections change only after successful synchronous admission.
The controller restores start/end markers on synchronous loop-start publication failure;
asynchronous resident work retains its existing rollback owner and acknowledgement path.

## Validation limits

The deterministic reproduction establishes the physical mirror defect and the subsequent
capture failure. The Human's missing original first-crash log cannot establish that every
initial failure was this exact interleaving. Tests use generated audio and offline callback
harnesses. They do not claim CPAL/device, listening, click-free musical acceptance or closure
of the separate unfinished Key Lock start-mode work.
