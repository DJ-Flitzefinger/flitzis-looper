# Remove source SYNC and REALTIME BPM

The Human confirmed that the accepted constant-BPM detector and robust Tap BPM
keep the reference loops aligned for more than 100 bars, without source SYNC.
The additional SYNC path makes those same tracks diverge. Retire that feature
from main rather than retain dormant playback and editor complexity.

Preserve the complete implementation in the remote `SYNC-REALTIME-BPM` branch.
Its accepted baseline is `949cff1dae4b979639dca1cbc1e86c586387cd21`;
`27ef7f878f55c0955835ca536af8df6ef4a89a28` additionally preserves unpublished
regional-probe work, which is not part of the accepted baseline or main delivery.

Remove controls, actions, map editing/preparation/storage, policy adoption and
mapped phase correction. Preserve neural BPM inference, CPU/CUDA receipts,
manual/Tap/half/double BPM, BPM Lock, scalar grids, precise constant loop periods,
quantized starts, pitch, Key Lock and ordinary source/reader custody.

Also draw scalar grid lines on the same loaded-sample lattice as physical loop
markers, without changing the stored origin, BPM or playback period.

## Non-goals

No new detector policy, variable-tempo playback, audio-device operation, private
data cleanup, full Rust port or unrelated optimization.

## Realtime safety

Removal must preserve bounded scheduling, immutable source ownership and deferred
retirement. No file access, inference, JSON, Python/GIL or blocking work enters the
audio callback.
