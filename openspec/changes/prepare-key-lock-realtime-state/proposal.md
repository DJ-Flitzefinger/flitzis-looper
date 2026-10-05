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
- Generate dry varispeed and native input from one reusable fractional source timeline,
  preserving source carry and sample sequences across callback partitions in every playback mode.
- Advance per-voice tempo smoothing on fixed active-output-frame intervals instead of callback
  counts, and split bounded rendering at those rate changes.
- Add a reproducible release probe for cold/warmed preparation and impulse timing.
- Verify an exact-ratio source-preparation fixture against an independently resampled contiguous
  native reference, keeping logical and future feed cursors separate. Record transient clipping
  and uncropped residuals before selecting an audible discard rule. The fixture is test-only.
- Extend that proof with an explicit forward source-history origin, preserving logical phase
  separately from raw discard and feed. Measure impulse/tone/percussion timing and retention
  against predeclared stereo-energy criteria before choosing compensation; report failure too.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `time-stretch-pitch-shift`: preparation ownership, bounded reserve fallback, deterministic
  adapter latency, continuous stem processing and partition-independent fractional source reads.

## Impact

Rust backend, preparation worker, source reader/timeline, per-voice processor, mixer construction
and native stream startup;
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
native blocks. Fractional source interpolation uses two bounded reads per channel through the
accepted integer source policy. Rate-interval splitting reuses fixed processor storage.
Queue saturation retains ownership; it never discards native state in realtime.
