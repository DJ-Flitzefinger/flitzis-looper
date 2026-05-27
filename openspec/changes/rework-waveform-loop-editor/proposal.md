# Change: Rework waveform loop editor behavior

## Why

The waveform loop editor needs to behave more like a performance tool and less like a
destructive marker editor. Newly loaded tracks should be immediately useful as 8-bar loops from
the start of the track, the full-track action should be explicit, and seeking inside the waveform
should not accidentally move loop markers.

Current specs still define 4-bar defaults, a Reset action, and a dedicated Stop button. Those
contracts conflict with the requested loop editor workflow.

## What Changes

- Newly loaded tracks default to auto-loop enabled, 8 bars, and loop start `0.0`.
- The old Reset loop action is replaced with `ALL`, which stores an explicit full-track loop region
  by disabling auto-loop for that pad.
- Loop bar counts support `0.5` and larger finite values, with power-of-two left-click stepping and
  exact `1.0` bar right-click stepping bounded by the remaining loaded track duration.
- Empty waveform left-click sets the selected pad loop start and immediately retriggers the
  selected pad from the new effective loop start without stopping other pads.
- Middle mouse down in the waveform seeks the selected pad's active or paused voice without
  changing loop markers.
- Seeking outside the active loop plays through to the loop or track boundary before normal loop
  wrapping resumes.
- Waveform transport buttons change to mouse-down Play and Pause controls: Play left-click
  retriggers from loop start, Play right-click stops, Pause left-click toggles pause/resume, and
  Pause right-hold pauses only for the hold duration.
- The waveform editor gains maximize/restore behavior and larger toolbar hit targets.
- `Adjust Loop` toggles the already-open editor closed when it targets the same selected pad, and
  switches the editor when it targets a different pad.

## Non-Goals

- No live loop-edit crossfade, zero-crossing policy, or broader loop smoothing.
- No plugin, VST, LV2, CLAP, AU, external DSP, stem-separation, or key-lock behavior changes.
- No broad UI redesign outside the waveform loop editor toolbar/plot and selected-pad sidebar
  `Adjust Loop` action.
- No change to the existing offline stem cache or sample loading architecture.

## Realtime Constraints

This change may add a bounded scalar seek command to the Rust audio command path, but the CPAL
audio callback MUST NOT add disk I/O, JSON reads/writes, Python/GIL access, UI calls, blocking
locks, logging, neural inference, plugin loading/scanning, unbounded loops, heavy allocation, or
long-running work.
