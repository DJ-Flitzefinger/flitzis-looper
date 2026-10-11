## MODIFIED Requirements

### Requirement: Persist Per-Pad Key Lock Intent
The system SHALL persist the global boolean Key Lock default and loaded pads' nullable override
intent. The versioned representation SHALL use strict integer `key_lock_intent_version: 2` and
reuse `pad_key_lock` as a fixed-length list of strict `bool | null` values matching the pad count.
`null` SHALL mean inheritance; `true` and `false` SHALL mean explicit ON and OFF. New projects
SHALL initialize every override to `null`. Unloaded pads SHALL restore and save `null` overrides.

Legacy files without this discriminator SHALL migrate loaded pads' existing boolean entries to
explicit overrides preserving their previous audible intent. If the legacy per-pad field is
absent, every pad SHALL inherit the restored global default. Unloaded legacy entries SHALL clear
to `null`. Invalid length, entry types or an unsupported/non-integer version SHALL follow the safe
project-load validation path. Global changes SHALL persist the default without rewriting overrides.
Save/restore SHALL NOT replace requested origin with transient preparation, failure or confirmed
Native state.

#### Scenario: Missing legacy per-pad values inherit the restored default
- **GIVEN** a legacy project omits `pad_key_lock` and has global Key Lock enabled
- **WHEN** the project is restored
- **THEN** all pad overrides are `null`
- **AND** restored loaded pads request enabled Key Lock from the global default

#### Scenario: Loaded legacy booleans preserve explicit audible intent
- **GIVEN** a legacy project has loaded Pad 3 set to `false` and loaded Pad 4 set to `true`
- **AND** the restored global default is enabled
- **WHEN** the project is migrated
- **THEN** Pad 3 retains explicit OFF and Pad 4 retains explicit ON
- **AND** later global changes do not rewrite either override

#### Scenario: Nullable origins round-trip
- **GIVEN** loaded pads have inherited, explicit ON and explicit OFF intent
- **WHEN** the project is saved and restored
- **THEN** `key_lock_intent_version` is `2`
- **AND** the corresponding `pad_key_lock` entries remain `null`, `true` and `false`
- **AND** their origins remain distinct even when their effective boolean modes match

#### Scenario: Global changes preserve durable override origins
- **GIVEN** a project has inherited and explicit pad overrides
- **WHEN** the performer changes the global default and saves
- **THEN** the saved global default changes
- **AND** every loaded pad's override remains unchanged
- **AND** unloaded entries are saved as `null`

#### Scenario: Unloaded legacy intent is cleared
- **GIVEN** a legacy project contains `true` for an unloaded pad
- **WHEN** the project is restored
- **THEN** that pad's override is `null`
- **AND** it does not publish stale enabled live state

#### Scenario: Failed Native request is not saved as observed state
- **GIVEN** loaded Pad 3 requests explicit ON while its confirmed Native state remains OFF after failure
- **WHEN** the project is saved and restored
- **THEN** Pad 3's persisted override remains `true`
- **AND** the failure and confirmed Native projection are not restored as durable audio truth

#### Scenario: Invalid per-pad Key Lock length is rejected
- **GIVEN** a project file contains a per-pad Key Lock list with fewer or more entries than the project pad count
- **WHEN** the project is loaded
- **THEN** the invalid project data fails model validation or falls back through the safe project-load path
- **AND** the application remains usable

#### Scenario: Invalid nullable entry or version is rejected
- **GIVEN** a project contains a non-boolean/non-null override or a version other than strict integer `2`
- **WHEN** the project is loaded
- **THEN** the invalid data fails model validation or follows the safe project-load fallback
- **AND** the application remains usable
