## ADDED Requirements

### Requirement: Prepared Stem Admission Requires Loaded Source Content
The system SHALL identify preparation originals with a versioned complete content
digest and SHALL require that digest to match the current loaded source's retained
original digest before admitting stem preparation. Legacy stat-only cache tokens
SHALL invalidate instead of being silently promoted. Hashing SHALL occur outside
realtime processing and observable changes during a read SHALL reject that read.

#### Scenario: Equal-sized replacement retains its timestamp
- **GIVEN** a loaded source and a replacement with the same path, size and mtime
- **WHEN** stem preparation or cache restoration checks source identity
- **THEN** changed bytes produce a different identity and stale stems are rejected
- **AND** preparation does not adopt the replacement into the old loaded source

#### Scenario: A legacy project restores stat-only stem metadata
- **WHEN** a project restores stem metadata without a content digest
- **THEN** its stems are unavailable until regenerated
- **AND** saved manual, TAP and legacy timing values retain existing behavior

### Requirement: Preparation Tickets Bind Actual Native Publication Ownership
The system SHALL capture and retain an opaque engine-specific source/request and
timing-publication ticket before preparation, recheck actual ownership through
enqueue and mixer adoption, and invalidate pending preparation on source/request,
unload or successful timing changes. Counter wrap SHALL be rejected and failed
publication SHALL preserve the previous prepared state.

#### Scenario: A same-shaped source replaces the preparation source
- **GIVEN** stems aligned to source A's immutable loaded buffer
- **WHEN** source B replaces A before enqueue or callback adoption
- **THEN** the actual source pointer/request check rejects the stems
- **AND** rejected buffer owners retire outside the callback

#### Scenario: Timing changes during preparation or after enqueue
- **GIVEN** a captured preparation ticket
- **WHEN** a pad BPM or origin publication supersedes it before mixer adoption
- **THEN** the pending preparation cannot be adopted
- **AND** successfully adopted same-source stems remain usable after later edits
- **AND** repeated equal-valued timing edits also invalidate pending preparation

#### Scenario: The control queue is full
- **WHEN** prepared stem enqueue fails
- **THEN** previous prepared state is preserved and the new set is not published

#### Scenario: Offline analysis changes the shared request
- **GIVEN** prepared stems queued for the current source
- **WHEN** offline analysis admission or cancellation advances that pad's request
- **THEN** callback adoption rejects the queued stems even if the source buffer is unchanged

#### Scenario: An ownership counter is exhausted
- **WHEN** a source/request or timing publication would wrap its counter
- **THEN** it fails without reusing an earlier ticket identity

### Requirement: Stem Controls Wait For Native Acceptance
The system SHALL keep queued stem controls unavailable until actual callback
acceptance and SHALL report late callback rejection through bounded feedback.

#### Scenario: Callback adoption rejects a previously queued set
- **WHEN** source or timing changes invalidate a successfully enqueued set before callback acceptance
- **THEN** bounded native feedback reports rejection
- **AND** Python leaves stem controls unavailable and reports the failed publication

### Requirement: Separator Jobs Own Isolated Artifacts And Shared Trajectory
The system SHALL isolate separator artifact writes per admitted job and SHALL
promote only the current job's complete output. It SHALL bind prepared playback to
the actual alignment reference and preserve one source-frame trajectory for the
full mix and same-source stems. Callback checks SHALL use bounded pointer/scalar
operations with no hashing, disk work, GIL access or blocking locks.

#### Scenario: An obsolete same-source separator finishes late
- **GIVEN** a newer job for the same pad and original bytes
- **WHEN** an old job finishes after unload/reload or replacement
- **THEN** its ticket identity cannot consume the new job or overwrite its canonical artifacts

#### Scenario: Accepted same-source stems follow a fractional rate
- **GIVEN** accepted stems at a fractional rate and source phase
- **WHEN** callbacks are partitioned differently or timing is edited later
- **THEN** stems and full mix retain the same SourcePlayback trajectory

### Requirement: Canonical Stem Sets Require A Complete Integrity Marker
The system SHALL bind complete canonical stem sets to a marker identifying source
version and all artifact digests, publish that marker last, and reject incomplete
or tampered restoration.

#### Scenario: Promotion is interrupted between artifact replacements
- **WHEN** promotion stops before the new complete-set marker is published
- **THEN** restoration does not make the mixed/incomplete set available

### Requirement: Performance Stem Commands Avoid Full File Hashing
The system SHALL avoid full original content hashing in performance mode/mask
commands while retaining native loaded-source identity checks.

#### Scenario: A performer changes a prepared stem mask
- **WHEN** an accepted loaded-source stem mask or mode changes
- **THEN** the command checks native source identity without hashing the full original file

### Requirement: Legacy Native Generation Cannot Bypass Admission
The legacy native placeholder generator SHALL fail without writing canonical
artifacts; it SHALL NOT bypass the isolated ticket-bound separator path.

#### Scenario: A caller invokes the old native placeholder separator
- **WHEN** the old exported generation API is called
- **THEN** it reports that the legacy path is disabled
- **AND** no worker starts and no canonical artifact is overwritten
