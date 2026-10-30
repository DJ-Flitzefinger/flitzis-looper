## ADDED Requirements

### Requirement: Optional independently supported nominal beat unit

The system SHALL support one verified optional CPU model to arbitrate between original
successful NN4/5 and independently admitted own-QM half/double candidates. The
published periods SHALL agree within5ms over the complete common extent. A choice
SHALL require unique in-vocabulary classes and a unique strict model preference
for QM, preserving the exact own-QM scalar and unchanged original observations.

#### Scenario: Candidate outside a captured enabled range
- **WHEN** the original neural value is outside the captured range
- **THEN** existing range7 SHALL run exclusively without nominal arbitration
- **AND** a nominal comparison otherwise SHALL reject an outside QM candidate before classification

#### Scenario: No choice or optional-stage failure
- **WHEN** no eligible comparison exists, the model does not uniquely prefer QM, or optional work fails
- **THEN** the exact original scalar, method and independent key SHALL remain
- **AND** ordinary NoChoice SHALL be silent while exceptional failure MAY report a bounded diagnostic
- **AND** cancellation, source supersession and final publication SHALL retain existing ownership fences

#### Scenario: Separate verified runtime and process memory
- **WHEN** optional classification is attempted
- **THEN** it SHALL use one captured retained profile and the existing protected complete mono lease
- **AND** frontend and Node SHALL retire sequentially under existing owned-process supervision
- **AND** declared parent/bridge and child current/peak private-memory accounting SHALL retain512MiB before successful acceptance
- **AND** no analysis-time install, download, PATH lookup or new service SHALL occur

#### Scenario: Persisted packed model evidence
- **WHEN** schema8 analysis is saved, restored or copied
- **THEN** exact packed f32 distributions and binary64 summaries SHALL replay the selection with source/profile geometry and pins
- **AND** original neural math and own-QM evidence SHALL remain separately valid without DSP or runtime lookup
- **AND** historical cached observations SHALL retain truthful origin rather than claim fresh execution
