# Recover scalar BPM from missing and extra observations

## Why

The accepted scalar estimator uses the detector event index as its beat number.
Missing or extra detections can therefore invalidate otherwise consistent long
recordings. Raw observations should remain unchanged while a bounded fallback
tests explicit beat-number assignments and reports their actual limitations.

## What changes

Keep every successful v4 estimate unchanged. Only after unavailable v4 analysis,
fit explicit inferred ordinals, compare distributed support and competing models,
and serialize the evidence through the existing App tempo DTO. Show understandable
automatic-analysis uncertainty and existing component errors near the BPM controls.

## Non-goals

No live source SYNC, playback maps, phase following, second clock, model/worker
replacement, source cropping, memory-limit increase, compatibility/migration,
restore-time inference, new certification framework or private-data cleanup.
