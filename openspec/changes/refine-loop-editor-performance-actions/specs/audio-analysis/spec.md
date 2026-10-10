## MODIFIED Requirements

### Requirement: Analysis Can Be Triggered Automatically And Manually
The system SHALL run analysis automatically as part of the existing sample-load
workflow and SHALL require a warning confirmation before the performer's manual
Analyze Audio action enqueues analysis for already-loaded content (E11-16).

Manual analysis SHALL remain analysis-only background work without repeated
decoding, resampling, channel mapping or sample publication. Its warning SHALL
capture pad/current content identity and controller acceptance SHALL revalidate
that content and current eligibility. Selection changes SHALL NOT retarget the
request; Cancel/Escape/stale acceptance SHALL enqueue nothing. Loading pads SHALL
remain ineligible. Valid restored analysis SHALL retain the existing restore path
without automatic reanalysis.

#### Scenario: Confirmed analysis uses captured content
- **GIVEN** loaded eligible pad A has prior analysis results
- **WHEN** the performer activates Analyze Audio
- **THEN** a warning appears and no analysis task starts
- **WHEN** the performer confirms while captured A content remains eligible
- **THEN** one analysis-only task is enqueued for that content
- **AND** the current manual/grid/loop preservation policy remains in force

#### Scenario: Cancel replacement or loading prevents manual analysis
- **GIVEN** a manual-analysis warning captures pad A content X
- **WHEN** the warning is cancelled, X changes, or A becomes ineligible/loading
- **THEN** no task is admitted from that warning
- **AND** another selected pad cannot become its target

#### Scenario: Automatic and restored analysis retain existing behavior
- **WHEN** a normal load completes or valid saved analysis is restored
- **THEN** the existing automatic load analysis or saved-result restore path applies
- **AND** no performer confirmation is invented for internal load/restore work
