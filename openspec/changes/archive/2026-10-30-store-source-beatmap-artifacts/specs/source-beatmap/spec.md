## ADDED Requirements

### Requirement: Native immutable map storage uses the original source owner
The system MUST provide asynchronous typed immutable source-map storage through
the existing bounded cold workers and original offline map job. It SHALL retain
the actual captured source custody through source verification and publication,
preserve source counters and playback state, and use no new job registry, parser
or inference. Artifact file/hash/parse work and artifact-owned large retirement
MUST remain off realtime and the UI thread. Lane refusal SHALL precede offline admission.

Storage SHALL write the supported complete envelope outside project and journal
images, using exclusive guarded births, sync, sealed reopening and exact identity,
extent, hash and map-revision verification before immutable publication. The
legacy, readable and M-ID layouts SHALL have distinct explicit typed map paths.
Other project artifact kinds SHALL retain their existing meaning and guards.

#### Scenario: A real source-qualified map is exported
- **WHEN** the original map job stores its verified envelope
- **THEN** the private output contains its exact durable file receipt and geometry
- **AND** existing source, scalar timing and playback authority remain unchanged

#### Scenario: An unpublished artifact fails or becomes stale
- **WHEN** publication, delivery or ownership fails before artifact resolution
- **THEN** native cleanup retires only its exact proven births off the UI thread
- **AND** unknown children, replacement identities and reparse targets are preserved
- **AND** another admitted asset owner or reader prevents physical removal under the existing admission gate

### Requirement: A portable receipt requires fresh native source qualification
The system MUST restore a strict compact receipt through the same async lane.
The receipt SHALL contain only version, kind, typed normalized relative path,
bounded byte length, full-file SHA256, map revision and decoder identity. The
reference MUST include the literal `samples/` prefix with normalized forward
slashes; absolute, traversal, backslash and prefix-omitting aliases SHALL refuse. Saved
FileIDs or verified/current flags SHALL NOT supply portable authority.

Restore MUST hold sealed artifact and ancestor readers plus the original asset
pin, verify bounded bytes through EOF and all declared digests/identities, parse
with the existing Rust map parser and verify against the newly captured actual
source. Invalid, foreign, oversized, modified or stale input SHALL refuse without
replacing playback or selecting a map. No full record serialization SHALL run on
the Python caller during restoration or status polling.

#### Scenario: A fresh engine restores an exported artifact
- **WHEN** it has loaded and acknowledged the matching complete source
- **AND** the original worker verifies its saved compact receipt
- **THEN** only that job can resolve the genuine source-qualified artifact
- **AND** no saved runtime authority, model load or mono export is used

#### Scenario: Source or backing changes
- **WHEN** source ownership becomes stale or sealed backing fails its identity, extent or hash checks
- **THEN** restoration refuses and keeps the existing source and playback state

### Requirement: Artifact output custody is distinct from worker retirement
The private completion MUST transfer actual sealed file custody and the existing
ProjectAssets pin to an immutable nonconstructible output, without retaining the
running worker reservation. Only exact original-job private delivery resolution
SHALL acknowledge a newly stored output as durable. Mutable event fields, equal
numeric requests or another genuine job SHALL NOT authorize another artifact.
Natural finished-handle Drop SHALL preserve queued completion. Cancelled, stale
or dropped unresolved births SHALL retire safely; restored files SHALL never be
treated as new births. Resolved outputs SHALL remain durable after handle Drop.
This contract SHALL NOT enable selected-map persistence or automatic collection.

#### Scenario: Output readers outlive the worker
- **WHEN** the worker reaches actual terminal retirement
- **THEN** its lane admission is free while the genuine output still holds its file and asset pin
- **AND** the exported receipt remains durable after the output reader closes

#### Scenario: An unrelated or mutated delivery is supplied
- **WHEN** artifact resolution receives a different job or substituted event fields
- **THEN** it refuses or returns only its own privately retained genuine output
