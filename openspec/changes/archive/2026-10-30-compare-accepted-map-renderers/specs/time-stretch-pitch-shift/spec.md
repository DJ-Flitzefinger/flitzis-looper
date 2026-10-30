## ADDED Requirements

### Requirement: Renderer comparison consumes the canonical accepted map
The isolated comparison SHALL consume an actual validated SourceBeatmap and complete synthetic source PCM with verified matching geometry and complete source hashes. The same checked Native inverse SHALL derive sampled source positions and monotone positive offline keyframes. An independent frozen oracle MUST verify canonical coordinates without replacing the shared evaluator. Synthetic data consistency SHALL NOT claim current producer custody or live preparation permission.

#### Scenario: A nonuniform trusted map is compared
- **GIVEN** the frozen source anchors and master/output spans describe rates0.5,1,2,1.5
- **WHEN** the two existing render paths process the comparison
- **THEN** they use the same accepted map revision and source/output addressing
- **AND** their audio and coordinate errors are reported separately

### Requirement: Comparison retains exact pitch coupling and unsupported outcomes
The comparison SHALL preserve canonical coordinates for every independent semitone intent k, with h=2^(k/12). Mapped-reader Native pitch SHALL be h/r with Key Lock enabled and h when disabled. Original-source offline Stretcher SHALL use pitch h for enabled Key Lock; variable-pitch behavior unavailable through its offline API MUST be reported unsupported rather than approximated or silently clipped.

#### Scenario: Key Lock is disabled under variable map rate
- **WHEN** the mapped-reader path processes a nonzero semitone intent
- **THEN** its pitch follows h times the physical map rate
- **AND** the unavailable equivalent offline pitch automation is identified explicitly

### Requirement: Diagnostic evidence preserves source addressing and original failures
The comparison SHALL reuse the existing fractional reader, Native/FIFO adapter and offline Stretcher lifetime owner. It MUST cover nonuniform segment boundaries, caller partitions, loop-address/seam behavior and explicit unsupported coverage. Parent-map stem comparisons SHALL report timing and audio coherence separately. Complete audio and original timing, pitch, discard and tail failures MUST remain visible without fitted delay, gain or crop. Trusted unknown-meter geometry SHALL remain eligible for quarter-note diagnostics.

#### Scenario: A gap intersects the requested trajectory
- **WHEN** the diagnostic validates the accepted trajectory
- **THEN** it refuses before starting the candidate renderer
- **AND** no scalar fallback or extrapolated source position is substituted

#### Scenario: Coordinates pass while an attack fails
- **WHEN** the shared map matches the frozen oracle but complete audio violates an original predicate
- **THEN** the report preserves both results and the failure
- **AND** it does not declare a production winner or enable live mapped playback

### Requirement: Comparison remains outside production audio authority
All comparison construction, evaluation, allocation, processing and reporting SHALL remain isolated and non-realtime. No result SHALL alter production editor selection, scalar playback, Quantize, SYNC, source/preparation permits or scheduler policy. B5 full-domain, B6/B7/B8/P6, musical and genuine Human/device gates SHALL remain separate.

#### Scenario: The comparison completes
- **THEN** source/map/native identity and actual measurement limits are retained
- **AND** the App and its live callback behavior remain unchanged
