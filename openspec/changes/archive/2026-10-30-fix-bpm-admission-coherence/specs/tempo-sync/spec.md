## ADDED Requirements

### Requirement: BPM intent follows actual primary admission through derived failure
Manual, clear and tap BPM actions SHALL preserve prior project intent when the primary Native BPM publication is refused. After that publication is admitted, the project SHALL retain and mark dirty its accepted override and actual legacy timing authority before any derived publication can fail. Manual or tap authority SHALL be projected only after its own Native declaration is admitted. A later derived refusal SHALL remain visible without discarding admitted intent or automatically replaying the gesture.

#### Scenario: Derived origin, timing declaration or loop publication is refused
- **GIVEN** Native already admitted the original source-bound BPM publication
- **WHEN** a later derived publication is refused
- **THEN** persistence records the admitted override and actual declared authority
- **AND** no rollback or automatic frame replay changes that accepted state

### Requirement: BPM Lock projection follows Native flag admission
BPM Lock SHALL stage its anchor data and publish required current grid data before its Native flag. A grid or flag refusal SHALL preserve the previous Python flag and anchor and SHALL allow an identical later explicit retry. Once the Native flag is admitted, project flag/anchor intent SHALL be marked dirty before derived master/phase work. Derived refusal SHALL preserve that admitted flag and only actually accepted master projection. Existing automatic timing safeguards and source readiness SHALL remain enforced.

#### Scenario: Primary BPM Lock publication is refused
- **GIVEN** a request to enable or disable BPM Lock
- **WHEN** its required grid or Native flag publication is refused
- **THEN** project flag and anchor remain unchanged and the same explicit request can retry

#### Scenario: Master or phase work is refused after BPM Lock admission
- **GIVEN** Native admitted the original BPM Lock flag
- **WHEN** later master or phase publication is refused
- **THEN** the admitted flag and anchor remain persistable without fabricated master effect

### Requirement: Shared legacy publication mirrors authority at its actual seam
Every admitted legacy BPM or grid-origin publication on the shared maintenance path SHALL project Legacy intent and mark changed project authority dirty before any later call can fail. Refused calls and accepted-Automatic paths that publish nothing SHALL leave that projection unchanged. A requested manual or tap redeclaration SHALL be captured before such intermediate publications and SHALL be restored only after its own Native declaration is accepted.

#### Scenario: BPM Lock origin is admitted but its flag is refused
- **GIVEN** current manual or tap authority on the selected pad
- **WHEN** its legacy grid origin is accepted and the later Lock flag is refused
- **THEN** Legacy authority remains persistable while Lock flag and anchor stay unchanged

#### Scenario: Shared refresh must redeclare the original TAP intent
- **GIVEN** the shared refresh began with an explicit manual or tap request
- **WHEN** intermediate legacy publications and the later original declaration succeed
- **THEN** the project mirrors the original declared authority without converting TAP to Manual
