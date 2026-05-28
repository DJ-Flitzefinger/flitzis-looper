## ADDED Requirements

### Requirement: Unload Clears Per-Pad Key Lock Intent
The system SHALL reset a pad's per-pad Key Lock intent to disabled when audio is unloaded from that pad.

Loading into an empty pad SHALL also clear any stale per-pad Key Lock intent before the new load is scheduled. The system SHALL publish the disabled per-pad Key Lock live default outside the audio callback so a later track loaded into the same pad cannot inherit stale Key Lock state.

#### Scenario: Unload clears pad Key Lock
- **GIVEN** Pad 3 has loaded audio
- **AND** Pad 3 Key Lock is enabled
- **WHEN** the performer unloads audio from Pad 3
- **THEN** Pad 3 has no loaded audio
- **AND** Pad 3's per-pad Key Lock value is disabled
- **AND** the audio engine receives disabled live Key Lock state for Pad 3

#### Scenario: Loading into empty pad clears stale Key Lock
- **GIVEN** Pad 3 has no loaded audio
- **AND** stale project data has Pad 3 Key Lock enabled
- **WHEN** the performer loads new audio into Pad 3
- **THEN** Pad 3's per-pad Key Lock value is disabled before the new load is scheduled
- **AND** the audio engine receives disabled live Key Lock state for Pad 3
