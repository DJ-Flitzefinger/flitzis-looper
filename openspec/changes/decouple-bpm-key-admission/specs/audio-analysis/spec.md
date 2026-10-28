## ADDED Requirements

### Requirement: Primary BPM survives unavailable optional key analysis
Configured primary-neural analysis SHALL admit complete-source BPM independently
of optional key work at every pre-read and execution gate. It SHALL retain the
512MiB preparation limit, full original observation extent and current source,
request, cancellation, staged-PCM and publication ownership guards.

#### Scenario: BPM fits but concurrent key work does not
- **GIVEN** complete-source BPM fits and retained PCM plus wire and key workspace would exceed512MiB
- **WHEN** automatic or explicit same-source analysis is admitted
- **THEN** the system SHALL run complete-source BPM without optional key allocation
- **AND** the result SHALL keep its required key display field and report skipped_budget explicitly
- **AND** the valid BPM result SHALL remain publishable through the existing source/request guards

#### Scenario: Mandatory preparation cannot fit
- **WHEN** mandatory complete-source BPM preparation exceeds512MiB or its geometry overflows
- **THEN** analysis SHALL refuse before materializing complete PCM
- **AND** it SHALL NOT crop the source or increase the limit

#### Scenario: Optional key work fails or was not requested
- **WHEN** optional conversion or model work fails, or tempo-only analysis did not request it
- **THEN** the result SHALL report failed with detail or not_requested respectively
- **AND** it SHALL preserve an independently obtained BPM result
- **AND** cancellation or stale source identity SHALL still refuse the whole stale publication

### Requirement: Key completion owns key metadata changes
Key analysis admission SHALL capture the source/request/analysis epoch without
clearing correction. Only matching ready key completion MAY publish a detected
source key version and clear its captured preexisting correction. A newer correction
SHALL remain authoritative. Optional key metadata capacity SHALL NOT veto BPM.

#### Scenario: Fresh load skips key work
- **WHEN** a current fresh source completes BPM with skipped_budget key status
- **THEN** both initial-load routes SHALL preserve valid BPM and display unknown key
- **AND** they SHALL NOT publish a fabricated detected unknown SourceKeyVersion

#### Scenario: Same-source reanalysis skips or fails key work
- **GIVEN** valid detected key and possibly manual correction with numeric shifts
- **WHEN** new BPM completes while key work is skipped, failed or not requested
- **THEN** the system SHALL retain the prior detected key, correction and shifts
- **AND** it SHALL settle only the matching key request and expose the component detail

#### Scenario: Newer correction or source wins
- **WHEN** correction changes after admission or the content/source/request is replaced or cancelled
- **THEN** late key completion SHALL preserve newer intent and reject stale source publication
- **AND** independent current BPM SHALL retain its own existing publication guards

#### Scenario: Key metadata epochs are exhausted
- **WHEN** optional key metadata preparation cannot reserve its required epoch
- **THEN** independent BPM admission SHALL continue if its own source and memory guards pass
- **AND** key state SHALL remain unchanged with an understandable component error
