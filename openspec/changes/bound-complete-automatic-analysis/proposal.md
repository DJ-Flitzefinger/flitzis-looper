# Bound complete automatic analysis

## Why
Automatic PAD import rejects a 376-second stereo MP3 before BPM inference because
the analysis reservation includes avoidable full-track copies. The existing CQT
also materializes full STFT real, imaginary and magnitude matrices that are absent
from that reservation. Raising the limit would retain both defects.

## What Changes
- Prepare the unchanged complete mono signal from borrowed immutable playback PCM
  and share one binary64 input between the concurrent BPM and key pipelines.
- Execute the existing CQT response one FFT frame at a time, preserving its window,
  padding, resampling and accumulation order.
- Admit all simultaneously live preparation, spectral and downbeat workspaces with
  checked arithmetic under the unchanged 512 MiB analysis limit.
- Prove numerical parity, genuine oversize rejection and hardware-free real import.

## Non-goals
No analyzer/model cutover, duration truncation, larger limit, realtime DSP change,
installed Human extension replacement, device/listening acceptance or closure of
the broader musical/resource/Pre-Rust program.

## Realtime and GIL
All preparation and inference remain on existing non-realtime workers. The callback
retains its existing source adoption and acknowledgement path, without allocations,
I/O, locks or Python calls introduced by this change.
