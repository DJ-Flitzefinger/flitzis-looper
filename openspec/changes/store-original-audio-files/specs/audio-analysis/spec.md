# audio-analysis Specification

## Purpose
To analyze loaded audio samples for BPM, key, and beat grid information, which enables tempo-sync features and musical key display.

## MODIFIED Requirements

### Requirement: Analysis Can Be Triggered Automatically And Manually
The system SHALL run analysis automatically as part of the sample loading workflow, except when restoring a project with valid stored analysis results.

Manual analysis SHALL enqueue an analysis-only background task and SHALL NOT re-run decoding, resampling, channel mapping, or sample publication.

#### Scenario: Automatic analysis runs on new load
- **WHEN** a sample load completes successfully for a pad
- **AND** no valid analysis results are stored in `ProjectState.sample_analysis[pad_id]`
- **THEN** the system runs analysis for that pad before considering the load operation complete

#### Scenario: Automatic analysis is skipped on restoration
- **WHEN** a sample is restored from project state
- **AND** `ProjectState.sample_analysis[pad_id]` contains valid results
- **THEN** the system does not run automatic analysis for that pad
- **AND** the load operation is considered complete after decoding and publication

#### Scenario: Manual analysis re-runs detection
- **GIVEN** a pad has a loaded audio file
- **AND** analysis results already exist for that pad
- **AND** the pad is not currently loading
- **WHEN** the user triggers "Analyze audio"
- **THEN** the system re-runs analysis and updates stored results for that pad

#### Scenario: Manual analysis is blocked while loading
- **GIVEN** a pad is currently loading
- **WHEN** the user attempts to trigger "Analyze audio" for that pad
- **THEN** the system blocks the request
- **AND** no analysis-only job is started for that pad
