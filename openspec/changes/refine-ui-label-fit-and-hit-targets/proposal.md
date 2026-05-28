## Why

Long loaded filenames currently clip in the pad grid and selected-pad sidebar, making
sample identity hard to read during performance. A few bottom-bar and Pitch/Master
controls also need slightly larger or better-aligned hit targets for faster use.

## What Changes

- Wrap loaded pad filename labels within the pad surface with a small horizontal inset.
- Wrap the selected-pad sidebar `Filename` value instead of right-aligning it into clipping.
- Align all stem mask/preset buttons in the bottom bar to the same vertical row.
- Increase the Master Volume slider width and the vertical Pitch fader grab size.

## Non-Goals

- No changes to audio loading, sample naming, stem behavior, input mapping semantics, or
  persisted project data.
- No redesign of the pad grid, sidebars, bottom bar, or control color palette.
- No new DSP, scheduling, or realtime audio behavior.

## Realtime Constraints

This is a Python/Dear ImGui rendering-only change. It MUST NOT add disk I/O, Python/GIL
access, logging, blocking work, heavy allocation, neural inference, or any new work to the
Rust audio callback.
