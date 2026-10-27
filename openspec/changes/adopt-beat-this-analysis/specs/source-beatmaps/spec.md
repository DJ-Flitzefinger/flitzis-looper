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

### Requirement: Ordinary Analysis Carries Optional Automatic Geometry Under Native Custody

Configured ordinary neural analysis SHALL attempt an optional v4 Estimated map directly
from its validated native raw capture. It SHALL preserve original decoder identity,
timestamps, unknown meter and unreviewed count interpretation. Existing source-second
geometry may be prepared for a different loaded playback rate without relabelling
the original capture. This data SHALL NOT set accepted timing or a phase ACK.

The existing cold-load/analysis completion SHALL carry the optional map and its
sealed artifact privately. Storage SHALL verify the actual committed cold lease or
complete-source reader. Build/store failure SHALL preserve successful scalar tempo,
key and source adoption and retain a separate optional-map outcome. Preparation
SHALL account for simultaneously live PCM, raw/encoded data, method/grid storage,
map geometry and artifact reopening under the existing bounded worker budgets.

Only a genuine automatic terminal may provide its private one-shot export claim.
The claim SHALL be created per original admission after artifact storage, so cached
immutable geometry cannot share mutable consume state across PADs. Export SHALL
validate the original source, placement and strict analysis epoch under the existing
request lock. Cold delivery requires its explicit current timing epoch; task
delivery requires its admitted analysis epoch. Stale, foreign, manual and pending
deliveries SHALL NOT acquire or spend export permission. Fallible Python wrappers
SHALL be prepared before the final checks and one-shot claim. No fresh owner may be
recaptured from the current mutable slot.
The original automatic delivery SHALL remain ineligible for existing map getters
and qualification before and after consumption. Only the separately exported
derived delivery, with its consumed claim, may expose that map custody.

Successful export MAY derive a separate source-only map delivery from that same
original owner, preserving cancellation and source lifetime while clearing only
the derived analysis epoch. The original analysis delivery remains unchanged.
The derived delivery SHALL always report stale analysis timing while retaining
source-slot resolution for independent geometry qualification.
Later timing changes do not turn original analysis metadata current; actual source
replacement, retirement and cancellation still fence the derived map. Replay
SHALL return no new export, including after unselected retirement. Unconsumed or
failed exports SHALL use the existing reserved artifact cleanup owner.

This carrier alone SHALL NOT select an App map, edit loop markers, publish a
runtime map or activate SYNC. Automatic selection, playback-rate marker rounding,
GridOffset beat-axis alignment, operational bar extents and the single optional
SYNC control remain the connected activation work.

#### Scenario: Timing changes after valid data export
- **GIVEN** a genuine automatic result is exported once under its original current epoch
- **WHEN** a later BPM or resident-window edit changes the timing epoch
- **THEN** the original analysis metadata remains stale
- **AND** the derived immutable geometry still requires the same original current source
- **AND** neither replay nor edited DTO fields can acquire another export
