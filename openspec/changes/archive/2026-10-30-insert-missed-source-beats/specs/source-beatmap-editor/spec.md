## ADDED Requirements

### Requirement: Captured maps support explicit detector-missed sample anchors
Editor behavior capability4 SHALL accept a strict bounded insert_manual_beat intent
on a selected complete captured map. It MUST bind current map and inner capture
revisions, exact finite quarter bits, physical integer source sample and actual
rate to adjacent interior endpoints or the actual outer endpoint. It SHALL add
exactly one reviewed anchor without a Raw observation association or any change
to prior anchors, complete Raw or exterior intervals. New/split intervals MUST
remain uncertain with unknown meter/phase. Existing MoveBeat unsupported insertion
guards and separate count/coverage qualifications SHALL remain.

#### Scenario: A detector omitted a beat inside an unknown-count interval
- **WHEN** an explicit current insertion lies strictly between both adjacent endpoints in source and quarter coordinates
- **THEN** exactly one sample-positioned manual anchor is prepared through the existing source-qualified immutable edit path
- **AND** its two intervals remain uncertain without count, meter, snap or effective-SYNC promotion

#### Scenario: The performer extends a captured map at either edge
- **WHEN** the new physical sample and quarter coordinate are strictly outside the actual first or last anchor in the chosen direction
- **THEN** only that outer interval is added and all previous geometry/evidence remains exact
- **AND** source frame0 is allowed but the exclusive source end and ambiguous geometry refuse

### Requirement: Manual insertion has exact closed Undo and bounded cold ownership
Insertion SHALL reuse existing refinement inverse/replay and predecessor digest
validation to restore the complete predecessor geometry and metadata. Its source,
map/index, prospective inverse/output and serialization/reparse memory MUST be
admitted before growth under existing checked bounds. Stale, malformed, forged,
source/rate/extent, count, resource, save or cancellation failures MUST preserve
the prior durable selection and audio intent. No new callback work or authority
is permitted.

#### Scenario: An inserted interval had previously verified metadata
- **WHEN** insertion resets its new split intervals and a later exact Undo executes
- **THEN** all predecessor anchors, coverage/meter/phase, Raw and lineage are restored through the existing source-qualified worker and artifact path
- **AND** a forged inverse, parent or replay cannot obtain the same permission

### Requirement: Modular insertion controls capture the complete current gesture
The English UI SHALL bind PAD/source/map/capture, fixed endpoint indices and exact
coordinates, placement mode, sample/rate and exact quarter bits to one edit
gesture. Any relevant selection/revision/endpoint or pending-state change MUST
invalidate stale drafts before admission. It SHALL use existing bounded Native
readback and the existing selection/edit path without full Python map parsing.
Launcher and Portable checks MUST require real editor behavior capability4 while
preserving all earlier readback/correction signatures and refusing bool versions.

#### Scenario: A captured map changes after manual input
- **WHEN** the selected source, map/capture revision, endpoints or busy state changes before apply
- **THEN** the old gesture is refused or cleared and no manual insertion job is admitted
- **AND** reopening shows current bounded endpoints without assuming trusted interpolation

#### Scenario: A selected legacy map has no complete capture
- **WHEN** its existing full editor and variable grid are rendered
- **THEN** no unavailable manual-insertion panel displaces the existing controls or grid
- **AND** bounded Native capture readback retains its explicit unavailable result
