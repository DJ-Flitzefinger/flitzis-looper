# Design

The placement API distinguishes Current, Pending, Stale and Retired. Cold preparation must use that distinction rather than converting every unsuccessful boolean observation to permanent cancellation. Shutdown, explicit cancellation, the original request, source epoch, residency guard and proposal remain independently authoritative.

Any pending final publication retains the same prepared sample and original proposal, releases control locks between bounded retries, and must reacquire only that original transaction's existing claim. It must never capture a successor source or advance request/epoch authority before the claim succeeds. All waiting and file work remain on the existing bounded cold workers, outside the callback.

Deterministic tests hold a real unrelated placement writer during the productive worker's cancellation check, then release it and require the original source command and real virtual callback acknowledgement. Negative cases preserve actual replacement, explicit cancellation, request/epoch supersession and shutdown rejection.
