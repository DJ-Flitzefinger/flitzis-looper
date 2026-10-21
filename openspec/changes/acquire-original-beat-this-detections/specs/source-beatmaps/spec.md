## ADDED Requirements

### Requirement: Selected Beat This Raw acquisition retains actual original authority
The original-fullmix review command SHALL execute the selected verified Beat This
worker using complete mono staged by the existing Native original reader. The
existing map job, cold lane, source/content/request/placement guards, single Raw
review admission and off-thread retirement SHALL remain the only owners.

#### Scenario: Real source analysis completes
- **WHEN** Beat This completes for a current admitted original source
- **THEN** the complete Raw capture SHALL bind actual source and staged-input identity, request and verified model/frontend provenance
- **AND** durable private Raw publication SHALL precede delivery without selecting an accepted map or changing audio state

#### Scenario: Worker is missing or source changes
- **WHEN** installation verification fails or the admitted source is cancelled, replaced or shut down
- **THEN** the attempt SHALL remain unavailable/failed/cancelled without QM fallback or stale delivery
- **AND** all source, staged PCM and lane owners SHALL remain held until actual worker and reader retirement

### Requirement: Backend-aware Raw and map versions preserve complete evidence
Beat This capture SHALL retain exact complete beat timestamps, independent
downbeat timestamps and both binary64 logit arrays. Raw schema2 SHALL use
`source-beatmap-unreviewed-beat-this-raw-v2`; accepted map schema3 SHALL use
`source-beatmap-pwa-v3` and embed that capture. Prior QM Raw and map V1/V2 SHALL
retain their strict contracts. Native staged-input provenance SHALL NOT be
misrepresented as measured internal frontend PCM or model accuracy.

#### Scenario: Arrays exceed an existing bound
- **WHEN** a result exceeds the Raw byte limit or an array's existing count bound, or contains invalid values or provenance
- **THEN** validation SHALL reject the complete result before unbounded derived allocation
- **AND** no cropped, renumbered or substituted successful result SHALL be published

#### Scenario: Explicit labels use a Beat This capture
- **WHEN** the existing bounded interpretation resolves original Beat This observations
- **THEN** their exact source seconds and full Raw arrays SHALL survive the new accepted snapshot
- **AND** estimated anchors, uncertain coverage and unknown meter SHALL remain until independent explicit qualification
- **AND** prior manual selections and corrections SHALL remain unchanged without selection intent

### Requirement: Analysis integration remains modular and outside realtime
Backend capture/validation, worker orchestration and musical interpretation SHALL
remain separate modules behind the existing source/map lifecycle. Callback code
SHALL NOT perform Python/GIL, inference, setup, file I/O, blocking work or heavy
allocation. This acquisition SHALL NOT open productive renderer admission or the
ordinary pad-load analyzer cutover before their separate acceptance.

#### Scenario: Source Map analysis is requested during playback
- **WHEN** an eligible original-fullmix review is admitted
- **THEN** it SHALL use the existing bounded background lane and current source without another audio clock or scheduler
- **AND** previous effective audio and map selection SHALL remain unchanged
