## ADDED Requirements

### Requirement: Pending material retirement makes bounded fair progress
The existing background retirement worker SHALL attempt at most eight pending material roots per lap with fair rotation. Persistent sharing failures or an ownership gate delaying earlier roots SHALL NOT starve later eligible roots. Pending routing SHALL retain one entry per admitted pending root, preserve its place on repeated pending declarations and append newly pending work after existing work. The existing admission bound SHALL bound that routing index. Routing SHALL confer no deletion authority: exact birth receipts, FileID, no-reparse, reader and current ownership checks SHALL remain enforced.

#### Scenario: Eight pending roots remain blocked by actual sharing handles
- **GIVEN** nine pending owned roots and sharing blockers on the first eight selected
- **WHEN** the worker completes two bounded material laps
- **THEN** the ninth eligible root receives its actual retirement attempt
- **AND** the eight blocked roots preserve their exact binding bytes and FileIDs

#### Scenario: A blocked marker is replaced before retry
- **GIVEN** a pending root whose original marker retirement failed with sharing pressure
- **WHEN** an equal-byte marker with a new FileID replaces that marker before retry
- **THEN** the old creation receipt preserves the replacement and terminates that attempt

### Requirement: Cleanup counts only completed physical retirement
The public completed-deletion count SHALL count an actual retired PCM generation once at its physical deletion worker. Handoff between existing asset and ColdStore queues SHALL NOT increment that completed count. Pending work, terminal outcomes, sharing retries and the exact existing deletion authority SHALL remain unchanged.

#### Scenario: Verified PCM retirement is handed off while a sharing handle blocks deletion
- **GIVEN** a verified generation with an independent handle that denies deletion
- **WHEN** asset retirement transfers it to the existing ColdStore queue
- **THEN** the completed count remains unchanged while actual retirement is pending
- **AND** releasing the handle permits physical completion and exactly one increment
