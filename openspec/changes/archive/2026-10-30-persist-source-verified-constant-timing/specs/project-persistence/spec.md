## ADDED Requirements

### Requirement: Save Complete Source-Verified Accepted Timing
The system SHALL persist supported COMPLETE accepted timing evidence only from
the acknowledged current native owner after verifying actual source bytes and
complete owned channel-mean PCM, loaded rate, full extent and source zero.

#### Scenario: Current accepted evidence is saved
- **GIVEN** a supported COMPLETE native record is current and acknowledged
- **WHEN** project persistence verifies the actual source and full native mono timebase
- **THEN** SampleAnalysis retains its complete accepted record and exact identity/values
- **AND** no opaque runtime ticket is written as durable identity

#### Scenario: Source changes at the same path before saving
- **GIVEN** accepted timing for a loaded source
- **WHEN** project source bytes change at the same path before saving
- **THEN** persistence rejects that accepted export
- **AND** a stale loaded digest alone cannot authorize saving acceptance

### Requirement: Preserve Complete Accepted Identity
The system SHALL preserve full canonical accepted identity, exact binary64 period,
signed independent origin, complete backend/count/time-error evidence and separate
origin and acceptance policy provenance in a versioned record. Runtime tickets,
raw revisions and BPM-only metadata SHALL NOT substitute for that identity.

#### Scenario: Exact accepted evidence roundtrips
- **GIVEN** a source-verified supported COMPLETE accepted record
- **WHEN** it is saved and restored through fresh guarded adoption
- **THEN** its full identity, exact binary64 values and independent provenance are preserved
- **AND** opaque runtime tickets are never persisted as identity

### Requirement: Restore Requires Fresh Native Adoption
The system SHALL treat persisted accepted records as historical evidence until
actual complete source verification and genuine guarded fresh runtime adoption
are acknowledged by the native callback.

Pending, stale, failed or rejected work SHALL NOT become CURRENT or replay saved
legacy timing.

#### Scenario: A matching record is restored
- **GIVEN** saved supported COMPLETE evidence and matching newly loaded source
- **WHEN** a fresh current native request verifies and publishes it
- **THEN** accepted identity and exact period/signed origin are preserved
- **AND** current timing stays unavailable until actual fresh callback acknowledgement

#### Scenario: Fresh adoption becomes stale or fails
- **GIVEN** a saved record admitted under a fresh native source/request/authority
- **WHEN** ownership changes or preparation/publication/adoption fails or remains pending
- **THEN** historical acceptance cannot be treated as CURRENT
- **AND** no BPM-only legacy fallback is promoted to Automatic timing

### Requirement: Verify Fresh Restore Ownership And Complete Timebase
The system SHALL capture fresh source/request/authority ownership at restore
admission, retain historical identity separately, and reject source/mono/rate/full
extent/source-zero mismatches. Heavy JSON/evidence/PCM/hash work SHALL remain off
the audio callback.

#### Scenario: Complete timebase or evidence is incompatible
- **GIVEN** a persisted record with changed source/mono/rate/extent/source zero or unsupported schema/evidence
- **WHEN** restore evaluates it against the actual current loaded source
- **THEN** accepted restoration is rejected
- **AND** other durable project intent remains available

### Requirement: Persist Explicit Timing Intent
The system SHALL preserve explicit Manual, Tap and Legacy intent independently of
accepted evidence, and SHALL give a manual BPM override priority over saved
Automatic acceptance.

Older projects SHALL migrate missing intent to Manual when a manual override is
present and otherwise to Legacy. They SHALL NOT infer accepted identity from old
analysis BPM, raw revision or grid metadata. Unsupported accepted extensions SHALL
be discarded without replacing valid legacy analysis or explicit intent.

#### Scenario: Tap or Manual has priority
- **GIVEN** a saved accepted record alongside a manual BPM override or explicit Manual/Tap intent
- **WHEN** the project is restored
- **THEN** the explicit nonaccepted intent/value is restored
- **AND** the historical record cannot adopt Automatic timing

#### Scenario: An old project contains only legacy metadata
- **GIVEN** an old project containing BPM/grid metadata and no accepted envelope or intent
- **WHEN** the project is loaded
- **THEN** its metadata remains Legacy, or Manual for an existing manual override
- **AND** no accepted identity is synthesized
