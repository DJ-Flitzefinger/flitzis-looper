## Why

The Human requests independently switchable global Realtime BPM and SYNC controls.
Some material should retain its initially determined fixed BPM; other material
should use a qualified variable source beat map. The Human explicitly chose to
keep the established master tempo as the common target.

Current per-content SYNC has no global control and requires a mapped source.
The engine already has scalar source-period/master-period matching and qualified
mapped trajectories, but no single explicit policy selecting these behaviors.

## What changes

- Persist independent global tempo-tracking and synchronization choices, with
  checked revisioning and compatible old-project behavior.
- Resolve those choices with existing independent PAD preferences through one
  policy path shared by GUI, Learn/MIDI, loop/grid selection and Native capture.
- Select fixed matching, qualified variable matching or no matching explicitly;
  preserve the existing master, source/map owners and unrelated controls.
- Report actual callback-owned policy adoption independently of requested state.

## Non-goals

This change does not complete automatic musical map qualification, Beat This B2
default cutover, mapped Native rendering quality, both onset policies or Human
device/listening acceptance. It does not open unsupported renderer guards. Active
voices retain their original owners until a supported real transition or restart;
pending intent is not evidence of live adoption. No second clock, scheduler,
plugin host, callback inference or full Rust port is introduced.

## Realtime and delivery

Policy resolution and preparation remain on existing control/cold paths. Callback
changes use bounded existing publication/capture/feedback and retirement paths,
without Python/GIL, I/O, blocking locks or heavy allocation. Complete meaningful
affected regressions, strict specs, a necessary Release/import/preflight and one
independent final review, then a coherent Conventional Commit and normal existing
main push. Verify clean HEAD/tracking/fresh remote equality before safe own cleanup
and the next independent step; no individual commit/push approval is required.
