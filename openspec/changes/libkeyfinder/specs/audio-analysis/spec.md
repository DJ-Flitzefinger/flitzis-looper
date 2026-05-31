## MODIFIED Requirements

### Requirement: Analyze Audio For BPM, Key, And Beat Grid
The system SHALL analyze a loaded audio sample to determine its BPM, musical key, and beat grid.

The system SHALL use a libkeyfinder-based Rust port for key detection, implementing chromagram-based analysis with Krumhansl-Schmuckler tone profiles and cosine similarity classification.

The system SHALL represent the detected key as a musical-notation string (e.g., `"C#m"`) suitable for display in a professional audio application.

The system SHALL return a silence indicator when the audio signal has insufficient energy for reliable key detection.

#### Scenario: Analysis produces BPM, key, and beat grid
- **GIVEN** a pad has a loaded audio file
- **WHEN** the analysis workflow is executed for that pad
- **THEN** the system produces a BPM value (float)
- **AND** the system produces a musical key value in musical notation (e.g., `"C#m"`)
- **AND** the system produces a beat grid containing beat times
- **AND** the system produces downbeat times when they can be determined

#### Scenario: Analysis failure is reported
- **GIVEN** a pad has a loaded audio file
- **WHEN** analysis fails due to an unsupported or invalid audio signal
- **THEN** the system reports an error for that pad
- **AND** the previously stored analysis result (if any) remains unchanged unless explicitly cleared

#### Scenario: Silent audio produces silence key
- **GIVEN** a pad has a loaded audio file with negligible audio energy
- **WHEN** the analysis workflow is executed for that pad
- **THEN** the system produces a silence indicator for the key
- **AND** the key value is represented as `"?"` in the output
