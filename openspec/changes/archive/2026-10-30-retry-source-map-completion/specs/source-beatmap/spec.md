## ADDED Requirements

### Requirement: Late placement pending preserves the original accepted completion
The system MUST retain an accepted-map completion when original private job
resolution reports the exact content-placement-pending condition after the
loader's initial owner lookup succeeded. It SHALL retry the same delivery through
the existing held-event queue and bounded polling path, without a new admission,
replacement payload or duplicate event. Both selection and restore SHALL obey
this rule.

Until genuine resolution succeeds, the pending job and its reservation MUST stay
owned, and requested intent, durable configuration, previous effective geometry
and playback delivery MUST remain unchanged. Later cancellation or staleness MUST
still prevent publication. Unrelated resolution errors MUST remain visible.
Successful retry SHALL use the existing durable-before-selection publication and
retirement path exactly once.

#### Scenario: Placement starts between two original owner checks
- **WHEN** the loader first resolves the original owner but private map resolution reports pending placement
- **THEN** polling retains that same completion and pending job without changing configuration or the previous effective map
- **AND** a later current resolution publishes once without admitting another job

#### Scenario: Selection is cleared before the held completion is retried
- **WHEN** the original pending map job is cancelled by a successful clear
- **THEN** retry cannot restore its obsolete selection or effective geometry

#### Scenario: The original job reports another resolution failure
- **WHEN** private map resolution raises an error other than the exact placement-pending condition
- **THEN** the error remains visible and is not converted into an indefinite held retry
