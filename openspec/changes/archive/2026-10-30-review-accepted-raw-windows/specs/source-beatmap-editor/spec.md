## ADDED Requirements

### Requirement: Accepted captured maps support bounded explicit review windows
The selected captured source map SHALL support extending either outer end with
up to63 explicit Raw labels or replacing at most62 interior anchors between two
fixed endpoints with up to62 explicit Raw/Existing labels. Source positions SHALL
be resolved from the current embedded capture or current Existing row, never
client coordinates. Exact map/capture revisions, strict finite labels, physical
new-Raw positions, uniqueness and source/quarter ordering MUST be checked before
an atomic edit. Unsupported/no-op requests MUST preserve the selected map.

#### Scenario: A full map is reviewed in successive bounded windows
- **WHEN** explicit valid windows extend64 anchors to127 and then190
- **THEN** all earlier anchor/interval rows and complete captured evidence remain exact
- **AND** the new geometry is processed through the existing asynchronous edit/publication path

### Requirement: Window repair preserves corrections and outside qualification
Fixed endpoint rows and all untouched exterior rows/intervals MUST remain exact.
An interior Existing reference SHALL preserve all position/provenance fields and
change only its explicit quarter bits. Only a replacement window may remove
omitted interior rows. New/replaced intervals MUST begin uncertain with unknown
meter/phase. A window SHALL NOT shift suffix labels or silently qualify coverage.

#### Scenario: A corrected anchor receives a non-affine count repair
- **WHEN** its Existing reference receives a new explicit interior quarter label
- **THEN** source bits, id, Raw reference, review and complete sample_edit remain unchanged
- **AND** outside qualification survives while changed intervals require separate review

### Requirement: Review windows have closed Undo and gesture currentness
Window Undo MUST require exact inverse shape, predecessor reconstruction, digest
and deterministic forward replay. Generic lineage or a plausible inverse SHALL
NOT supply authority. GUI Apply MUST bind source, map/capture revisions, mode,
endpoints, page and draft through the whole press/release gesture and use the
existing selection/edit admission. Stale, busy, cancel and save failures MUST
preserve prior selection and markers.

#### Scenario: A draft or source changes while Apply is pressed
- **WHEN** the release no longer has the captured complete gesture identity
- **THEN** no window edit is submitted
- **AND** a later fresh gesture requires the current explicit draft and endpoints

#### Scenario: A self-consistent inverse names an impossible predecessor
- **WHEN** reconstructed predecessor metadata has a non-null non-digest or self parent
- **THEN** closed Undo is unavailable even if its forged predecessor digest matches
- **AND** valid null or digest parents and existing interval Undo remain supported
