## ADDED Requirements
### Requirement: Pad Loading Progress Indicator
When a pad’s sample slot is being loaded asynchronously (see `async-sample-loading`), the system **SHALL** show a loading progress indicator directly on that pad.

The indicator **SHALL** include:
- The current loader `stage` text (main task with optional sub-task), e.g. `Loading (decoding)`.
- The current total progress percentage rendered as an integer percent string (e.g. `33 %`).
- A background progress bar rendered as a filled rectangle whose width is proportional to progress.

The system **SHALL** show the stage + percentage in the selected-pad sidebar as well when the selected pad is loading.

The progress bar color **SHALL** be a slightly darker shade than the pad’s normal background color.

#### Scenario: Loading pad shows stage and percentage text
- **WHEN** a pad is loading and the UI has received a `LoaderEvent::Progress` for that pad
- **THEN** the pad label includes the current `stage` and a percentage derived from `percent`

#### Scenario: Loading pad shows a progress bar
- **WHEN** a pad is loading and has `percent == 0.33`
- **THEN** the pad shows a filled rectangle background whose width is approximately 33% of the pad width

#### Scenario: Selected-pad sidebar shows stage and percentage
- **WHEN** the selected pad is loading and the UI has received a `LoaderEvent::Progress` for that pad
- **THEN** the sidebar shows the current `stage` and a percentage derived from `percent`

#### Scenario: Progress indicator clears on completion
- **WHEN** a pad finishes loading successfully
- **THEN** the loading progress indicator is no longer shown on that pad
- **AND** the pad returns to its normal background rendering