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

The supported additive envelope SHALL use schema version 1 and encoding `source-beatmap-pwa-v1`.
Its strict record SHALL separate stable source identity, immutable raw evidence and accepted
anchors/coverage/correction lineage. Authoritative binary64 coordinates and evidence SHALL retain
their exact hexadecimal bits. Raw and map revisions SHALL be complete 64-digit lowercase SHA-256
identities over their canonical complete payloads, excluding only the respective self-revision
field. The map revision SHALL bind source, raw provenance and complete accepted state.

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

#### Scenario: Exact record values survive serialization
- **GIVEN** a supported record containing fractional binary64 anchors and a complete map revision
- **WHEN** that record is encoded and decoded
- **THEN** every authoritative coordinate retains its original bits and complete revision
- **AND** serialization does not promote declared evidence to verified source or musical acceptance

### Requirement: Map Identity And Actual Source Authority Remain Separate
The system SHALL reuse the existing canonical cold-store decoder descriptor and complete
`decoder_identity` for stable map identity, distinguishing original decoded geometry and policy
from playback conversion, loaded rate and current pad/content/request/lifetime authority.

The system SHALL reuse the existing canonical descriptor/hash implementation. A parsed JSON
descriptor and consistent digest SHALL establish data consistency only. Actual source-bound
acceptance or restore SHALL additionally require verified decoder metadata and readable immutable
producer custody under the current source/content/request/lifetime binding. A source path,
duration, saved runtime token, digest string or historical ticket SHALL NOT grant that authority.
Persisted map evidence SHALL remain historical until the required current verification succeeds.

Map geometry acceptance SHALL NOT grant current audible timing, a callback acknowledgement,
source preparation permission or a live source permit. Loaded-frame projection SHALL be derived
from unchanged source seconds at the actual loaded rate, without changing stable map identity.

#### Scenario: A descriptor is internally consistent but unverified
- **GIVEN** JSON containing a valid decoder descriptor and recomputed matching digest
- **WHEN** the control-side map parser accepts its structure and numeric geometry
- **THEN** the result is not source-bound acceptance or current audio authority
- **AND** actual verified producer custody and current source binding are still required

#### Scenario: Saved map belongs to different decoded material
- **GIVEN** a saved map and a newly loaded source with incompatible decoder identity or metadata
- **WHEN** restore checks current actual source custody
- **THEN** it rejects source-bound restoration without replacing the previous valid state
- **AND** a reused path or matching duration does not bypass the mismatch

### Requirement: Immutable Map Revisions Preserve Independent Content Intent
The system SHALL share only verified immutable map/raw versions while preserving each movable
content instance's independent selected revision and correction intent.

Position, beat-count, downbeat/meter, whole-map translation and optional refinement SHALL remain
distinct lineage operations. A correction SHALL create a new validated snapshot and retain raw
evidence. Whole-map translation SHALL preserve intervals within binary64 precision; its lineage
SHALL NOT act as a second transform over the authoritative stored coordinates. Sample edits
SHALL retain their chosen integer index, rate and evidence/author provenance.

Copy SHALL retain the musical revision selection in independent stopped content and share only
immutable backing; later edits SHALL NOT automatically fan out to equal-source copies. Move/Swap
SHALL carry the content's selection. Selected stems SHALL refer to their parent source map.
Same-source reanalysis SHALL preserve explicit corrections until explicit replacement intent;
source replacement SHALL NOT inherit corrections solely from path or duration equality.

#### Scenario: One copied content receives a local correction
- **GIVEN** two content instances sharing one immutable source/map version
- **WHEN** one instance accepts a new local map revision
- **THEN** the other retains its selected revision and independent intent
- **AND** raw evidence and the old immutable revision remain unchanged

#### Scenario: Analysis refresh completes after a manual correction
- **GIVEN** an accepted correction for the same verified source
- **WHEN** fresh raw analysis completes without explicit replacement intent
- **THEN** raw analysis remains distinguishable from the preserved correction
- **AND** the refresh does not silently select an uncorrected replacement map

### Requirement: One Checked Mapping Defines Source And Beat Coordinates
The system SHALL provide one Rust control-side map evaluator with a checked monotone inverse
for accepted source and beat anchors.

Both coordinates SHALL be strictly increasing, finite and bounded in count with validated
spacing. For adjacent coordinates on each axis, the positive finite difference SHALL be at least
16 times the greater endpoint ULP. ULP SHALL mean the distance from the endpoint magnitude to
the next greater finite binary64 value, using the preceding finite value at the largest finite
magnitude and the smallest positive subnormal at zero. Overflow of this bound SHALL be rejected.
Both forward/inverse slopes and `60*dB/ds` SHALL be finite and positive. These numerical guards
SHALL NOT be presented as a new musical BPM acceptance range or bypassed by clipping.

Policy `pwa-source-seconds-quarter-v1` SHALL use checked piecewise-affine interpolation and its
algebraic inverse with O(log N) lookup. Exact source/beat anchor queries SHALL return the stored
counterpart bits directly. For supported loaded projections, mapping/round-trip error at
representable anchors SHALL be at most one loaded frame, with exact equality for integer fixtures.
Unsafe arithmetic or projection SHALL report unavailable rather than fabricate sample accuracy.

Each interval SHALL explicitly distinguish trusted, uncertain and unknown-count-gap coverage;
meter certainty SHALL remain separate. Ordinary supported queries SHALL NOT bridge uncertain
coverage or unknown-count gaps. Exact gap-boundary coordinates SHALL remain queryable without
certifying a covered interval. Beat units, downbeat labels and meter certainty SHALL NOT be
inferred solely from array position.

#### Scenario: A variable-tempo segment is evaluated in either direction
- **GIVEN** adjacent anchors at source seconds 1.0 and 1.6 with beat indices 2 and 3
- **WHEN** the evaluator maps beat 2.5 to source and back
- **THEN** it returns source seconds 1.3 and beat 2.5 within declared precision

#### Scenario: Invalid or incomplete detection is not promoted to a trusted map
- **GIVEN** duplicate/nonfinite anchors or a gap with unknown beat count
- **WHEN** a map is prepared
- **THEN** invalid coordinates are rejected and uncertain coverage is marked explicitly
- **AND** the last accepted state is retained without fabricating trusted consecutive beats

#### Scenario: Numerically unusable geometry is refused
- **GIVEN** finite anchors with inadequate spacing or an overflowing forward, inverse or BPM slope
- **WHEN** a candidate map is validated
- **THEN** validation rejects it with a reason and preserves the prior accepted state
- **AND** no clipping, hidden BPM range or trusted fallback manufactures a valid map

#### Scenario: A query lands exactly on a stored anchor
- **GIVEN** a validated map with a fractional stored source/beat anchor
- **WHEN** either direction is evaluated at that exact anchor coordinate
- **THEN** the counterpart equals its stored binary64 bits
- **AND** an interpolation round trip does not alter the anchor

### Requirement: Local Tempo And Diagnostic Extrapolation Respect Coverage
The system SHALL derive local source BPM only from the shared map's quarter-note slope, not from
a separately stored tempo curve. At an ordinary internal anchor the right interval SHALL select
the slope; at the final anchor the left interval SHALL select it. If an anchor touches any
uncertain or uncovered interval, its exact coordinate SHALL remain queryable but local BPM SHALL
be unavailable. No gap edge SHALL supply a tempo bridge.

Ordinary queries outside map coverage SHALL be unavailable. Separately named diagnostic
extrapolation MAY use only the adjacent supported endpoint interval and SHALL report unverified
extrapolation. Signed virtual references and extrapolation SHALL NOT grant readable source
indices, physical audio access or future SYNC eligibility. Physical source bounds SHALL remain
independently checked.

#### Scenario: Playhead reaches a covered internal tempo change
- **GIVEN** two trusted intervals with different quarter-note slopes
- **WHEN** local BPM is queried exactly at their shared internal anchor
- **THEN** it uses the right interval under the recorded policy version
- **AND** source/beat anchor coordinates retain their stored values

#### Scenario: A known anchor borders a missing-beat region
- **WHEN** the anchor coordinate and local BPM are queried
- **THEN** its exact recorded coordinate remains available with explicit coverage status
- **AND** local BPM is unavailable and no interpolation crosses the gap

#### Scenario: Diagnostic query lies before the first anchor
- **WHEN** an explicit diagnostic extrapolation succeeds from a supported endpoint interval
- **THEN** the returned coordinate is marked unverified extrapolation
- **AND** it grants no physical read or live adoption authority

### Requirement: Map Work And Projections Have Explicit Bounds
The system SHALL accept 2 through 250,000 accepted anchors and exactly one interval record for
each adjacent pair. Raw beat and downbeat arrays SHALL each contain at most 250,000 events;
explicitly manual maps MAY have no raw detections. Correction lineage SHALL contain at most
250,000 operations. The complete encoded envelope SHALL be at most 64 MiB (67,108,864 bytes),
checked before full decoding; count bounds SHALL be checked before allocating derived storage.
Each projection request SHALL return at most 2,048 positions.

Exceeding a bound SHALL reject the new operation with an explicit reason and preserve previous
state. The system SHALL NOT truncate raw evidence, anchors, lineage or projected success to
conceal refusal. Evaluation results SHALL identify full revision, optional source/beat/local-BPM
values, coordinate/interval status, segment index and physical-domain status. Invalid, uncertain,
gap, outside and unverified-extrapolation outcomes SHALL remain distinguishable from supported
coordinates. Python SHALL NOT implement a second source-to-beat interpolation.
These record/query bounds SHALL NOT substitute for actual process-RAM, simultaneous-preparation,
responsiveness or retained resource acceptance measurements.

#### Scenario: An oversized record is submitted
- **WHEN** its byte, anchor, raw-event or lineage count exceeds the supported bound
- **THEN** preparation rejects it without replacing the last valid record
- **AND** no truncated evidence is labeled accepted

#### Scenario: A visible-range request exceeds its projection limit
- **WHEN** the requested projection cannot fit within the supported returned-position bound
- **THEN** the control result reports the limit explicitly
- **AND** a partial projection is not presented as complete supported coverage

### Requirement: Legacy Scalar Timing Remains A Separate Valid Mode
The system SHALL preserve existing scalar BPM, signed-origin and source-time loop semantics
when restoring projects without an explicitly accepted variable beatmap.

Existing detected beat arrays SHALL NOT automatically replace manual scalar grids. The new
foundation SHALL NOT change live launch scheduling, master transport, current audio addressing,
or prepared-stem playback. Stems SHALL refer to their parent source map when a map is stored.

Current acknowledged constant timing SHALL retain its complete accepted revision, exact binary64
period, signed origin, current ownership and pending/effective distinction. Existing scalar
nearest-even grid and physical-marker ties SHALL remain unchanged. Variable snapping SHALL use
the separately versioned `nearest-quarter-subdivision-later-tie-v1` policy: nearest musical
point, with the later point selected at an exact halfway tie, followed by final loaded-frame
rounding. No broad epsilon SHALL move near-halfway requests onto a different musical point.

#### Scenario: A legacy project has a manual BPM and negative grid origin
- **WHEN** the project is restored with the beatmap foundation available
- **THEN** its existing scalar grid and source-time markers remain unchanged
- **AND** no variable mapping is silently activated

#### Scenario: Scalar and variable halfway rules remain explicit
- **GIVEN** a scalar operation and a variable operation each exactly halfway between musical points
- **WHEN** they resolve under their own recorded policies
- **THEN** scalar retains its existing nearest-even result and variable selects the later point
- **AND** adding the variable foundation does not change scalar marker rounding or live timing

### Requirement: The Foundation Preserves Later Variable Integration Obligations
The system SHALL keep this increment's validated map geometry, actual source-bound acceptance,
displayed revision and current audible state distinct. Parsing/digest consistency SHALL NOT be
reported as implemented source acceptance, persistence integration or audio adoption.

B4 shared editor/snap/auto-loop/local-tempo integration and later prepared live map/trajectory
adoption, continuous SYNC, restore, resource, musical, device and Human acceptance SHALL remain
required. This temporary offline/control-side boundary SHALL NOT replace the final variable B/S
behavior or declare it complete from existing constant timing. Later integration SHALL reuse the
existing master/output clock, current source/lifetime authority, preparation, SourcePlayback,
BPMLOCK/rate resolution, captured input target, scheduler and acknowledgement paths.

#### Scenario: A pure map fixture passes numerical checks
- **WHEN** its structure, inverse and declared precision checks pass
- **THEN** those results establish bounded software evidence for that fixture only
- **AND** editor integration, live variable behavior and the retained acceptance gates remain open

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
