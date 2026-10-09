## ADDED Requirements

### Requirement: Legacy pitch migration stays audibly neutral
The system SHALL persist separate source correction/epochs/base_shift/extra_shift/retrigger intent and SHALL migrate prior manual_key display overrides as metadata with base0,extra0,retriggerOFF.

#### Scenario: Old arbitrary key string and new independent copies restore
- **WHEN** an old project with arbitrary manual_key opens or new musical copies save/reopen
- **THEN** old audio stays unchanged, unknown text is preserved as metadata and new copies restore independent settings
- **AND** no new analysis or native token/physical press is restored

### Requirement: Accepted replacement alone resets source bound pitch
The system SHALL reset correction/base/extra/retrigger to neutral defaults only on successful true audio reassignment and SHALL preserve their intent for failed admission, Copy, Move/Swap and same-source project restore.

#### Scenario: Equal byte reassignment still creates a fresh lifetime
- **WHEN** true replacement is admitted even for equal bytes/path
- **THEN** source-bound pitch is neutral and old actions/holds are retired
- **AND** failure before admission preserves old lifetime/settings and Copy/Move/restore do not reset them
