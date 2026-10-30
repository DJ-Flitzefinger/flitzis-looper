## ADDED Requirements

### Requirement: Accurate period before musical beat-level preference

The system SHALL retain a physically supported canonical tempo, and SHALL NOT
suppress an otherwise supported scalar merely because a coherently nested direct
half/double unit remains plausible. If the canonical candidate is unsupported,
the system SHALL evaluate only its two already fitted harmonic alternatives in
the declared bounded order under unchanged precision and complete rival checks.

#### Scenario: Preferred count fails while its half is precise
- **WHEN** the canonical count fails a physical check and a declared half candidate passes its own checks and rival coverage
- **THEN** the precise half candidate SHALL be available with unit uncertainty
- **AND** no integer rounding, raw-event rewrite or reference-tempo seed SHALL occur

#### Scenario: A genuinely incompatible witness remains
- **WHEN** a prospective candidate has a nonunit, shifted-phase, local-conflict or unresolved-search veto
- **THEN** that candidate SHALL remain unsupported regardless of its own precision or another candidate's unit relationship

#### Scenario: A fixed precision failure cannot benefit from more rivals
- **WHEN** a fallback candidate already fails its own eligibility or precision
- **THEN** the system SHALL record that refusal without spending additional rival-fit fuel for it

### Requirement: Bounded and source-bound period selection evidence

Schema9 SHALL retain canonical seed provenance, performed candidate trials,
their own precision and limit states, and the selected mapping with bounded
critical witnesses. Existing source, raw, key, cancellation and publication
ownership SHALL remain unchanged. Stored older evidence SHALL not be silently
reinterpreted under the new selection semantics.

#### Scenario: A range or saved copy consumes the new result
- **WHEN** schema9 passes through existing joint6, range7, Copy or restore
- **THEN** actual version and source mathematics SHALL replay without DSP or runtime lookup
- **AND** enabled range bounds SHALL remain captured and enforced

### Requirement: Primary automatic publication excludes learned unit arbitration

Fresh automatic analysis SHALL not run the learned nominal classifier, its
readiness scan or its additional frontend to settle a musical half/double choice.
Existing manual /2, x2, Tap and optional range controls SHALL remain available.

#### Scenario: A verified nominal profile is already installed
- **WHEN** a fresh primary automatic request is analyzed
- **THEN** that profile SHALL cause zero learned nominal-stage calls
- **AND** no second or delayed correction publication SHALL be introduced

#### Scenario: Existing packed schema8 evidence is restored
- **WHEN** an actual stored schema8 result is copied or restored
- **THEN** its recorded historical source/profile/math SHALL retain its original strict semantics
- **AND** no automatic runtime work or new kernel9 interpretation SHALL occur
