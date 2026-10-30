# Correct productive fractional musical loops

## Why

G3c1 measured actual rendered recurrence at integer physical length H instead of
accepted musical length P. The 48-kHz P=1500.25/H=1500 fixture accumulates 250
loaded frames of error after 1000 cycles. Passing physical PCM tests is insufficient.

## What Changes

- Admit P from the voice's effective accepted period and existing compatible
  logical-cycle rule while retaining exact persisted integer endpoints.
- Share one fractional loop domain across live progression, full/stem reads and
  copied native preparation. Interpolate the final admitted PCM knot to source
  start at P without an extra H/P speed factor.
- Prove actual unwrapped recurrence and independently expected PCM/features at
  75/1000 cycles, including domain ownership and native continuation.

## Non-goals and realtime constraints

No analyzer/default change, variable-map SYNC, UI resets, plugin host, cache
ownership redesign, audible DSP compensation, device/listening certification or
full application Rust-port planning. Callback changes are fixed-size scalar
arithmetic over admitted PCM, with no allocation, locks, I/O, logging or GIL.
Heavy preparation, hashing, exports and native retirement remain off realtime.
