## Why

The productive dry mapped path already follows the original accepted map, but
the actual content-bound pause/resume and waveform-hold handlers reject mapped
voices. Removing those guards alone leaves an obsolete absolute output cursor.
The existing playback contract requires silence while paused and continuation
from the saved source position, including the same fractional sample.

## What Changes

This change retains the complete pause/resume objective and is delivered in
coherent steps. Stage A implements the actual waveform-hold path, which already
has durable own-effect acknowledgement, plus its reusable exact source
projection. Stage B integrates ordinary content transport with lossless,
revision-correct feedback. Ordinary mapped transport guards remain closed until
Stage B is implemented and checked; Stage A is not full pause/SYNC acceptance.

- Reuse the original qualified map owner and single TransportTimeline to rebase
  a supported dry mapped voice at the actual resume boundary.
- Share bounded cohort preflight between actual content transport and caused
  waveform-hold actions, preserving their existing content/revision ownership.
- Preserve truthful requested/effective feedback and actual control-path tests.

## Non-goals

No new renderer, scalar mapper, clock, scheduler, automatic map qualification,
live SYNC-policy conversion, mapped seek or Native/pitch/stem transition opening.
Full SYNC and all musical/device/Human/parent gates remain separate.

## Realtime

Only fixed-size copied trajectory preparation and bounded existing map queries
belong at the callback boundary. No Arc owner cloning/final destruction, heap
growth, locks, logging, Python, disk or DSP construction is introduced there.
