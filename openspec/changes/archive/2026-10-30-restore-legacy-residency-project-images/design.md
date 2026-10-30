## Existing authority and precise gap

Pair, Copy and Bank journals share `validated_project_image`. This reader checks
bounded finite unambiguous JSON, complete top-level fields, strict ProjectState
validation, canonical nested metadata and unique content identities. Journal
validators separately check the original serialized image digest. ProjectState
already migrates the automatic admission mode but the strict image field check
rejects the input before that migration can run.

Allow only subsets of the two named historical fields. After strict validation,
remove only fields actually absent from the canonical comparison. An explicitly
stored mode must still match strictly. Do not replace the serialized image with
the migrated projection or recalculate its digest. No coordinator gains a new
ACK, writer or retirement authority.

## Verification and limits

Use real sealed Pair, Copy and Bank journal histories and App startup/Close with
512-MiB and 9925-MiB legacy images, both with and without source-map intent.
Reject arbitrary missing/unknown fields, malformed nested values and tampering.
Reinspect the actual reported project read-only and verify config and journals
remain byte-identical. Reuse the unchanged Native Release build; these checks
do not prove device, musical or arbitrary damaged-history recovery acceptance.
