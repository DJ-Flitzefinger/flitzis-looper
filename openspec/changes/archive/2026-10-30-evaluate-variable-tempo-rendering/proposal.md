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

Status: the initial affine mechanism and the bounded canonical-map comparison are
implemented as isolated test diagnostics. The wider experiment matrix, measured
production recommendation and live adoption remain open. See
[historical accepted-map-render-comparison.md](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/27ef7f878f55c0955835ca536af8df6ef4a89a28/docs/accepted-map-render-comparison.md) for actual evidence and limits,
and [historical beatmap-sync-research.md](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/27ef7f878f55c0955835ca536af8df6ef4a89a28/docs/beatmap-sync-research.md) for the original experiment scope.

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
