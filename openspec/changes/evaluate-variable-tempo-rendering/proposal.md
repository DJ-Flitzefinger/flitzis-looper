## Why

The live renderer combines variable source reads with Rubber Band LiveShifter pitch correction.
It does not implement an offline keyframe map or prove variable-rate audible alignment. A
repeatable offline experiment must compare render paths before a SYNC control can promise it.

## What Changes

- Specify an isolated diagnostic comparing current varispeed/LiveShifter with RubberBandStretcher
  offline keyframe warping using identical accepted maps and audio.
- Measure rate trajectory, audible markers, native content retention, loop seams and stem behavior.
- Preserve previous causal failures and report preparation/resource costs separately.
- Include independent semitone trajectories now so a later per-pad KEY control cannot require
  redesigning source timing, map identity or master synchronization.

Status: experiment proposal only, not implemented or selected for production. Depends on accepted
map semantics or exact synthetic maps. See `../../../docs/beatmap-sync-research.md`.

## Non-goals

No live adoption, new user-facing KEY control, model installation, fitted launch bridge, reduced audio
quality, changed original thresholds, plugin host or replacement of Rubber Band.

## Realtime Constraints

All experimental construction, analysis, rendering and exports run outside the callback. Results
do not establish realtime safety or deadlines. A live design needs a separate allocation/state
ownership audit; diagnostic execution must not modify active playback.

## Impact

Future test/example adapter and deterministic fixtures; production wrapper remains unchanged.
The diagnostic must run without an audio device and keep private audio/results outside Git.
