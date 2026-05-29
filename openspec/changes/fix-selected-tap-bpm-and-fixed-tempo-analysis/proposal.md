# Change: Fix selected Tap BPM mapping and fixed-tempo analysis

## Why

Tap BPM Learn currently saves pad-specific mappings, so one learned keyboard or MIDI input keeps
tapping the pad that was selected when Learn was performed. For performance use, the learned Tap BPM
input should follow the currently selected pad.

Automatic BPM analysis also reports incorrect values for fixed-tempo material. A synthetic 120 BPM
metronome MP3 can be reported as 119.86 BPM even though its decoded transient grid fits 120.00 BPM.
Real tracks can also contain the correct tempo in the analyzer candidate list while the selected
primary BPM lands on a half-time, double-time, or subdivision candidate, especially when confidence
is low. Since this app assumes one fixed tempo per song, stable candidate families and highly
regular transient grids should refine the published BPM.

## What Changes

- Add a selected-pad Tap BPM mapping action that always taps the current selected pad when executed.
- Keep existing pad-specific Tap BPM mappings valid for backwards compatibility.
- Refine automatic analysis BPM after `stratum_dsp` analysis when low-confidence candidates show
  stronger octave-family consensus than the primary BPM.
- Further refine BPM when strong decoded transients form a stable fixed-tempo grid near the chosen
  candidate-family tempo.
- Round the refined fixed-tempo BPM to 0.01 BPM before publishing analysis metadata.

## Non-Goals

- No tempo-change detection or variable-tempo support.
- No neural inference, plugin loading, disk I/O, Python/GIL access, blocking work, logging, or heavy
  allocation in the Rust audio callback.
- No replacement of `stratum_dsp`; candidate consensus and fixed-tempo refinement are bounded
  non-realtime post-processes in the existing analysis worker.

## Impact

- Affected specs: `pad-manual-bpm`, `audio-analysis`
- Affected code: Python input mapping actions/controller, UI action facade, Rust non-realtime audio
  analysis, focused Python/Rust tests.
