## MODIFIED Requirements

### Requirement: Analyze Audio For BPM, Key, And Beat Grid
The system SHALL analyze a loaded audio sample to determine its BPM, musical key, and beat grid.

The system SHALL use `stratum_dsp` for primary analysis. After primary analysis, the system SHALL
refine BPM for fixed-tempo material when low-confidence analyzer candidates show stronger
octave-family consensus than the selected primary BPM, when decoded audio contains enough strong
transient starts that fit one stable constant-tempo grid near the chosen candidate-family BPM, or
when a full-track spectral autocorrelation check supports a common-ratio performer tempo. Supported
common-ratio tempos include 3/4 of a stronger subdivision candidate and strongly dominant 4/5
spectral performer-tempo targets. The fixed-tempo refinement SHALL publish the chosen BPM with at
least 0.001 BPM precision for timing and grid metadata, SHALL NOT snap near-integer BPM values to
integer BPM solely for display readability, and SHALL leave the primary analysis BPM unchanged when
candidate-family consensus is inconclusive and the transient or spectral checks are missing,
sparse, unstable, or inconsistent with the chosen tempo.

The candidate-family and fixed-tempo refinements SHALL run only in the non-realtime analysis worker
and SHALL NOT add disk I/O, Python/GIL access, logging, blocking work, heavy allocation, neural
inference, or unbounded work to the Rust audio callback.

#### Scenario: Exact fixed metronome refines to exact BPM
- **GIVEN** a decoded audio sample contains a stable 120 BPM metronome for the analysis duration
- **WHEN** automatic analysis completes
- **THEN** the published BPM is approximately 120.00

#### Scenario: Low-confidence half-time candidate refines to performer tempo
- **GIVEN** primary analysis selects a low-confidence half-time or double-time BPM
- **AND** the analyzer candidate list contains stronger octave-family support for the performer BPM
- **WHEN** automatic analysis completes
- **THEN** the published BPM uses the supported performer BPM with at least 0.001 BPM precision

#### Scenario: Low-confidence subdivision candidate is outvoted
- **GIVEN** primary analysis selects a low-confidence subdivision BPM
- **AND** multiple octave-related candidates support a different fixed performer BPM
- **WHEN** automatic analysis completes
- **THEN** the published BPM uses the stronger octave-family BPM with at least 0.001 BPM precision

#### Scenario: Common-ratio subdivision candidate refines to performer tempo
- **GIVEN** primary analysis selects a low-confidence subdivision BPM near 4/3 of the performer tempo
- **AND** the analyzer candidate list contains support near the performer tempo or its octave family
- **AND** full-track spectral autocorrelation remains stable near the performer tempo
- **WHEN** automatic analysis completes
- **THEN** the published BPM uses the performer tempo with at least 0.001 BPM precision

#### Scenario: Strong 4/5 common-ratio target refines to performer tempo
- **GIVEN** primary analysis selects a fixed-tempo subdivision BPM near 5/4 of the performer tempo
- **AND** the full-track spectral autocorrelation score near 4/5 of the selected BPM is strongly
  dominant over the score near the selected BPM
- **WHEN** automatic analysis completes
- **THEN** the published BPM uses the 4/5 performer tempo with at least 0.001 BPM precision

#### Scenario: Near-integer spectral tempo remains fractional
- **GIVEN** primary analysis selects or refines to a fixed-tempo BPM slightly below or above an
  integer BPM
- **AND** full-track spectral autocorrelation supports the fractional near-integer BPM
- **WHEN** automatic analysis completes
- **THEN** the published BPM preserves the fractional near-integer BPM instead of snapping it to the
  integer BPM

#### Scenario: Unstable transient grid keeps primary BPM
- **GIVEN** primary analysis returns a BPM
- **AND** analyzer candidate-family consensus is inconclusive
- **AND** decoded strong transients and spectral autocorrelation do not fit a stable constant-tempo grid near that BPM
- **WHEN** automatic analysis completes
- **THEN** the published BPM remains the primary analysis BPM
