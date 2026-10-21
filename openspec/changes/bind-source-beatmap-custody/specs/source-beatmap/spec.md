## ADDED Requirements

### Requirement: Source map acceptance retains actual source custody
The system MUST distinguish supported historical map parsing from acceptance
against an actual current source. Native map-purpose work SHALL reuse existing
offline admission, original content/request freshness, cancellation and retirement
owners. It SHALL require the exact held CommittedColdLease for complete as well as
finite residency, verify its actual reference and original off realtime, and match
the validated map decoder identity to that held source. Equal bytes or metadata
alone MUST NOT substitute for original content lifetime and request ownership.
Map-purpose work MUST snapshot the existing source request and cell epoch. Its
begin/prepare/finish/cancel/drop MUST NOT advance the numeric source request,
logical source-request incarnation or preparation epoch. Explicit cancellation
SHALL mutate only its original map token. Existing analysis-purpose request
advancement SHALL remain unchanged.

Map-only validation SHALL NOT require mono export or its analysis-size budget.
Analysis-only export, inference, diagnostic finish, PCM staging statistics and
early PCM retirement APIs MUST refuse map-purpose work. Cancellation/drop SHALL
retain source readers and admission until actual native work ends; unstarted abort
SHALL retire without fabricating a Beat/Key result. No heavy work SHALL run in the
audio callback, and worker hashing/parsing SHALL release the GIL.

#### Scenario: A valid record names a different actual decoder
- **WHEN** internally consistent supported map data names a decoder other than the original held assignment
- **THEN** preparation refuses the map without changing current source, timing or prior selection
- **AND** ordinary geometry parsing alone remains available as historical data

#### Scenario: A map needs no mono export
- **WHEN** a real source with a bounded resident window admits map-purpose validation
- **THEN** only existing source readers and bounded map data are required
- **AND** no diagnostic export-size restriction, PCM export or fabricated zero staging is applied

#### Scenario: Ordinary analysis keeps its strict metadata contract
- **WHEN** an existing analysis-purpose job exposes its Native metadata
- **THEN** its existing strict Python metadata schema remains valid without additional map-purpose fields
- **AND** map-specific method isolation does not alter ordinary export or diagnostic completion

#### Scenario: Map data work preserves already admitted playback
- **GIVEN** ordinary and piano retrigger actions are sealed for a future start
- **WHEN** source-map data validation is admitted, completed or cancelled
- **THEN** their original source-request fences remain current and they can execute normally
- **AND** a real external source or analysis-request supersession still revokes stale map completion

### Requirement: Map completion follows the original native request
Map finish MUST revalidate the original NativeAnalysisOwner and request under the
existing publication transition. A dedicated NativeOwned completion SHALL carry
the immutable native map and original identity. Polling MUST NOT serialize the
complete map JSON. The completion SHALL supply no current audio, project selection
or musical acceptance authority by itself.

#### Scenario: Original content moves before completion
- **WHEN** map preparation overlaps a genuine Move/Pair callback ACK and control mirror
- **THEN** the same original owner waits through pending placement and delivers only to its current cell
- **AND** the independent equal-source peer retains its own state

#### Scenario: Completion becomes stale before delivery
- **WHEN** source lifetime, original request or cancellation invalidates a prepared or queued completion
- **THEN** it cannot become a current map for the replacement occupant or another engine
- **AND** actual readers and shared offline capacity retire only after work ends

#### Scenario: Explicit cancellation and finished handle retirement differ
- **WHEN** the original caller explicitly cancels after map finish but before delivery, including a pending placement mirror
- **THEN** the queued completion is revoked through that same original cancellation owner
- **AND** simply dropping a successfully finished handle preserves valid queued completion
