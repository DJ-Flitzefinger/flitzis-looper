# audio-analysis Delta Specification

## MODIFIED Requirements

### Requirement: Analyze Audio For BPM, Key, And Beat Grid
The system SHALL analyze a loaded audio sample to determine its BPM, musical key, and beat grid.

The system SHALL decode the audio file once, convert to mono, and resample to 44100 Hz (if the source rate differs). The resulting mono buffer SHALL be shared between the BPM detection pipeline and the key detection pipeline — no pipeline SHALL re-decode or re-resample the audio.

The system SHALL execute the BPM detection pipeline (qm-dsp: DetectionFunction → TempoTrackV2 → DownBeat) and the key detection pipeline (KeyNet CNN via ONNX) **concurrently** on separate threads after shared preprocessing completes. The analysis result SHALL be assembled only after both pipelines finish.

The system SHALL represent the detected key as a musical-notation string (e.g., `"C#m"`) suitable for display in a professional audio application. When key detection fails or the audio is insufficient, the system SHALL use `"unknown"` as the key value.

#### Scenario: Analysis produces BPM, key, and beat grid
- **GIVEN** a pad has a loaded audio file
- **WHEN** the analysis workflow is executed for that pad
- **THEN** the system decodes the audio once and converts to mono
- **AND** the system runs BPM detection and key detection concurrently on separate threads
- **AND** the system produces a BPM value (float) from the BPM pipeline
- **AND** the system produces a musical key value in musical notation (e.g., `"C#m"`) from the key detection pipeline
- **AND** the system produces a beat grid containing beat times from the BPM pipeline
- **AND** the system produces downbeat times when they can be determined
- **AND** the total analysis time is bounded by the slower pipeline, not the sum of both

#### Scenario: Analysis failure is reported
- **GIVEN** a pad has a loaded audio file
- **WHEN** analysis fails due to an unsupported or invalid audio signal
- **THEN** the system reports an error for that pad
- **AND** the previously stored analysis result (if any) remains unchanged unless explicitly cleared

#### Scenario: Key detection failure does not block BPM results
- **GIVEN** a pad has a loaded audio file
- **WHEN** the BPM pipeline succeeds but the key detection pipeline fails
- **THEN** the system returns the BPM and beat grid from the BPM pipeline
- **AND** the system uses `"unknown"` as the key value
- **AND** the error is logged for diagnostics
