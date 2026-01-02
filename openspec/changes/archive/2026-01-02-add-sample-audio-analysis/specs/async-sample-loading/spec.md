## ADDED Requirements
### Requirement: Sample Loading Includes Audio Analysis Stage
The async sample loading pipeline SHALL include a dedicated analysis stage that detects BPM, key, and beat grid for the sample being loaded.

The analysis stage SHALL run off the audio thread (on the background worker) and SHALL complete before the load operation is considered successful.

#### Scenario: Load progress reports an analyzing stage
- **WHEN** the worker begins analyzing a loaded sample
- **THEN** it sends one or more progress updates whose `stage` indicates analysis (e.g., `"Analyzing (bpm/key/beat grid)"`)

#### Scenario: Analysis happens after resampling
- **GIVEN** a sample load requires resampling
- **WHEN** the worker processes the load pipeline
- **THEN** the worker completes the resampling stage
- **AND** the worker begins the analysis stage after resampling

#### Scenario: Load success includes analysis results
- **WHEN** the worker finishes loading a sample successfully
- **THEN** the corresponding completion event includes the sample duration
- **AND** the completion event includes analysis results (BPM, key, beat grid) for that sample

### Requirement: Analysis Progress Uses Weighted Best-effort Percent
When the analysis stage cannot provide fine-grained progress updates, the system SHALL map analysis to a weighted portion of total progress and SHALL emit a progress update that jumps to completion once analysis finishes.

#### Scenario: Analysis progress completes in a single jump
- **WHEN** analysis starts and the system cannot compute fine-grained analysis progress
- **THEN** the system emits a progress update at the start of analysis
- **AND** the system emits a progress update with `percent == 1.0` once analysis finishes
