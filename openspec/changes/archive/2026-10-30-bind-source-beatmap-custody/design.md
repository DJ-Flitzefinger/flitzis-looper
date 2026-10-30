## Existing authority
Reuse OfflineJobs and JobState purpose, busy reservation, NativeAnalysisOwner,
transitions, cancellation and actual off-thread retirement. Ordinary analysis
retains its original request advancement; map-purpose work snapshots the existing
source request and cell epoch without advancing audio request state.
Map-purpose admission does not allocate/export mono PCM or apply the unrelated
analysis export-size ceiling. Analysis-only APIs reject map purpose explicitly;
unknown staging is not reported as zero. Aborting an unstarted map job retires
without fabricating a Beat/Key diagnostic result.

CompleteSourceReader retains the exact available cold assignment for both full
and finite residency. Map preparation requires it, verifies actual reference and
held original, and matches the existing parser's complete decoder identity.
Full-resident ordinary PCM reads keep their existing in-memory route.

The worker retains the immutable map only after supported bounded parsing and
source verification. Finish rechecks the original content/request under the same
publication boundary and queues a dedicated NativeOwned completion. A Pair/Move
follows the original cell across actual ACK and control mirror; stale/cancelled or
equal-byte replacement work cannot target a new occupant. The dispatcher exposes
the frozen native map object without serializing its potentially64MiB JSON during
ordinary polling. The DTO supplies no current audio or durable-selection authority.

The original map job shares its cancellation token with its NativeAnalysisOwner.
Explicit cancellation also revokes an already queued completion or a pending
placement mirror. Normal Drop after successful finish preserves the queued data;
only explicit cancel, superseding request or source lifetime can revoke it.
Map begin/prepare/finish/cancel/drop preserve the source request counter, logical
source-request incarnation and preparation epoch. Its own cancellation token is
the sole map cancellation mutation; external source/analysis supersession still
invalidates the captured source owner. Sequential immutable map DTOs may share a
numeric source request, because they supply data rather than selected-map intent.
The later durable selection vertical must match its own requested map revision.

## Verification limits
Real cold complete and finite fixtures exercise wrong decoder, cancellation,
request supersession, equal-byte ABA, blocked work and actual ACK/mirror routing.
Existing scalar rendering and diagnostic analysis remain unchanged. Source custody
is not musical acceptance, source-map project integration, an inferred live timing
contract or a whole B3/resource/Human gate.
