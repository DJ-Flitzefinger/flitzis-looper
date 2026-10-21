## ADDED Requirements

### Requirement: Beatmap selection belongs independently to content intent
The system MUST store a compact source-beatmap intent independently of scalar
SampleAnalysis. Each nonneutral intent SHALL contain exactly schema_version=1,
checked u64 revision and an optional exact existing Native artifact receipt.
Untouched legacy content SHALL be neutral. Every selection or clear request MUST
advance its revision without wrapping. No full map envelope, saved current/verified
flag, portable FileID authority or fabricated BPM/Key carrier SHALL be used.

Copy MUST share immutable backing with independent content intent; Move/Swap and
Bank operations SHALL use the same musical field authority. Same-content reanalysis
SHALL preserve explicit map correction; real replacement/unload SHALL clear it,
including timing-stale completion. Failed replacement MUST preserve its predecessor.

#### Scenario: A corrected map is copied and its origin is removed
- **WHEN** copied content survives Move/Swap and the original is unloaded
- **THEN** its independent intent still references the same protected immutable map
- **AND** changing or clearing one content intent does not mutate the other

### Requirement: Legacy sealed metadata remains exact and strict
Musical snapshot serialization MUST preserve exact legacy 21-key canonical seals
when map intent is neutral. New nonneutral intent SHALL use the strict additional
field. Complete legacy project images MAY omit only the new 216-row intent table;
all other required fields SHALL remain mandatory. Validation MUST retain raw
project-image text and existing hashes, and reject unknown or malformed nested
metadata. Native Copy, Pair/Bank and Portable validators SHALL agree on these shapes.

#### Scenario: An old unresolved journal is restored
- **WHEN** its original complete image and musical seals contain no map field
- **THEN** validation retains their original bytes and receipt hashes
- **AND** arbitrary omissions, unknown fields and invalid added map metadata still refuse

### Requirement: Source-qualified selection commits before live publication
The existing Loader and Config writer MUST adopt a map only from its exact original
Native job/private delivery, captured content instance and still-requested intent
revision. Backing and a complete prepared assignment image SHALL remain owned
through atomic compact Config commitment and subsequent live publication. A failed
commit MUST keep the old selection and retire only the proven unused candidate.
Dictionary substitution, equal requests, source ABA or changed intent MUST NOT
authorize a stale map. Selection SHALL NOT change scalar timing or playback state.

#### Scenario: A map selection races a newer intent or save failure
- **WHEN** the old worker finishes after intent/source supersession or Config replacement fails
- **THEN** the predecessor remains selected and owned
- **AND** the stale candidate cannot become effective or remove the predecessor

### Requirement: Saved maps restore without analysis or inference
After actual matching cold source ACK the system MUST restore a saved selected map
through the original bounded Native worker and private completion, independently
of automatic/manual/TAP BPM mode and SampleAnalysis presence. It SHALL freshly
verify backing and actual source before effective availability. Material aliases
MAY preserve immutable backing references, but MUST require fresh source qualification.

#### Scenario: A fresh application restores manual content
- **WHEN** Config has a selected receipt and no scalar analysis or automatic timing
- **THEN** actual source ACK starts model-free map restoration
- **AND** only the matching current content/intent/original job exposes its verified geometry

#### Scenario: A supported saved receipt has missing or damaged backing
- **WHEN** a valid Config selects a map whose sidecar cannot currently be opened or verified
- **THEN** startup retains its historical receipt, source and scalar controls and reports map unavailability
- **AND** unrelated PAD operations remain possible while the failed map is never exposed as effective
- **AND** conservative historical pins confer no saved acknowledgement or deletion proof, while a new selection still requires its exact verified candidate

### Requirement: Map cleanup retains exact proof through actual last readers
The system MUST retain exact map cleanup evidence through actual last readers.
Map assignments, unresolved full-image/history/ancestor and Bank references, saved
registered Config references and actual readers MUST protect immutable map backing
through existing asset owners. Native cleanup SHALL retain exact map birth or
retirement evidence in the existing bounded ProjectAssets queue until actual
last-reader retirement or a verified preservation decision. Birth capacity MUST
be reserved before creation; queue-full refusal SHALL preserve old state and owned
proof. Transient/history map acquisition MUST NOT discard queued proof. A saved
map acknowledgement MAY cancel obsolete retirement at actual assignment publication.

Deletion MUST remain limited to the exact typed generation and its known map.json
with captured identities and applicable extent/SHA evidence. Unknown children,
replacement, reparse and originals SHALL remain preserved. No new collector,
executor, job registry or generic recursive deletion authority SHALL be introduced.

#### Scenario: A transient pin survives an unresolved output
- **WHEN** the original worker and output retire while another map owner still exists
- **THEN** exact cleanup proof remains queued without retaining a running worker
- **AND** physical cleanup occurs only after that last owner actually releases

#### Scenario: Recovery history or another Config alone retains a map
- **WHEN** no current PAD refers to it but an unresolved full image, ancestor or registered Config does
- **THEN** that existing custody prevents physical deletion until the reference is actually settled or removed

### Requirement: Damaged map intent cannot silently overwrite its original
Malformed new metadata SHALL recover only its own invalid rows while retaining
valid sibling preferences and establishing the existing unsafe-metadata save fence.
The original Config bytes MUST remain unchanged until explicitly resolved. Missing
legacy map metadata SHALL remain neutral and SHALL NOT fabricate Native acceptance.

#### Scenario: A single selected receipt is unsupported
- **WHEN** a Config contains valid settings and one invalid map intent
- **THEN** only that row is neutralized for safe runtime use
- **AND** the original file is retained and ordinary saving remains fenced
