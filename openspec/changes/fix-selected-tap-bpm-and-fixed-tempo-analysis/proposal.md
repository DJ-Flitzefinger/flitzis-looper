# Change: Fix selected Tap BPM mapping and fixed-tempo analysis

## Why

Tap BPM Learn currently saves pad-specific mappings, so one learned keyboard or MIDI input keeps
tapping the pad that was selected when Learn was performed. For performance use, the learned Tap BPM
input should follow the currently selected pad.

Automatic BPM analysis also reports slightly incorrect values for simple fixed-tempo material. A
synthetic 120 BPM metronome MP3 can be reported as 119.86 BPM even though its decoded transient grid
fits 120.00 BPM. Since this app assumes one fixed tempo per song, highly regular transient grids
should refine the detected BPM to the fitted constant tempo.

## What Changes

- Add a selected-pad Tap BPM mapping action that always taps the current selected pad when executed.
- Keep existing pad-specific Tap BPM mappings valid for backwards compatibility.
- Refine automatic analysis BPM after `stratum_dsp` analysis when strong decoded transients form a
  stable fixed-tempo grid near the detected tempo.
- Round the refined fixed-tempo BPM to 0.01 BPM before publishing analysis metadata.

## Non-Goals

- No tempo-change detection or variable-tempo support.
- No neural inference, plugin loading, disk I/O, Python/GIL access, blocking work, logging, or heavy
  allocation in the Rust audio callback.
- No replacement of `stratum_dsp`; the fixed-tempo refinement is a bounded non-realtime post-process
  in the existing analysis worker.

## Impact

- Affected specs: `pad-manual-bpm`, `audio-analysis`
- Affected code: Python input mapping actions/controller, UI action facade, Rust non-realtime audio
  analysis, focused Python/Rust tests.
