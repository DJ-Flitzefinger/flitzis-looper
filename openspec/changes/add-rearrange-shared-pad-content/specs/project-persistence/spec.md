## ADDED Requirements

### Requirement: Copies restore independently of their former origin
The system SHALL persist durable ContentInstance identity/lineage and independent musical assignments and shared immutable version references across all banks and SHALL restore surviving copies after deleting origin slot/bank with fresh current native source/timing/residency ACK.

#### Scenario: Origin bank removed then new process opens
- **WHEN** copies save after origin-bank deletion and project closes/reopens
- **THEN** source/analysis/stems/loop/timing/GainEQ/correction/base/extra/masks/preset/mutes/retrigger restore independently
- **AND** valid prepared data need no new analysis/separation/complete decoding/file duplicates
- **AND** voices/cursors/DSP history/progress/job/physical hold and old native ACK do not restore as live authority

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
