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

Some fixed-tempo MP3s still expose ambiguous subdivision candidates and near-integer tempo drift.
The analysis should prefer the performer's constant tempo when a supported common-ratio candidate
or a strongly dominant common-ratio spectral target survives a full-track spectral autocorrelation
check. It should also preserve near-integer fractional tempos such as 89.99 BPM for timing and grid
math instead of snapping them to 90 BPM for display readability. Separately, beat-grid metadata near
the very start of a file can contain analyzer hop latency (for example 512 or 1536 samples) rather
than a musical offset, causing the Loop Editor's first bar line to start late. Recoverable MP3
packet decode errors should also preserve the decoded timeline whenever the packet duration is
known.

## What Changes

- Add a selected-pad Tap BPM mapping action that always taps the current selected pad when executed.
- Keep existing pad-specific Tap BPM mappings valid for backwards compatibility.
- Refine automatic analysis BPM after `stratum_dsp` analysis when low-confidence candidates show
  stronger octave-family consensus than the primary BPM.
- Further refine BPM when strong decoded transients form a stable fixed-tempo grid near the chosen
  candidate-family tempo.
- Add a spectral autocorrelation post-check that can choose a supported common-ratio performer
  tempo, such as 3/4 of a stronger subdivision candidate, or a strongly dominant 4/5 performer
  tempo.
- Preserve refined fixed-tempo BPMs to millibpm precision for timing/grid metadata and avoid
  internal integer snapping for near-integer tempos.
- Keep pad-control BPM overlays rounded for quick readability while selected/manual BPM displays
  continue to show two decimals.
- Snap analysis beat/downbeat anchors very close to file start to 0.0 seconds before deriving the
  Loop Editor grid anchor and Rust pad timing metadata.
- Preserve MP3 timeline length across recoverable packet decode errors by inserting silence for a
  bad packet when the stream channel layout and packet duration are known.

## Non-Goals

- No tempo-change detection or variable-tempo support.
- No neural inference, plugin loading, disk I/O, Python/GIL access, blocking work, logging, or heavy
  allocation in the Rust audio callback.
- No replacement of `stratum_dsp`; candidate consensus, transient checks, and spectral tempo
  refinement are bounded non-realtime post-processes in the existing analysis worker.

## Impact

- Affected specs: `pad-manual-bpm`, `audio-analysis`, `loop-region`, `waveform-editor`,
  `load-audio-files`, `performance-ui`
- Affected code: Python input mapping actions/controller, UI action facade, timing metadata, Rust
  non-realtime audio analysis/sample loading, focused Python/Rust tests.
