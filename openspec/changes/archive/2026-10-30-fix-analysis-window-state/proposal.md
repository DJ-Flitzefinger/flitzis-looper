## Why

Rounded downsampling can leave octave CQT responses wider than the trimmed output;
scaling those discarded columns aliases the next retained row. The tempo tracker
also reuses a nonzero resonator accumulator, contaminating each new window's
observation before the legitimate temporal decoder runs.

## What Changes

- Scale each retained CQT cell exactly once using its octave normalization.
- Reset local RCF accumulation for each observation while preserving Viterbi
  smoothing and the existing detector configuration, source timebase and bounds.

## Non-goals

No model replacement, retuning, tempo snapping, changed musical thresholds,
automatic reanalysis of stored Human results or realtime work. All transforms
remain in their existing offline analysis workers.
