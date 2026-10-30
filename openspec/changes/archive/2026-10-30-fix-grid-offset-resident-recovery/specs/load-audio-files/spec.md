## ADDED Requirements

### Requirement: Adopted resident storage is reconciled independently of timing edits
The system SHALL reconcile an original resident transaction's already observed physical
storage adoption even if a later timing edit changes its authority revision. Reconciliation
MUST retain the exact original publication, content lifetime/cell, source generation,
source identity, adopted window and cache predecessor guards. It SHALL NOT authorize a
rejected launch, overwrite current timing or accept a replaced source or window. All cache
mirroring and final destruction SHALL remain outside the realtime callback.

#### Scenario: Grid edit follows native resident adoption before cache mirroring
- **GIVEN** the callback adopted a finite loop window with its genuine stem readers
- **AND** the control cache still holds that transaction's original predecessor window
- **WHEN** a later grid-origin edit changes timing authority before reconciliation
- **THEN** reconciliation mirrors the exact adopted physical window and matching stems
- **AND** a fresh current binding and start capture use that coherent source
- **AND** the original rejected ticket does not acquire new playback authority

#### Scenario: Source or physical window has been replaced
- **GIVEN** the original resident transaction no longer owns the current source or window
- **WHEN** its old storage receipt is reconciled
- **THEN** no old sample or stem reader overwrites the replacement
