## MODIFIED Requirements

### Requirement: Unload Clears Per-Pad Key Lock Intent
The system SHALL clear a pad's track-bound Key Lock override to `null` when its audio is unloaded,
while publishing disabled neutral live state outside the audio callback to remove stale Rust state.

A new source load SHALL clear the previous track's override only on matching successful source
completion/publication. The newly loaded track SHALL then inherit the current global default
through the authoritative resolver. Load admission failure, worker failure or a stale/cancelled
completion SHALL NOT clear the prior track's requested origin or change its confirmed Native
state. A failed subsequent Key Lock publication SHALL retain inherited requested origin and expose
pending/error status separately from the previous confirmed Native state.

#### Scenario: Unload clears pad Key Lock
- **GIVEN** Pad 3 has loaded audio
- **AND** Pad 3 Key Lock is enabled
- **WHEN** the performer unloads audio from Pad 3
- **THEN** Pad 3 has no loaded audio
- **AND** Pad 3's override is `null`
- **AND** the audio engine receives disabled live Key Lock state for Pad 3

#### Scenario: Successful new source inherits the latest global default
- **GIVEN** Pad 3's previous track has explicit OFF
- **AND** a new source is loading while the global default changes from OFF to ON
- **WHEN** matching successful source completion is accepted
- **THEN** Pad 3's override becomes `null` and requests ON from the current default
- **AND** the prior track's explicit OFF is not transferred to the new track

#### Scenario: Admission failure preserves the old track's intent and Native state
- **GIVEN** Pad 3 has loaded audio with explicit ON and confirmed Native ON
- **WHEN** a new source cannot be admitted or its worker fails
- **THEN** Pad 3 retains the old source and explicit ON origin
- **AND** its confirmed Native state is unchanged

#### Scenario: Stale source completion cannot clear a newer override
- **GIVEN** a newer source request or source assignment supersedes a pending Pad 3 load
- **WHEN** the old load completion arrives
- **THEN** the stale result is ignored
- **AND** it does not clear the current override or publish a new Key Lock request
