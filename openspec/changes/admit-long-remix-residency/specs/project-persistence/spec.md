## ADDED Requirements

### Requirement: Persist automatic residency and optional manual allowance
ProjectState SHALL persist automatic live-resident admission separately from
the existing integer128..16384-MiB manual allowance. Automatic admission SHALL
be the default for new projects and old projects with the former default512-MiB
allowance; a missing mode with a saved nondefault allowance SHALL remain manual.
The numeric allowance SHALL survive automatic/manual mode switches.

#### Scenario: Restore and explicit manual policy
- **WHEN** a project is restored or the performer switches admission policy in Settings
- **THEN** the validated Native automatic or manual policy SHALL be applied before startup admission or persisting the edit
- **AND** a Native rejection SHALL leave the prior persisted policy unchanged
- **AND** manual mode SHALL retain its aggregate ownership bound in addition to actual host-memory admission
- **AND** selecting automatic mode SHALL NOT discard existing readers or reset the stored manual allowance

#### Scenario: Truthful memory feedback
- **WHEN** Settings reports memory admission and residency
- **THEN** it SHALL distinguish automatic machine admission from an explicit manual cap and exact held/pending PCM
- **AND** it SHALL NOT label those counters whole-process RSS, infer allocation success from a snapshot, or persist runtime readiness/headroom

#### Scenario: Manual RAM slider uses whole GiB steps
- **WHEN** the performer changes the manual RAM allowance slider
- **THEN** the control SHALL offer whole1-GiB steps and communicate its RAM allowance in GiB
- **AND** the existing MiB persistence/API representation SHALL retain its full supported precision
- **AND** an existing nonwhole-GiB allowance such as9925MiB SHALL remain unchanged until an actual edit and SHALL be displayed truthfully
