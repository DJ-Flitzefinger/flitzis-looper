## ADDED Requirements

### Requirement: Explicit automatic estimated source coordinates
The Native source-map layer SHALL support a separate v4 envelope containing the
unchanged complete pinned Beat This capture and explicitly estimated coordinates.
Detector order MAY supply a documented operational quarter-count hypothesis; it
SHALL NOT establish reviewed counts, verified quarter interpretation, source 4/4
meter, downbeats, scientific constant timing, source custody or audible phase.
This explicit estimated class supersedes the older prohibition on computational
use of ordinal-only maps solely for v4. Existing Trusted rules remain unchanged.
V1-v3 SHALL reject Estimated coverage, and Uncertain/GapUnknownCount SHALL remain
unusable. User coverage edits SHALL NOT manufacture constructor-owned Estimated.

The `beat-this-local-line-pwa-v1` builder SHALL work off realtime, independently
of global constant-BPM-estimator success. It SHALL split count-discontinuous
adjacencies before centered local fits of at most nine events, retain original
ordinals, emit knots every four events plus endpoints, and test every observation
against the final piecewise-linear geometry. Supported residuals SHALL be bounded
by min(25ms,5% of the local period). Invalid fits/slopes SHALL become explicit gaps
or refuse construction; they SHALL NOT be clamped into apparently valid timing.
Half/double ambiguity and musical count quality SHALL remain unverified.

Source-zero/EOF coverage MAY extrapolate the same supported edge-run slope by at
most one adjacent estimated beat, explicitly as Estimated. Longer unsupported
edges SHALL remain gaps without moving physical audio boundaries or discarding
attacks. Generic query extrapolation SHALL retain its diagnostic-only semantics.

All source, retained raw, derived geometry, JSON tree, serialization and immutable
index/prefix costs SHALL receive checked admission before derived allocation or
capture embedding, within the existing 512-MiB job bound. No new callback work,
clock, worker pool or current source/adoption authority SHALL arise from parsing.

#### Scenario: Supported estimated timing retains uncertainty
- **GIVEN** locally coherent quantized neural beat observations
- **WHEN** the automatic builder constructs and round-trips v4 geometry
- **THEN** original raw arrays/provenance remain unchanged and anchors stay Estimated
- **AND** meter remains Unknown and bar/downbeat qualification remains absent
- **AND** numeric beat queries may use Estimated coverage without reporting Trusted

#### Scenario: Variable timing, gaps and bounded source edges
- **GIVEN** locally coherent variable tempo with no usable global constant estimate
- **WHEN** an interval has supported event order and bounded final-map residuals
- **THEN** its estimated variable timing remains representable
- **AND** missing/extra events or excessive residuals refuse only affected coverage
- **AND** source edges longer than one estimated beat remain unsupported

#### Scenario: Mixed coverage queries preserve their provenance
- **WHEN** a positive query interval crosses Trusted and Estimated segments
- **THEN** its successful result reports Estimated
- **AND** an unsupported gap takes precedence at a shared exact anchor
- **AND** an exterior endpoint-only gap retains existing open-interval semantics

This construction/query step SHALL NOT claim automatic App availability. Genuine
original-source artifact/delivery custody, persisted selection/manual priorities,
Copy/restore, operational loop-bar semantics, GridOffset and actual renderer ACK
remain required before automatic activation and the single-SYNC UI consolidation.
