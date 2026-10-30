## Existing authority and actual gap
The delivered OfflineAnalysisJob map purpose retains exact complete/finite cold
source custody. NativeAnalysisOwner already distinguishes map admissions through
their original cancellation Arc. LoaderEventDelivery already carries the original
owner across acknowledgement/mirror and observer fences. Reuse those owners.

Add asynchronous engine admission using its existing cold_jobs two-worker,
32-queued lane. Reserve that lane before accepting the offline map job. Retain a
queued/running guard under the existing job transitions; handle Drop or cancellation
cannot retire the real snapshot or shared offline reservation while a queued or
running closure still owns work. Failed submission, stopped/shutdown queues,
worker failure and genuine completion must each retire exactly once. Source-request
counters, logical incarnations and prepared epochs remain unchanged by map work.

The existing lane reservation needs a narrowly shared lifetime: its original
handle and one map retirement guard share the same admission owner. Only final
owner destruction decrements the existing admitted/active counts. A separate
non-owning receipt becomes true after those decrements, allowing truthful status
without keeping the reservation alive. This introduces no second reservation or
counter. Running and panic cleanup stays synchronous on the same worker before
its body returns. Only a never-started discarded/submission-failed closure may
retire on the existing off-thread retirement pattern, retaining a queued admission
with running=false. A worker must not start another job while its previous running
map guard survives; otherwise active accounting could exceed the two-worker bound.

Expose concise asynchronous stages and error detail on the original job handle.
Cancellation requested is distinct from actual terminal retirement. No inference,
mono export, fake Beat/Key envelope or additional scheduler is introduced. Heavy
JSON/hashing runs on that existing lane, never ordinary polling or audio callback.

Retain an optional private immutable SourceBeatmap in LoaderEventDelivery only for
an actual SourceBeatmapCompleted payload. The original map job resolves it only
when the owners match exactly and remain current, including placement readiness.
Return that private map; never accept a map substituted in a mutable Python event
dictionary. Same numeric request, revision or source digest is insufficient.

## Integration boundary
This is the required worker/delivery bridge for the later durable vertical. Full
maps cannot be embedded in project images: their supported 64 MiB exceeds the
existing 8 MiB image/16 MiB journal boundaries, and 216 inline copies can repeat
13.5 GiB before wrappers. Preserve those contracts. Durable integration needs
compact per-content references plus typed immutable backing and journal/reader
retention, with exact legacy-shape handling. It remains open here.
