## ADDED Requirements

### Requirement: Existing anchors remain inspectable without trusted coverage
The Native editor viewport MUST expose bounded physically visible existing accepted
anchor positions independently of supported loop-reference interpolation. Its combined
anchor and projected-line count SHALL remain bounded by the requested limit and at
most 2048. Invalid inputs and overfull anchor ranges MUST explicitly refuse without
partial truncation. An unsupported reference SHALL retain its truthful refusal status
and SHALL NOT produce interpolated grid lines or invented loop-relative beat labels.
Python MUST consume this bounded view without parsing full map geometry or interpolating
uncertain/gap regions. Inspection marks SHALL remain visually distinct from musical grid
lines and SHALL NOT grant snap, bar, tempo or playback qualification.

#### Scenario: An uncertain map contains the current loop reference
- **WHEN** its physical anchors fit the valid visible viewport budget
- **THEN** those existing source positions remain visible as inspection marks
- **AND** the unsupported reference remains explicit with no interpolated grid or loop numbering

#### Scenario: Too many anchors are visible
- **WHEN** the visible existing anchor count exceeds the requested bounded point budget
- **THEN** the viewport refuses explicitly without silently dropping observations or growing beyond the limit

### Requirement: Existing uncertain anchor positions can be corrected independently
The editor MUST allow explicit sample positioning of an exact existing accepted beat
anchor even when adjacent coverage is uncertain or has unknown count. Insertion of a
new beat into unsupported geometry MUST remain refused. Finite beat, actual loaded
sample-rate, physical sample-index and strict monotonicity guards SHALL remain. Position
correction MUST NOT promote or change quarter coordinates, coverage, meter, downbeat
phase or the original anchor review classification. It SHALL retain immutable Raw evidence, explicit sample-position evidence,
parent lineage and exact closed Undo through the existing source-qualified worker,
artifact and durable-before-selection path. Stale revision/content/source and failed
save/cancel MUST preserve the selected predecessor and physical loop markers.

#### Scenario: A detector estimate is corrected before coverage is reviewed
- **WHEN** the performer moves an existing uncertain anchor by one source sample
- **THEN** only its accepted position and position-review evidence change
- **AND** coverage/count/meter/phase remain unchanged and Undo restores the exact predecessor anchor

#### Scenario: A new interior beat lies in an unknown-count gap
- **WHEN** the performer requests insertion by positioning that unsupported beat
- **THEN** the edit refuses and no immutable selection, source marker or audio intent changes

### Requirement: Span endpoints can fill position controls without implying support
The English editor MUST offer actions selecting either existing inspected span endpoint
into its beat/sample position controls using the current Native span and expected map
revision. A virtual endpoint without a physical sample MUST refuse. Selection alone
SHALL NOT submit an edit, change coverage, seek playback or move loop markers. Existing
content identity and revision fences SHALL apply before a subsequent correction.

#### Scenario: The performer chooses an uncertain span endpoint
- **WHEN** the span and its physical endpoint belong to the current displayed revision
- **THEN** the controls receive its exact Native beat and sample index without requiring supported interpolation
- **AND** only the separate Move beat action can admit the existing correction job
