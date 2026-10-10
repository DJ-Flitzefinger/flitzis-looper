## ADDED Requirements

### Requirement: Legacy pitch migration stays audibly neutral
The system SHALL persist separate source correction/epochs/base_shift/extra_shift/retrigger intent and SHALL migrate prior manual_key display overrides as metadata with base0,extra0,retriggerOFF.

#### Scenario: Old arbitrary key string and new independent copies restore
- **WHEN** an old project with arbitrary manual_key opens or new musical copies save/reopen
- **THEN** old audio stays unchanged, unknown text is preserved as metadata and new copies restore independent settings
- **AND** no new analysis or native token/physical press is restored

#### Scenario: Explicit correction removal survives repeated reopen
- **GIVEN** a new key-intent field with correction explicitly None and retained legacy manual_key text
- **WHEN** the project saves and reopens repeatedly
- **THEN** the new field remains authoritative and the old correction is not resurrected
- **AND** valid numeric shifts, retrigger and metadata epochs are preserved

#### Scenario: Unsupported new key evidence preserves other performer settings
- **GIVEN** an otherwise valid project with malformed source-key evidence or a malformed new key field
- **WHEN** persistence restores that project
- **THEN** only invalid key fields fall back locally to neutral values
- **AND** valid correction, base, extra and retrigger fields in the same pad and all other performer settings remain intact
- **AND** direct policy and project-model mutation reject invalid strict integer, boolean, epoch, mode and fixed216-list inputs before changing intent

### Requirement: Accepted replacement alone resets source bound pitch
The system SHALL reset correction/base/extra/retrigger to neutral defaults only on successful true audio reassignment and SHALL preserve their intent for failed admission, Copy, Move/Swap and same-source project restore.

#### Scenario: Equal byte reassignment still creates a fresh lifetime
- **WHEN** the matching successful native source ACK and reserved original owner are adopted for true replacement even for equal bytes/path
- **THEN** source-bound pitch is neutral and old actions/holds are retired
- **AND** correction, base, extra and retrigger reset even if a newer timing edit makes the load's timing projection stale
- **AND** failure before admission preserves old lifetime/settings and Copy/Move/restore do not reset them
