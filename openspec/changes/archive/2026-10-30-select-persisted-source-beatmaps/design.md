# Authority, compatibility and lifetime

ProjectState gains `pad_source_beatmap_intent`: 216 independent optional immutable
values. None is untouched legacy revision zero. A nonneutral value has exactly
`schema_version:1`, `revision:u64` and `selected:<existing receipt>|null`. Every
requested selection/clear advances a checked revision, including A→B→A. The
selected object is the existing exact seven-key Native artifact receipt; saved
FileIDs or runtime verified flags are prohibited. It remains outside SampleAnalysis.

Musical snapshots add `source_beatmap_intent`. Neutral None is omitted from sealed
serialization, preserving exact legacy 21-key canonical payloads and hashes. All
existing mandatory fields remain mandatory. A detached in-memory projection may
restore the neutral key for shared field-table loops. Exact old complete project
images may omit only the new table; new images include all 216 rows. Preserve raw
image strings and their existing SHA receipts. Unknown/nested/partial damage
continues to refuse. Native Copy and Portable journal/Bank validators follow the
same old-or-new shapes using the existing receipt validator, not geometry parsing.

The Loader owns pending selection/restore on its existing event drain and Native
offline map job. It captures content instance and intent revision, stores on the
existing worker and resolves only its exact private delivery. Prepare the complete
assignment image while the genuine artifact still seals its backing. Commit the
compact reference with the existing Config writer/revision/fences before publishing
the live intent and assignment table; failure preserves the previous selection.
An unsuccessful newly exported candidate is retired by its exact acquired owner.
The geometry supplies no scalar timing, source-rate, BPM, Key Lock or audio effect.

Fresh cold ACK starts selected-map restore independently of SampleAnalysis and
automatic/manual/TAP BPM gating. Only current original job + content instance +
still-requested intent/revision may become effective. Copy shares immutable backing
but has independent content intent; Move/Swap carries the original content. Same
content reanalysis preserves corrections; real source replacement/unload clears
them even when timing is stale; failed replacement retains the predecessor.

Use AssignmentPublicationPlan and CopyJournalCustody, not a second owner table.
Map leaf pins carry compact historical expected bytes/hash and captured runtime
leaf/directory identities. All still-required unresolved captured/before/after/
ancestor full-project images and Bank histories retain map pins until actual
settlement/compaction. Registered Config inventories include map references.
Material aliases preserve immutable map references without copying/relabeling the
map; new source custody must freshly qualify restore. Migration publishes the map
table and treats map-only references as retained material references.

Native cleanup uses the existing ProjectAssets retiring queue. Reserve its bounded
capacity before a new map birth, then transfer the same slot plus exact directory
and complete/partial leaf birth evidence to that queue on unresolved Drop/failure.
No second queue/registry is introduced. Physical removal waits for all other pins
and actual readers, retries transient sharing, and preserves changed identities,
unknown children and reparse paths. A typed bound map lease can queue explicit
retirement using its captured IDs and receipt evidence; full hashing is on the
existing cleanup worker. Ordinary transient/history map acquisition must not
discard queued proof. A current saved-map acknowledgement may cancel its obsolete
retirement only at actual assignment publication after durable Config commitment.
Map retirement never authorizes generic recursive deletion or removal of originals.

Malformed new metadata is contained to its row, preserves valid sibling settings
and original on-disk bytes, and establishes the existing unsafe-metadata save fence.
Missing old metadata is neutral. Historical receipt presence is never effective
state or Native qualification. No total-memory or final acceptance claim follows
from small receipts or isolated tests.
