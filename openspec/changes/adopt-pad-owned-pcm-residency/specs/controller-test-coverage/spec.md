## MODIFIED Requirements

### Requirement: GlobalParametersController No-Op Test Coverage
The test suite SHALL include GlobalParametersController tests that distinguish
explicit KEYLOCK broadcasts from BPM-lock no-op behavior and edge cases.
Every explicit KEYLOCK action SHALL address all currently loaded pads across
banks, even when the global requested value already equals that action. Identical
pending per-pad intent SHALL reuse its own work and deadline rather than enqueue
duplicates. Converged per-pad state MAY avoid redundant preparation only after
current source-bound native confirmation. Tests SHALL preserve coverage of None
and non-finite effective BPM and anchor clearing when BPM lock is disabled.

#### Scenario: Same-value KEYLOCK action broadcasts again
- **GIVEN** global KEYLOCK ON and a later local OFF override on one loaded pad
- **WHEN** explicit global ON is requested again
- **THEN** every loaded target SHALL be visited, including the overridden pad
- **AND** individual failure SHALL NOT abort remaining targets or rewind successful targets

#### Scenario: Identical pending KEYLOCK requests reuse work
- **GIVEN** a current source-bound per-pad KEYLOCK transaction is pending
- **WHEN** another explicit broadcast requests the identical per-pad intent
- **THEN** that target SHALL retain its existing work, attempts and deadline
- **AND** requested equality or enqueue alone SHALL NOT establish effective mode

#### Scenario: set_bpm_lock does nothing when already in state
- **GIVEN** BPM lock is already in the requested state
- **WHEN** set_bpm_lock is called
- **THEN** no change SHALL occur

#### Scenario: set_bpm_lock handles None effective_bpm
- **GIVEN** effective_bpm is None
- **WHEN** set_bpm_lock(True) is called
- **THEN** the method SHALL handle the absent value through the existing guarded path

#### Scenario: set_bpm_lock handles non-finite effective_bpm
- **GIVEN** effective_bpm is non-finite
- **WHEN** set_bpm_lock(True) is called
- **THEN** the method SHALL reject it as an anchor through the existing guarded path

#### Scenario: set_bpm_lock disable clears anchor
- **GIVEN** BPM lock is being disabled
- **WHEN** set_bpm_lock(False) is called
- **THEN** anchor pad and BPM SHALL be cleared
