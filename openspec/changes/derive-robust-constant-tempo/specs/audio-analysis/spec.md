## ADDED Requirements

### Requirement: Lossless QM Capture Preserves The Detector Timebase
The system SHALL expose complete QM analysis evidence before its legacy binary32
projection, retaining binary64 detector-frame positions, downbeat raw indices,
the actual integer sample hop, input sample rate and frame count, and complete
requested analysis configuration. The raw capture and legacy output SHALL use the same
tracking pipeline without rerunning analysis or changing the legacy BPM/grid.

#### Scenario: A long source has positions not representable in binary32
- **GIVEN** QM detector positions with binary64 source seconds beyond binary32 precision
- **WHEN** an offline caller reads the complete raw analysis
- **THEN** all original detector positions and downbeat index associations remain available
- **AND** source seconds use the actual sample hop and input sample rate before any binary32 cast
- **AND** the legacy projection retains the existing BPM, beats, downbeats and bars

#### Scenario: Raw capture has no independent musical verification
- **GIVEN** raw QM positions and their input timebase
- **WHEN** the raw evidence is prepared for later constant-tempo assessment
- **THEN** the capture does not fabricate source/PCM hashes, timing-error bounds or verified quarter-note counts
- **AND** independently established identity, origin and count evidence remain caller responsibilities

### Requirement: Constant Tempo Assessment Retains Immutable Evidence
The system SHALL assess constant-tempo candidates outside realtime processing from
complete immutable raw binary64 beat coordinates with source/PCM identity, coordinate
timebase, backend/configuration/raw revision and declared position uncertainty.
It SHALL retain raw-index associations, explicit exclusions, count provenance and
versioned fit/region policy in its diagnostic result.

#### Scenario: A fit excludes a suspected extra event
- **GIVEN** a complete raw result and an explicit count hypothesis excluding an event
- **WHEN** the offline summary evaluates that hypothesis
- **THEN** the original event remains in raw evidence with its exclusion recorded
- **AND** source identity and the original grid origin remain unchanged

### Requirement: Quarter Note Ambiguity Prevents Supported Constant Timing
The system SHALL require explicit quarter-note count interpretation and distinguish
unverified, ambiguous, unsupported and numerically supported candidates. It SHALL
NOT infer verified beat units from a small residual or proximity to an integer BPM.

#### Scenario: Half and double tempo fit the same timestamps
- **GIVEN** distinct viable quarter-note count hypotheses for the same evidence
- **WHEN** each fits with adequate distant support
- **THEN** the result reports ambiguity without selecting supported timing
- **AND** fractional quarter coordinates represent subdivision hypotheses without
  deleting alternating raw events to make the counts integral

#### Scenario: Missing beat count is explicitly repaired
- **GIVEN** a missing raw event and supported counts advancing across its gap
- **WHEN** the multiple-position fit assesses the remaining events
- **THEN** the gap does not silently shorten the musical count
- **AND** the complete supplied count mapping remains available for review

### Requirement: Robust Period Fits Require Distant Support And Honest Uncertainty
The system SHALL evaluate multiple positions and distant regions with bounded work,
explicit coverage and exclusion limits, residuals and declared timing uncertainty.
It SHALL preserve true fractional BPM, reject unsupported constant models and keep
the fitted intercept separate from the independently established source/grid origin.

#### Scenario: A stable middle hides a changing outro
- **GIVEN** a stable middle region but incompatible distant slopes or residual offsets
- **WHEN** a constant-tempo summary is evaluated
- **THEN** it reports unsupported constant timing rather than certifying the middle

#### Scenario: Quantized detections yield a precise numerical fit
- **GIVEN** detector timestamps with a nonzero declared position-error bound
- **WHEN** many observations yield a small fitted residual
- **THEN** diagnostics retain the position-error bound and conditional period uncertainty
- **AND** they do not claim calibrated sub-sample musical confidence

#### Scenario: Alternating deviations exceed the declared timing bound
- **GIVEN** observations inside a robust seed threshold but incompatible with any
  constant affine model within their declared position-error bounds
- **WHEN** constant-period feasibility is checked
- **THEN** the result remains unsupported even if distant mean slopes agree

### Requirement: Signal Refinement Preserves Counts And Publication Boundaries
The system SHALL refine raw timing only with source-matching comparable PCM attacks
and supported quarter-note counts, retaining original/refined coordinates and method
provenance. Unsupported signals SHALL remain uncertain. Offline assessment SHALL NOT
replace manual/TAP intent or publish live timing before the separate adoption stage.

#### Scenario: Exact reference validates the complete refinement stage
- **GIVEN** the 48-kHz exact reference with 1200 verified pulses over 0..599.5 seconds
- **WHEN** G2 refinement is evaluated against all pulse positions
- **THEN** the supported constant period has at most one loaded-frame slope error over
  that measured span
- **AND** the 600-second extrapolation is reported separately
- **AND** this numerical result does not certify musical-model or audible-SYNC acceptance
