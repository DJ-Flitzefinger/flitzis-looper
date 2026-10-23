## Why

Global SYNC and Realtime BPM requests currently reach complete runtime rows, but
living voices keep the policy captured at their last actual start. Enabling a
qualified map or disabling mapped playback therefore remains pending until a
restart. Scalar-to-mapped conversion also cannot recover an arbitrary separate
integer/fraction source cursor through a binary64 B/S round trip.

## What Changes

- First compact the already-used immutable master projection without changing
  its legacy arithmetic, authority or epoch comparisons.
- First allow independently qualified scalar-to-scalar live policy adoption using
  its existing projection/reader and glide; every unsupported cohort refuses.
- Specify phase-preserving conversion before opening any representation conversion
  or general mapped live admission.
- Reuse runtime publication and the existing 32-lane capture/preflight/claim path
  for idempotent, retryable dry policy and map adoption at actual execution E.
- Separate effective immutable map custody from original launch ownership and
  retain both through their actual last-reader/off-realtime retirement paths.
- Continue truthful requested/pending/effective feedback, including refusal causes.

## Non-goals

The later Human hierarchy and shared-grid/unequal-loop/one-shot requirements remain binding in the full Goal. This conversion block's exclusions below do not exclude their separate implementation; existing fixed ratio or mapped creative-offset mode acknowledgement is not complete grid-phase acceptance. Performance acceptance now uses realistic2/4/6 and stress20, with10 when performance-related stress failure warrants it; historical32 timing remains diagnostic while internal32 capacity/correctness bounds remain.

This change does not qualify automatic musical analysis, select a new stretcher,
open Native/pitch/Key-Lock transitions, change master tempo, alter physical loop
markers, implement mapped Seek/EOF, or close music/device/Human/resource gates.
Unsupported transitions remain explicit pending/refusal until their own proof.
The complete pre-Rust Goal and STOP before full Rust-port work/Slice8 remain.

## Realtime Constraints

Map validation, geometry construction, owning allocation and final retirement run
off realtime. The callback performs bounded actual-E and freshness preflight under
the original atomic claim before any mutation. No I/O, GIL, locks, inference,
logging, unbounded work or final owner destruction enters the audio callback.
