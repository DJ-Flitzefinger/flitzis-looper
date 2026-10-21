## Why

The admitted dry mapped voice follows a real master epoch, but currently rejects
every changed global rate tuple. This prevents continuous SYNC tempo adaptation
and the existing coupled speed/master-period fader. A scalar copied-history
projection cannot supply the missing original-map continuity proof.

## What Changes

- Reuse the global rate coordinator, source/participant capture, settled-drain
  fences and fixed-slot atomic claim for an explicitly supported all-dry cohort.
- Preserve master beat and each original mapped source position at the actual
  boundary; follow the new master slope without an additional speed multiplier.
- Support mapped start/retrigger after a qualified nonunity speed observation.
- Preserve complete refusal for unintegrated Native/history, paused mapped,
  transition and out-of-envelope participants; keep prior audio and timing intact.

## Non-goals

No new clock, scheduler, worker pipeline or Native output alignment. Nonzero
mapped pitch, Key Lock on, mapped pause/seek/catchup and wider mixed Native groups
remain gated. This does not close the original Rate37/pitch, resource, music,
device, Human or FULL Goal final acceptance matrix.

## Realtime

The callback borrows the original immutable maps and computes fixed copied
replacements. Source/map owners and generations remain unchanged. All lookups and
fallible preparation precede the existing atomic claim's effective stores. No
allocation, blocking, I/O, GIL or final large-owner destruction is permitted.
