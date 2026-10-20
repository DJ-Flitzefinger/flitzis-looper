## Why

Ordinary PAD preparation may finish after its planned execution E. The scheduler
currently defers the event to a later callback while its Work and Native request
still require exactly E, producing prepared_mismatch for an otherwise intact
original attack. The ordinary worker also requires a coherent global placement
snapshot while consuming immutable source owners; an unrelated brief scalar
publication can consequently reject preparation as placement_pending.

## What Changes

- Preserve each original content, source, sequence, timestamp and target T through
  bounded worker delay, with independently validated off-realtime preparation at
  a new own execution endpoint before any actual effect.
- Separate immutable worker validation from control-slot and callback-placement
  publication authority, retaining all source, lifetime, timing, window, launch,
  action and final effect guards.
- Prove sequential ordinary attacks with genuine worker gates and independent
  original-T Native/FIFO/EQ suffix references under both launch policies.

## Non-goals

No callback Native construction, reset, catch-up, allocation or blocking. No new
source recapture, timestamp shift, inferred changing-rate history, duplicate
trigger, second scheduler, hidden unlimited retries or device/listening acceptance.
Finite PCM storage, source readers and ordinary product acceptance gates remain.
