## ADDED Requirements

### Requirement: Complete automatic analysis has bounded shared storage
The system SHALL admit complete automatic and explicit analysis using checked
simultaneous allocation bounds under the existing 512 MiB analysis limit. It MUST
include immutable source PCM, separately pinned finite views, channel/rate conversion
overlap and capacities, shared mono, FFT and CQT workspaces, octave resampling and
compact responses, BPM tracking and downbeat storage. Model and ORT storage MUST
remain separately identified and measured. Admission MUST precede large allocation
and SHALL reject unsupported, overflowing or genuinely over-budget geometry visibly.

The existing concurrent complete BPM and key pipelines SHALL consume one shared
mono signal after one existing rate conversion. The preparation SHALL preserve
sample zero, complete tail, existing channel arithmetic and numerical detector
semantics. CQT SHALL process bounded FFT frames without whole-track STFT or padded
signal copies and SHALL retain the existing complete output and accumulation order.

#### Scenario: Six-minute stereo import completes its existing analysis
- **GIVEN** a valid approximately 376-second stereo source loaded at 44100 or 48000 Hz
- **WHEN** its complete analysis storage fits the unchanged limit
- **THEN** automatic import SHALL produce BPM and complete beat/downbeat results without a preparation-budget error
- **AND** the existing key frontend SHALL retain its result or truthful unknown outcome
- **AND** the result SHALL accompany only the source's own successful acknowledgement

#### Scenario: Allocation removal preserves complete detector input and output
- **WHEN** mono/stereo signals with silence, nonzero tail and rate conversion are analyzed
- **THEN** shared PCM and CQT values SHALL match the existing independent full-buffer oracle
- **AND** BPM, beats, downbeats and key outcomes SHALL retain their existing semantics

#### Scenario: Genuine resource denial does not mutate authority
- **WHEN** source, retained-view or FFT/workspace geometry overflows or exceeds admission
- **THEN** the request SHALL reject before the corresponding large allocations
- **AND** explicit denial SHALL preserve prior request and epoch authority
- **AND** automatic denial SHALL remain visible with the matching admitted source outcome

#### Scenario: Cancellation and stale completion keep the existing import fence
- **WHEN** analysis is cancelled or its source/timing request becomes stale
- **THEN** its analysis result or error SHALL NOT publish as current metadata
- **AND** existing own-ACK, rollback and final-reader cleanup guards SHALL remain effective
