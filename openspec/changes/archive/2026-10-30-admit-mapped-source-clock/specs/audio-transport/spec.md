## ADDED Requirements

### Requirement: Admitted mapped source clock follows the authoritative transport
An explicitly source-qualified mapped launch SHALL derive its intended source coordinate from the existing TransportTimeline master beat at that intended output frame and the shared canonical SourceBeatmap inverse. It SHALL preserve the captured phase, exact musical loop extent, map identity and master epoch. It MUST NOT integrate scalar local rates as the source-position authority or add a second master/BPM ratio, clock or scheduling path.

#### Scenario: Long vocal and repeated short loop
- **GIVEN** qualified supported maps for a long vocal and a short repeated loop
- **WHEN** their admitted mapped trajectories advance under the same master beat
- **THEN** distant source landmarks SHALL follow each canonical inverse at that beat
- **AND** callback partitions SHALL NOT introduce cumulative phase drift
- **AND** the shared master beat SHALL remain authoritative

#### Scenario: Fractional knots and buffered input
- **WHEN** mapped reads cross a knot between integer output frames or Native input is buffered
- **THEN** the read span SHALL use its actual intended output addresses
- **AND** its exclusive next endpoint SHALL be evaluated canonically across all intervening knots
- **AND** bounded lookup SHALL replace unbounded scans through intervening knots
- **AND** callback-now SHALL NOT relabel the buffered input's trajectory

### Requirement: Preserve exact musical loop length separately from physical rounding
Mapped auto-bars admission SHALL use L=4*bars and the unrounded canonical end S(B(actual start)+L), with required supported whole-interval meter and coverage. It SHALL prepare rounded physical PCM coverage through existing Residency while preserving the exact musical extent. A manual physical end SHALL remain explicit and define L=B(end)-B(start). Map selection alone SHALL NOT activate this behavior.

#### Scenario: Eight bars do not inherit scalar or rounding drift
- **WHEN** explicit mapped auto-loop admission requests eight bars
- **THEN** its musical cycle SHALL remain exactly32quarter notes
- **AND** neither an old scalar endpoint nor physical frame rounding SHALL redefine that cycle
- **AND** unavailable coverage SHALL preserve preceding effective audio with truthful feedback
