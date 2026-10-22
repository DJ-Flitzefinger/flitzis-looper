## Why

The retained realtime R3 Standard/HighConsistency candidate fails all four
canonical kick cases. Its disjoint timing and empty retention intervals cannot
be repaired by a constant delay. The pinned existing library offers a genuinely
different fixed-pitch signal order: HighSpeed at p=2 resamples before stretching,
whereas HighConsistency and HighQuality at that pitch resample afterwards.

## What Changes

- Measure that distinct HighSpeed path on the exact existing canonical inputs.
- Reuse the test-only Native owner, stager, drain and unchanged acoustic scorers.
- Add its own unity control and preserve old HC full-raw hashes and measurements.

## Non-goals

No production backend/profile/admission change, dynamic pitch, expanded matrix,
new source/clock/pipeline, callback/deadline/RAM proof or musical/Human acceptance.

## Realtime

All work remains bounded cfg(test) and off realtime. A realtime library API name
does not qualify this application's audio callback or public Native adoption.
