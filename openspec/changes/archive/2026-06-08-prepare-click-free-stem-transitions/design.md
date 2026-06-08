# Design: Click-free stem transition preparation

## Transition State

The mixer owns a fixed-size per-pad transition slot. A slot stores the previous render source
selection and a short linear ramp cursor. The current render source selection remains the existing
per-pad stem mode, source-version hash, and enabled component mask.

The transition does not own audio buffers, file paths, Python objects, vectors, plugin handles, or
dynamic processing chains. It stores only bounded scalar state.

## Render Path

When an accepted stem mode or mask change reaches Rust for an active pad, the mixer captures the
previous source selection and then applies the new source selection. During rendering, the existing
source-frame reader reads both selections at the same source frame and linearly crossfades from
old to new over the fixed ramp.

The crossfade happens before the existing Key Lock/stretch, pad gain/EQ, metering, and playhead
telemetry path. Source-frame position, loop wrapping, BPM-lock timing, and scheduler output time
are not changed by the transition.

## Scope Boundaries

Stem transitions are bounded source-selection changes. They are handled before broader DSP/FX work
because they do not require a new node graph, parameter model, or per-stem processing chain.

Live loop edits are intentionally left unchanged in this step. Moving loop boundaries can require
different policy decisions than selecting an aligned full-mix/stem source.

## Realtime Constraints

The audio callback must still avoid disk I/O, JSON reads/writes, Python/GIL access, UI calls,
blocking locks or waits, logging, neural inference, plugin loading/scanning, unbounded loops,
heavy allocation, and long-running work.
