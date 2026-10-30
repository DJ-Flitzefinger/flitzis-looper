## ADDED Requirements

### Requirement: Global transport admission failures remain visible and nonfatal
The global START/STOP GUI SHALL contain expected RuntimeError/ValueError admission failures
and display feedback on its existing control while preserving saved state and the previous
acknowledged restore set. It SHALL NOT manufacture a STOP acknowledgement, silently issue
an unguarded fallback, or retain an uncaptured command to affect later replacement content.
A later explicit successful gesture SHALL clear the previous GUI admission error.
Each right-button press SHALL issue at most one STOP admission; holding that press across
render frames SHALL NOT recapture or retry a rejected action against later content.

#### Scenario: Right-click STOP meets a persistently busy publication during analysis
- **GIVEN** PAD2 is being loaded or analyzed while the performer right-clicks START/STOP
- **WHEN** bounded native cancellation still cannot obtain its publication claim
- **THEN** the GUI stays responsive and shows that STOP was not admitted
- **AND** active, paused and remembered restore state are not falsely changed
- **AND** loading/analysis continues and a later genuine STOP may be admitted normally

#### Scenario: A guarded stop batch is rejected after cancellation
- **GIVEN** launch cancellation succeeds but source validation or batch admission fails
- **WHEN** the GUI handles the failure
- **THEN** the application stays running and exposes that admission error
- **AND** no new start, raw STOP fallback or fabricated stopped state is produced

#### Scenario: A rejected right press remains held while source publication finishes
- **GIVEN** a right-button press on START/STOP has failed admission
- **WHEN** publication finishes or source content changes while that button remains held
- **THEN** later render frames do not repeat cancellation or capture a replacement source
- **AND** the error remains visible until a new explicit successful transport gesture
