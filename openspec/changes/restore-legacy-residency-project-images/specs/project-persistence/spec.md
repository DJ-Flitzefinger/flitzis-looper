## ADDED Requirements

### Requirement: Strict compatibility for historical arrangement project images
The shared Pair, Copy and Bank project-image reader SHALL accept otherwise
complete historical images omitting `resident_pcm_budget_automatic`,
`pad_source_beatmap_intent`, or both. It MUST apply the existing ProjectState
migrations without modifying original sealed image bytes or their digests.
It MUST retain bounded, duplicate-key-free, finite, strict validation and reject
every other missing or unknown field, inconsistent nested metadata and duplicate
content identity. Explicitly present fields MUST retain their strict semantics.

#### Scenario: Restore historical automatic and manual allowances
- **WHEN** a valid historical arrangement image omits the automatic admission flag
- **THEN** its 512-MiB allowance SHALL migrate to automatic admission
- **AND** a nondefault allowance such as 9925 MiB SHALL remain manual
- **AND** omission of the historical source-map intent field SHALL compose with this migration
- **AND** a present explicit manual flag with 512 MiB SHALL remain manual

#### Scenario: Settled legacy history does not block normal Close
- **GIVEN** original sealed Pair, Copy or Bank history is settled and otherwise valid
- **AND** its image omits only the supported historical fields
- **WHEN** startup inspects that history and the application later prepares Close
- **THEN** the missing historical fields SHALL NOT fabricate unresolved recovery or acquire a config-writer fence
- **AND** normal Close SHALL use the existing actual-work settlement checks
- **AND** config and journal bytes SHALL remain unchanged by history inspection

#### Scenario: Damaged history remains guarded
- **WHEN** an image has another missing or unknown field, invalid nested data or a changed digest
- **THEN** historical compatibility SHALL NOT authorize accepting that image, writing config, clearing another writer fence or inventing Native settlement
