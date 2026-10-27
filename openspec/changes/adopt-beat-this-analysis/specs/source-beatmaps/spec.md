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
### Requirement: Optional Requested Grid Origin Uses Exact Source Phase

The Native map projection SHALL accept additive optional requested
`phase_origin_seconds` and `operational_four_quarter_bars` fields. Absent values
SHALL retain the existing legacy map behavior. A requested grid zero SHALL remain
intent data: its original delivery, current source binding and Native-captured
timing authority revision SHALL independently qualify admission. Python SHALL NOT
supply that authority revision. Legacy timing intent and SYNC intent revisions
SHALL NOT be rewritten merely to represent grid-target freshness.

Explicit grid-following phase SHALL target original-map `B(g) + M(n)` modulo the
unchanged physical loop's quarter extent. It SHALL retain the actual source pair
at admission E and converge using the existing bounded positive/negative phase
correction. Original launch T/E, physical A/Z/Q, original map knots and local tempo
SHALL remain unchanged. It SHALL NOT use an A-relative subtraction that cancels g,
round B(g) to a new binary64 anchor, translate source geometry or jump the first read.

Preparation SHALL include g and its original inverse segment in the checked
numeric grids. Qualified geometry, immutable phase anchors, programmes and read
witnesses SHALL bind the complete target and captured Native authority. Geometry
validation SHALL reject a different prepared target inverse even with equal origin
bits. Geometry-free retained spans SHALL evaluate only their immutable captured
target; an incoming historical launch reference cannot inject another g-side inverse.

Absolute grid phase SHALL use the complete current Native source/OwnACK, actual-E
reader/domain, master-projection, rate and final-cohort claim. It SHALL NOT require
or manufacture historical launch qualification: original T/M(T) remains unchanged
as history where available. Legacy None and Fixed/static paths SHALL retain their
existing intentional-launch guards. Actual programme/read confirmation remains
separate from intent or coordinate construction.

Copied playback SHALL retain the existing 512-byte and 32-voice 16-KiB cohort
bounds on the supported 64-bit build. Scalar glide fields MAY share the existing
exclusive clock storage with mapped/static trajectories, preserving exact scalar
target/ratio/timer state and their distinct applied versus nominal rate semantics.
This representation SHALL NOT change any clock law, history or source authority.
Unsupported coverage and unavailable numeric preparation SHALL refuse the explicit
target without silently using the legacy A-relative target. Signed historical
origin intent remains representable; virtual negative-origin activation support
is separate from this inert Native foundation.

Operational four-quarter bars SHALL remove only the computational verified-4/4
prerequisite for a requested beat count. Unknown meter, unreviewed detector count,
downbeat evidence and accepted-timing status SHALL remain unchanged. This Native
foundation SHALL NOT activate automatic App selection or assert musical acceptance.

#### Scenario: Different source segments and a delayed actual effect
- **GIVEN** A, the current source pair and g use three distinct original inverse segments
- **WHEN** explicit grid-following phase is admitted after its genuine launch T
- **THEN** the first read at E equals the original current source pair
- **AND** bounded correction converges toward B(g)+M using exact original axes
- **AND** physical loop boundaries, quarter extent and existing legacy defaults remain intact

#### Scenario: Grid-origin ABA and mixed copied references
- **WHEN** g changes and returns through genuine Native timing edits
- **THEN** its changed authority fences the original prepared target
- **AND** an old phase epoch refuses geometry with a different prepared g-side inverse
- **AND** another PAD's source and timing remain independent

#### Scenario: Unknown meter permits an operational count only
- **GIVEN** source-qualified geometry retains unknown meter
- **WHEN** the explicit operational four-quarter policy prepares a loop extent
- **THEN** numerical beat-count preparation may succeed without verified-4/4 status
- **AND** the original map and meter evidence remain unchanged
