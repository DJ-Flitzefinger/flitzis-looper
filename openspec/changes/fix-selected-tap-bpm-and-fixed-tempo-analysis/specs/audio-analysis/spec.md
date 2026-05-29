## MODIFIED Requirements

### Requirement: Analyze Audio For BPM, Key, And Beat Grid
The system SHALL analyze a loaded audio sample to determine its BPM, musical key, and beat grid.

The system SHALL use `stratum_dsp` for primary analysis. After primary analysis, the system SHALL
refine BPM for fixed-tempo material when decoded audio contains enough strong transient starts that
fit one stable constant-tempo grid near the primary BPM. The fixed-tempo refinement SHALL publish the
fitted BPM rounded to 0.01 BPM and SHALL leave the primary analysis BPM unchanged when the transient
grid is missing, sparse, unstable, or inconsistent with the primary tempo.

The fixed-tempo refinement SHALL run only in the non-realtime analysis worker and SHALL NOT add disk
I/O, Python/GIL access, logging, blocking work, heavy allocation, neural inference, or unbounded work
to the Rust audio callback.

#### Scenario: Exact fixed metronome refines to exact BPM
- **GIVEN** a decoded audio sample contains a stable 120 BPM metronome for the analysis duration
- **WHEN** automatic analysis completes
- **THEN** the published BPM is approximately 120.00

#### Scenario: Unstable transient grid keeps primary BPM
- **GIVEN** primary analysis returns a BPM
- **AND** decoded strong transients do not fit a stable constant-tempo grid near that BPM
- **WHEN** automatic analysis completes
- **THEN** the published BPM remains the primary analysis BPM
