## ADDED Requirements

### Requirement: Only Usable Automatic BPM May Become Pad Tempo

The automatic BPM analysis SHALL preserve the result and order of existing
successful estimators. Reusing existing estimator evidence SHALL preserve
the numeric observations required for strict replay. If the existing selection
ends without an admissible BPM, the analysis SHALL try the conventional
QM beat-interval estimate on the verified current source. It SHALL publish
a scalar only if the value is finite, positive and within the captured range
when enabled. A failed attempt SHALL leave the source and independent key
result usable without setting a BPM.

#### Scenario: Existing approximate evidence passes strict replay

- **GIVEN** the existing approximate detector supplies an admissible source-derived period
- **WHEN** its result crosses the native-to-App boundary
- **THEN** the original numeric evidence SHALL reach the App without an intermediate lossy JSON parse
- **AND** a valid result SHALL appear as **Approximate BPM**

#### Scenario: Conventional tempo is the final usable estimate

- **GIVEN** the current source is verified and earlier attempts yielded no admissible BPM
- **WHEN** the conventional detector supplies valid beat-interval evidence
- **THEN** the pad SHALL show the resulting scalar with the **Approximate BPM** warning
- **AND** its details SHALL identify the conventional method
- **AND** the result SHALL leave beat and downbeat phase unspecified

#### Scenario: Every automatic attempt fails

- **GIVEN** the source is silent or the available estimates fail validation or the captured range
- **WHEN** the analysis ends
- **THEN** the system SHALL leave automatic tempo unavailable and retain the failure diagnostic
- **AND** the failure SHALL preserve any valid same-source tempo and independent key result
