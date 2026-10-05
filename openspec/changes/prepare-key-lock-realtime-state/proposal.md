## Why

Rubber Band 4.0.0's LiveShifter reset and first pitch update allocate temporary resampler
measurement buffers. Current starts, retriggers, seeks, stops and mode/stem edges call those
operations from the audio callback. The wrapper's empty output FIFO also inserts a different
delay for different callback partitions. These are concrete prerequisites to resolve before
source pre-roll and synchronized audible launches can be enabled.

## What Changes

- Prepare and warm a fixed pair of unique Rubber Band handles per voice outside the callback.
- Recycle invalidated handles on one engine worker through bounded per-voice SPSC lanes.
- Make callback reset a local invalidation; swap only ready handles without waiting or dropping
  native state. Missing reserve produces bounded silence for shifted audio, while dry playback
  remains reactive. Repeated discontinuities coalesce until a reserve is available.
- Give the fixed-block adapter a deterministic block-size-minus-one frame lead independent of
  callback partitioning. Record native, adapter and device-estimate delays separately.
- Preserve Rubber Band history across the existing source-domain stem crossfade.
- Add a reproducible release probe for cold/warmed preparation and impulse timing.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `time-stretch-pitch-shift`: preparation ownership, bounded reserve fallback, deterministic
  adapter latency and continuous stem processing.

## Impact

Rust backend, preparation worker, per-voice processor, mixer construction and native stream startup;
Rust tests, release measurement example and maintained native/architecture documentation.
No project format, Python API, dependency, control or loop-marker changes.

## Non-goals

This bounded safety gate does not complete audible source pre-roll, source-aligned mode crossfades,
nearest Quantize launch, phase catch-up or hardware alignment acceptance. Silence warming is
allocation preparation, not source-content priming. A dynamically changed warmed handle's nominal
delay does not prove exact transient alignment at its new ratio. Subsequent work must preserve a
separate logical source position and DSP lookahead cursor, prepare exact-ratio/source state, and
reject stale handovers. No engine clock/marker changes, quality reduction, separator replacement,
plugin host, new FX or variable-tempo warp engine are included.

## Realtime constraints

Construction, native reset, cold pitch updates, first shift, scratch allocation and recycling run
outside the callback. The callback owns each active handle exclusively, exchanges unique handles
through preallocated lanes, and never waits, locks, logs, performs disk/GIL work or primes many
native blocks. Queue saturation retains ownership; it never discards native state in realtime.
