## Why

The manual Source Map editor can position a beat and translate a map, but cannot repair beat counts, meter, downbeat interpretation or gaps. Its viewport also cannot select an unsupported interval, and stored downbeat labels currently have no visible meaning.

## What Changes

- Add a bounded Native span inspector usable independently of supported viewport coverage.
- Provide separate count, meter, downbeat and coverage actions through the existing immutable edit job and selection owner.
- Define downbeat phase and expose it in Native bar grids and snapping, with truthful unknown phase.
- Extend closed sequential Undo, source-qualified persistence/Copy/restore, and Native compatibility checks.
- Keep detailed edit failures in the existing console diagnostic sink.

## Non-goals

No automatic detector/refinement, live variable-map adoption, new worker/scheduler, general time-signature system, audio callback change or musical/device acceptance.

## Impact

Reuse SourceBeatmap geometry, cold editor jobs, private completion, Config-before-owner publication and original retirement. Editing remains separate from physical loop markers, scalar audio, master time, pitch and other PADs. Every coherent checked delivery retains the authorized commit and normal main push workflow.
