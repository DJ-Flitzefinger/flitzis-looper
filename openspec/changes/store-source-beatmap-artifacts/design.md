# Existing owners and publication

Extend the original async map input and retirement guard on cold_jobs (2 workers,
32 queued) and OfflineJobs. Reserve before source admission; keep original job,
source request and exact NativeAnalysisOwner admission semantics unchanged.

The new typed namespace is `<actual material root>/.pcm-cache/source-beatmaps/v1/
.ready-<32 lowercase hex>/map.json`. Legacy flat sources explicitly use
`samples/.pcm-cache/source-beatmaps/v1/...` with no invented material binding.
Readable and M-ID sources use their actual bound material root. Map artifacts
must never be classified as originals, PCM or stem artifacts.

The compact receipt is a strict object with exactly schema_version=1,
kind=source-beatmap-v1, normalized project-relative forward-slash reference
beginning with the literal `samples/` (the prefix is mandatory),
bytes (1..64 MiB), sha256, map_revision and decoder_identity. Digests are 64
lowercase hex characters. FileID is captured afresh at runtime, never portable
authority in a receipt. A receipt is historical data, not source verification.

Generation names use the existing SHA256 dependency over PID, clock and a
checked process counter. Their spelling is not an authority. Exclusive directory
creation and refusal to reuse an existing ready target are the collision guards;
no UUID dependency or installer/bootstrap change is introduced.

Store first verifies the map against captured complete source custody once, then
uses exclusive guarded births, sync, sealed reopen/hash/parser verification and
same-directory ready publication with unchanged directory/file identity. Keep
the original verified source reader through publication and bounded currentness
checks. Restore opens a typed artifact under ancestor guards, bounds through EOF,
checks the full file hash, map revision and decoder identity, and freshly verifies
the current engine's actual source. No complete record crosses Python on restore.

Temporary source/input custody and lane reservations retire on their original
worker. Successful output transfers sealed file/ancestor guards and a verified
ProjectAssets pin into the private completion; it does not retain a running lane
reservation. Only exact original-job artifact resolution acknowledges the stored
artifact as a durable exported output. Unacknowledged births are cleaned off UI
after cancellation, stale delivery, failure or last Drop; removal is limited to
exact known births/identities and refuses unknown children, replacement/reparse.
Birth cleanup serializes with the existing ProjectAssets admission gate and
excludes only its own output pin; any other intersecting owner/reader preserves
the file. This step does not add a later collection/retry policy for such adopted
or conservatively preserved files.
A resolved receipt deliberately leaves a durable file after output handles close;
this is artifact storage, not a selected/current map or a collection policy.
Restored artifacts are never birth-owned. No automatic map collection is enabled.
Later compact Config publication must add assignment/history pins and exact
verified retirement before automatic collection is allowed.

The existing geometry resolver remains available. Artifact completion is private,
nonconstructible and resolved using the same admission/current/placement fences;
mutable event dictionaries cannot authorize a substituted receipt or map.

Artifact-owned cleanup is retired off UI. Existing caller-held SourceBeatmap
geometry clones retain their existing Arc lifetime; this prerequisite does not
claim that every geometry clone's final deallocation runs on a worker.
