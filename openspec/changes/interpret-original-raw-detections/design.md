## Shared data and one geometry authority

The initial UI client submits at most 64 explicitly entered labels in at most
8 KiB. The strict interpretation intent is:

```json
{"schema_version":1,"raw_revision":"<original capture revision>","anchors":[{"raw_index":0,"quarter_bits":"0000000000000000"},{"raw_index":4,"quarter_bits":"4014000000000000"}]}
```

This is an example of explicitly supplied coordinates, not a default rule for
indices. `quarter_bits` is an exact 16-digit lowercase binary64 encoding of the
entered finite quarter-note coordinate. Python supplies neither source seconds
nor sample positions. Native resolves original ODF positions and uses the existing
accepted validator/evaluator for the sole B/S map. A data-oriented builder from
validated capture plus validated interpretation is shared with future qualified
automatic evidence; public JSON cannot manufacture that future qualification.

At least two distinct, chronological, physically usable Raw events and strictly
increasing finite quarter coordinates are required. Existing numeric bounds apply.
There is no implicit sorting, neighboring label fill or count inference. All
unselected events remain in the complete capture. Position estimates stay
estimated; explicit label provenance does not certify their musical accuracy.
Initial intervals are uncertain, meter unknown and downbeat phase null. Existing
count/coverage/meter/phase and source-position edits remain independent authorities.

## Additive durable envelope

Version 2 uses encoding `source-beatmap-pwa-v2` and the existing source and accepted
sections. Its Raw section contains only `raw_revision` and `capture`, where
`capture` is the complete existing `source-beatmap-unreviewed-qm-raw-v1` envelope.
It does not duplicate derived seconds arrays. The original Capture revision,
canonical Raw-wrapper revision and complete Map revision are distinct identities.
Existing canonical hashing binds all source, complete Raw provenance, accepted
state and lineage. Nested field order is not the standalone Raw file's byte-order
contract; typed validation, exact bits and original revision remain mandatory.
Unknown/unsupported fields and versions refuse. Version 1 keeps its strict parser
and physical Raw limits. Initial interpretation rejects outside observations as
anchors. Later accepted virtual coordinates and explicit whole-map translation
retain the same existing geometry contract as version 1; signed virtual data
never grants physical reads or SYNC eligibility. Editing and undo use a Native-validated schema-neutral
Raw count and retain the actual envelope version plus complete immutable capture.
The existing compact artifact receipt remains a separate unchanged version.

## Admission and independent lifetimes

`AudioEngine.begin_source_beatmap_review_async(sample_id, candidate, review_json,
*, expected_source_generation=...)` accepts only the actual opaque original
candidate. Before admission it verifies engine, original content/placement/source
generation/currentness and capture revision; pending/stale/forged/ABA targets do
not gain authority from digest equality. The controller additionally fences the
exact transient attempt and revision before passing that genuine candidate.

Valid admission clones the actual Raw artifact with its genuine review lease.
The new accepted job captures its own original source/cancellation owner through
the existing cold-lane reservation. The old Raw cancellation right is not copied
or consulted as accepted authority after that transition. The worker freshly
verifies actual original custody. Raw discard after valid accepted admission may
hide Raw views and request their retirement; it cannot revoke the new job or its
completion. Source replacement still invalidates both according to their own
original source owners. Queue/arm/submit/failure/drop/cancel paths retain real
readers and reservations until actual retirement.

The worker embeds and independently parses owned accepted bytes before releasing
its Raw artifact reader. Accepted geometry and delivery retain neither a bare
shared RawData allocation nor the old Raw owner/lease. The Raw slot becomes free
only after its true last reader retires. Existing ArtifactBirth, sealed reopen,
private original completion, receipt, Config-before-Selection and retirement are
reused. The late accepted placement retry delivered in 26c99d4 remains authoritative.

The existing 64-MiB record and array/numeric limits remain. New build work has a
checked simultaneous peak budget covering retained Raw/capture, intent, actual
source readers, intermediate serialization trees/bytes and accepted reparse;
neither a 16-MiB Raw encoding nor input-byte length alone proves that bound.
This is a bounded job admission, not a whole-process or 216-PAD RAM acceptance.

## UI intent

The English Raw inspector adds blank Raw event index and Quarter-note coordinate
fields, Add/update label, Remove label, a bounded explicit label list and Build
source map from labels. Optional Use detection fills only the index and clears
the quarter field. No default quarter or count is inserted. A single draft is
keyed by original global attempt and capture revision so Move/Swap follows that
same content; every action still checks current source/content/Native authority.
Stale gestures, pending placement, missing labels and invalid input cannot mutate
accepted intent or admit work. Once validly admitted, the existing accepted
pending/effective and durable selection path owns the result. Scalar BPM, loop
markers, SYNC and audio stay unchanged until their existing explicit actions.
