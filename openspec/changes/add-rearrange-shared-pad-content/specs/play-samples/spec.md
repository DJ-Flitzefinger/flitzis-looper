## ADDED Requirements

### Requirement: Native content mutation and owned pause release linearize
The system SHALL atomically validate content/lifetime/slot epochs and native source/voice/cohort/control/effect guards at layout or release application and SHALL publish matching complete ACK-bound projections.

Move/Swap SHALL carry living history/voices/owned effects without unload/reload.
Delete/reassign/overwrite/bank removal SHALL fence every removed lifetime/action
before reuse; queued reader pins SHALL survive until real native terminal retirement.
Slot-only late started/stopped/paused telemetry SHALL NOT change a different lifetime.

#### Scenario: Release and move execute in either order
- **WHEN** release occurs before or after a native Move/Swap commit
- **THEN** it affects the same owned surviving cohort through the proper projection
- **AND** removal instead yields retired/noeffect with no replacement mutation

### Requirement: Callback commit stays bounded and retirement is deferred
The system SHALL execute layout/pitch/hold application using prepared fixed guards/handles and reserved capacity, with all file/JSON/Python/GIL/UI/locks/logging/inference/heavy allocation/destruction and unbounded scans outside the callback.

#### Scenario: Capacity is insufficient for complete bank transaction
- **WHEN** all36 refs/actions/native handles/ACK/retirement reservations cannot be secured
- **THEN** admission fails atomically while old audio/owners persist
- **AND** measured future capacity/drain proof is required rather than assuming document shapes are RT proof
