## ADDED Requirements

### Requirement: Offline Beatmaps Have Explicit Identity And Time Units
The system SHALL represent each accepted offline beatmap as an immutable versioned record tied
to the decoded source identity and containing explicit source-time and musical-beat units.

The record SHALL include schema/map revisions, detector/config provenance, model checksum when
applicable, preprocessing identity, interpolation policy, coverage/quality status and manual
edit lineage. Source anchors SHALL use finite f64 seconds; loaded source frames SHALL be derived
for the actual loaded rate. Unmodified analysis SHALL remain distinguishable from corrections.
Sample-domain accepted edits SHALL retain the selected sample index and source rate as provenance.
Model timestamp resolution and coordinate representation SHALL NOT be equated with musical
detection accuracy. Pitch/transposition intent SHALL NOT change source-map identity or coordinates.

#### Scenario: Output rate changes without changing the source
- **GIVEN** an accepted map tied to the same decoded source
- **WHEN** that source is loaded at a different supported output rate
- **THEN** the same seconds anchors are converted outside the callback to the new loaded frames
- **AND** the analysis-hop domain is not interpreted as loaded frames or seconds

#### Scenario: Old analysis finishes after source replacement
- **GIVEN** analysis was requested for one source version and the pad source has changed
- **WHEN** that result completes
- **THEN** it is not accepted as the replacement source's map

#### Scenario: Sample placement does not certify a model estimate
- **GIVEN** a raw Beat This estimate on its model time grid
- **WHEN** it is represented at a precise source sample position
- **THEN** its automatic evidence and uncertainty remain distinguishable from a reviewed anchor
- **AND** a future semitone change does not reanalyze or move that source anchor

### Requirement: One Checked Mapping Defines Source And Beat Coordinates
The system SHALL provide one Rust control-side map evaluator with a checked monotone inverse
for accepted source and beat anchors.

Both coordinates SHALL be strictly increasing, finite and bounded in count with validated
spacing. Piecewise affine interpolation SHALL preserve accepted anchors exactly within stated
floating-point precision. Missing-beat regions or unknown beat counts SHALL be explicit coverage
gaps. Extrapolated positions SHALL remain distinguishable from trusted coverage. Beat units,
downbeat labels and meter certainty SHALL not be inferred solely from array position.

#### Scenario: A variable-tempo segment is evaluated in either direction
- **GIVEN** adjacent anchors at source seconds 1.0 and 1.6 with beat indices 2 and 3
- **WHEN** the evaluator maps beat 2.5 to source and back
- **THEN** it returns source seconds 1.3 and beat 2.5 within declared precision

#### Scenario: Invalid or incomplete detection is not promoted to a trusted map
- **GIVEN** duplicate/nonfinite anchors or a gap with unknown beat count
- **WHEN** a map is prepared
- **THEN** invalid coordinates are rejected and uncertain coverage is marked explicitly
- **AND** the last accepted state is retained without fabricating trusted consecutive beats

### Requirement: Legacy Scalar Timing Remains A Separate Valid Mode
The system SHALL preserve existing scalar BPM, signed-origin and source-time loop semantics
when restoring projects without an explicitly accepted variable beatmap.

Existing detected beat arrays SHALL NOT automatically replace manual scalar grids. The new
foundation SHALL NOT change live launch scheduling, master transport, current audio addressing,
or prepared-stem playback. Stems SHALL refer to their parent source map when a map is stored.

#### Scenario: A legacy project has a manual BPM and negative grid origin
- **WHEN** the project is restored with the beatmap foundation available
- **THEN** its existing scalar grid and source-time markers remain unchanged
- **AND** no variable mapping is silently activated

### Requirement: Beatmap Preparation Stays Outside Realtime Audio
The system SHALL perform beatmap creation, validation, inference, persistence and control-side
evaluation outside the realtime callback.

This foundation SHALL NOT enqueue full beatmap vectors into the callback or introduce callback
GIL access, disk I/O, locks, allocation, inference or unbounded traversal. Invalid, cancelled and
stale work SHALL leave the last accepted map and live audio state intact.

#### Scenario: A map is rejected while audio is playing
- **WHEN** background validation rejects a candidate map
- **THEN** existing audio playback and master progression continue unchanged
- **AND** rejection is reported through the non-realtime control path
