# audio-analysis Delta Specification

## MODIFIED Requirements

### Requirement: Analyze Audio For BPM, Key, And Beat Grid
The system SHALL analyze a loaded audio sample to determine its BPM, musical key, and beat grid.

The system SHALL use the qm-dsp tempo tracking pipeline (DetectionFunction + TempoTrackV2 + DownBeat) for BPM and beat grid analysis. The system SHALL use `AnalysisConfig::default()` as the configuration defaults.

The system SHALL represent the detected key as a musical-notation string (e.g., `"C#m"`) suitable for display in a professional audio application.

The system SHALL produce a beat grid containing beat times, downbeat times, and bar start times. Downbeat and bar detection SHALL use the spectral difference method from the qm-dsp DownBeat module.

#### Scenario: Analysis produces BPM, key, and beat grid
- **GIVEN** a pad has a loaded audio file
- **WHEN** the analysis workflow is executed for that pad
- **THEN** the system produces a BPM value (float)
- **AND** the system produces a musical key value in musical notation (e.g., `"C#m"`)
- **AND** the system produces a beat grid containing beat times
- **AND** the system produces downbeat times when they can be determined
- **AND** the system produces bar start times when downbeats are detected

#### Scenario: Analysis failure is reported
- **GIVEN** a pad has a loaded audio file
- **WHEN** analysis fails due to an unsupported or invalid audio signal
- **THEN** the system reports an error for that pad
- **AND** the previously stored analysis result (if any) remains unchanged unless explicitly cleared

## REMOVED Requirements

### Requirement: stratum-dsp Backend
**Reason**: Replaced by qm-dsp tempo tracking pipeline (DetectionFunction + TempoTrackV2 + DownBeat) which provides superior beat tracking via Viterbi HMM, explicit downbeat/bar detection, and gradual tempo change support.
**Migration**: The `stratum-dsp` crate is removed from `Cargo.toml`. The `BeatGrid` struct is now a local Rust type with identical fields (`beats`, `downbeats`, `bars`). The `analyze_sample()` API surface is unchanged.
