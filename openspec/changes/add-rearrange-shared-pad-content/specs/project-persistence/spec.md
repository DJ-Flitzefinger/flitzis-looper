## ADDED Requirements

### Requirement: Copies restore independently of their former origin
The system SHALL persist durable ContentInstance identity/lineage and independent musical assignments and shared immutable version references across all banks and SHALL restore surviving copies after deleting origin slot/bank with fresh current native source/timing/residency ACK.

#### Scenario: Origin bank removed then new process opens
- **WHEN** copies save after origin-bank deletion and project closes/reopens
- **THEN** source/analysis/stems/loop/timing/GainEQ/correction/base/extra/masks/preset/mutes/retrigger restore independently
- **AND** valid prepared data need no new analysis/separation/complete decoding/file duplicates
- **AND** voices/cursors/DSP history/progress/job/physical hold and old native ACK do not restore as live authority

#### Scenario: Equal finite restored ranges share verified PCM
- **GIVEN** surviving copies request the same source range and fully verified immutable cache interpretation
- **WHEN** they restore concurrently through independent current Native assignments
- **THEN** equal ranges SHALL share live PCM backing and immutable complete source identity through the existing cache
- **AND** each assignment SHALL retain a fresh view, independent source custody, scoped resident admission and its own Native ACK
- **AND** mismatched source identity, extent or range, dead reader views and exhausted locator capacity SHALL NOT grant reuse or bypass resource admission

#### Scenario: Concurrent recovery briefly holds unused warm staging
- **GIVEN** a verified warm restore has an unused staging directory with its exact owned FileID
- **WHEN** a concurrent recovery reader temporarily prevents removal of that staging directory or its marker
- **THEN** the restore SHALL retain the verified cache and defer only its unused staging through the existing bounded identity-checked cleanup queue
- **AND** unrelated files, substituted generations and unknown cleanup errors SHALL NOT gain deletion authority or be silently accepted

#### Scenario: Saved lifetime cannot regain runtime authority
- **WHEN** a project reopens content with the same durable ContentInstance lineage
- **THEN** a fresh, nonreused runtime lifetime is allocated before accepting current native authority
- **AND** serialized old lifetime/action/feedback/HoldRelease tokens cannot authorize that reopened content
- **AND** durable musical intent and immutable references round-trip without restoring live voices or cohorts

### Requirement: Layout persistence and crash recovery follow native acceptance
The system SHALL serialize layout/reference/config/journal/autosave revisions and SHALL recover only recognized contained immutable transaction data without overwriting newer intent or fabricating historical ACK authority.

#### Scenario: Crash or newer edit around native claim
- **WHEN** transaction crashes before/after claim or autosave/newer musical edit races
- **THEN** verified recovery preserves old/new pins/current intent and obtains genuine fresh native ACK
- **AND** uncertainty stays visible/fenced until settled; no partial bank or rollback guess is published

### Requirement: Musical stem selection has one durable content intent
The system SHALL persist one validated per-content stem-selection intent containing
the enabled component mask, remembered custom mask and displayed custom/instrumental/all
preset. Component mutes SHALL be represented by the disabled mask bits without a
second mutable mute table. FullMix/ALL playback mode SHALL remain orthogonal.
Masks MUST be strict integers within the four component bits, including zero;
the intent schema and complete 216-slot table MUST be validated. Session state
SHALL NOT retain a second independent requested mask/custom/preset truth.

#### Scenario: Legacy and partially malformed project selection
- **WHEN** a legacy project lacks the durable selection table
- **THEN** each slot SHALL default to all components, remembered all and display all while preserving its saved FullMix/ALL intent
- **WHEN** one saved selection is malformed
- **THEN** contained metadata recovery SHALL preserve other valid musical settings and slots, report the rejected field and default only the invalid selection
- **AND** boolean masks, unsupported schemas and oversized tables SHALL NOT silently authorize a selection

#### Scenario: Copy and reopen preserve intent without copied eligibility
- **WHEN** custom masks, preset switches and component mutes are captured into a CopySnapshot and later reopened
- **THEN** the target SHALL retain independent enabled/custom/preset intent and the existing preset-toggle behavior
- **AND** a missing prepared set or failed Native publication SHALL preserve requested intent with separate effective/pending/error feedback
- **AND** source replacement or unload SHALL reset only the affected content through its matching assignment lifecycle
- **AND** copied available flags, source/timing generations or historical receipts SHALL NOT grant target playback eligibility

#### Scenario: Prepared set deletion preserves the content's musical selection
- **WHEN** a prepared set is deleted, becomes incompatible or is regenerated while the source content survives
- **THEN** enabled/custom/preset intent SHALL remain available for that content without authorizing unavailable stems
- **AND** FullMix fallback, independently pending/effective Native state and actual retirement guards SHALL retain their existing ownership rules

#### Scenario: A later launch withdraws an accepted stem receipt
- **WHEN** a previously accepted receipt loses current output authority after an ordinary launch while its explicit residency demand remains recorded
- **THEN** durable ALL/mask/preset intent and valid immutable data SHALL remain intact with truthful unconfirmed effective state
- **AND** the actual matching receipt withdrawal SHALL retire only that historical demand's implicit retry interest before periodic demand advancement
- **AND** polling SHALL NOT request a replacement live receipt without a fresh explicit gesture, nor withdraw newer explicit intent or independent preload interest

#### Scenario: Fresh mask gesture follows a withdrawn output receipt
- **GIVEN** an ordinary launch withdrew an accepted stem output receipt and retired its historical retry interest
- **WHEN** the Human changes the persisted ALL mask or preset and the fresh request meets bounded queue or PCM admission pressure
- **THEN** the existing explicit demand path SHALL retain that fresh gesture for retry when capacity becomes available
- **AND** a fresh own receipt SHALL be required; automatic source projection alone SHALL NOT recreate explicit demand
