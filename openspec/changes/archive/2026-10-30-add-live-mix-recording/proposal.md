# Add bounded live mix recording

## Why

The Human wants to record a prepared remix, either independently of playback
with a right click or together with the existing active/remembered START/STOP
group from its effective loop starts with a left click. No productive recorder
exists at the published baseline; the existing diagnostic loop-capture tooling
does not implement this feature.

## What changes

- E11-11: RECORD left of global START/STOP with a deliberate gap, reusing suitable
  button/gesture helpers and the authoritative global transport group.
- E11-12/13: single-edge record-only right toggle and atomic capture/group left
  launch with truthful native acknowledgement and precise output-frame bounds.
- E11-14: persisted WAV/FLAC/MP3 and explicit per-format quality defaults, verified
  real encoded files and visible readiness/encoding errors.
- E11-15: fixed preallocated capture storage, non-realtime spool/writer/encoding,
  repo-root `record/`, `/record/` Git ignore, drain/finalize/recovery lifecycle.

Delivery is split into `X11-RECORD-CAPTURE`, `X11-RECORD-FORMAT` and
`X11-RECORD-CONTROL`; only the last integrates the complete performer feature.
This is PLAN_ONLY. All implementation, resource, codec and Human gates remain
open. The central program defines additive dependencies and later R2/B7/V0
integration; existing historical tasks are not reopened or relabelled accepted.

## Non-goals and realtime boundary

No audio-input/device recorder, track export, overdub, per-pad multitrack,
automatic stem generation, normalization, plugin host, extra voice capacity or
new transport clock. Normal START/STOP remains independently usable.
The callback performs bounded copies and atomic/fixed-message updates only:
no disk/JSON I/O, codec work, GIL, locks, logs, heap allocation or unbounded work.
Full Rust-port planning/implementation and Slice8 remain excluded.
