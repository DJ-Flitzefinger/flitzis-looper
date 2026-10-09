## MODIFIED Requirements

### Requirement: Stem Availability Indicators
The system SHALL show distinct current disk eligibility, preparation/pending/error,
resident readiness, durable desired mode and effective acknowledged selection
through controller/session snapshots without blocking rendering.

The selected-pad sidebar SHALL explain FullMix fallback while ALL STEMS is pending
or failed. Compact grid indicators SHALL NOT claim disk existence as native-ready
or show hover status messages. Rendering SHALL NOT inspect directories, hash,
decode, align, infer, allocate PCM or run blocking work.

#### Scenario: Selected pad shows available stems
- **WHEN** current component windows have actual native readiness
- **THEN** sidebar/grid may indicate ready stems separately from desired/effective mode

#### Scenario: Stem generation progress is visible
- **WHEN** the selected pad generates stems
- **THEN** its sidebar shows stage/progress from snapshots while rendering remains responsive

#### Scenario: Stem generation error preserves pad usability
- **WHEN** generation fails
- **THEN** the sidebar shows the error and FullMix remains usable

#### Scenario: Pad-grid indicators do not show hover messages
- **WHEN** a compact pad stem indicator is hovered
- **THEN** no hover status message appears and sidebar remains the detailed surface

#### Scenario: Disk-only cache is shown honestly
- **WHEN** a pad has valid stem artifacts but no resident component windows
- **THEN** the sidebar distinguishes disk eligibility from readiness/effective mode
- **AND** it can display saved ALL STEMS desire with FullMix currently effective

#### Scenario: Preparation and error keep usable FullMix
- **WHEN** explicit residency demand is preparing or fails
- **THEN** progress/error is visible without blocking UI
- **AND** FullMix remains usable and saved ALL STEMS desire remains available for retry

### Requirement: Stem Mix Controls
The system SHALL provide selected-pad FULL MIX and ALL STEMS controls that can
request missing resident windows from a valid current selected disk cache.

FULL MIX SHALL remain available for loaded audio. ALL STEMS SHALL be requestable
when valid disk eligibility exists even without resident-ready PCM; stale/missing/
incomplete content SHALL show its blocked reason without pretending readiness.
Every explicit ALL STEMS click SHALL request preparation when needed, including
an unchanged saved desire, with bounded coalescing/retry. Desired/pending and
native effective selection SHALL remain distinguishable until genuine feedback.
Ordinary toggles SHALL retain valid windows warm subject to explicit budget and
final-reader retirement; they SHALL NOT delete artifacts or stop/restart voices.

#### Scenario: New projects default to full mix
- **WHEN** a new project starts
- **THEN** per-pad durable mode defaults to FullMix and stems do not affect playback without demand

#### Scenario: All-stems mode requires current prepared stems
- **WHEN** ALL STEMS requests missing current disk-backed windows
- **THEN** it becomes effective only after those windows are prepared and natively acknowledged
- **AND** ready existing windows use the bounded native mode path

#### Scenario: Mix mode buttons are disabled without stems
- **WHEN** a loaded pad has no valid current disk set and no ready component windows
- **THEN** ALL STEMS is blocked with its reason while FULL MIX remains available
- **AND** valid disk-only eligibility enables ALL STEMS preparation without claiming readiness

#### Scenario: Revert to full mix
- **WHEN** FULL MIX is selected
- **THEN** its durable desire is saved and native effective selection changes through feedback
- **AND** no disk deletion or forced window retirement is required

#### Scenario: Saved ALL STEMS still requests first load
- **WHEN** ALL STEMS is clicked with disk eligibility, no resident stems and the same saved desire
- **THEN** the shared controller requests bounded off-thread preparation
- **AND** buttons/status do not represent command enqueue as effective playback

#### Scenario: Return to FullMix keeps warm cache
- **WHEN** FULL MIX is chosen after component readiness
- **THEN** effective selection changes through native feedback and bounded crossfade
- **AND** valid idle windows remain reusable within budget without forced artifact deletion

## ADDED Requirements

### Requirement: Settings exposes deliberate startup stem RAM management
The system SHALL expose `Preload existing stem loops at startup`, default off,
and the explicit aggregate residency budget on the existing global Settings surface.

#### Scenario: Performer deliberately enables preload
- **WHEN** the performer enables the setting
- **THEN** Settings shows the RAM warning and estimated all-pad demand versus budget
- **AND** startup policy requests all eligible windows using bounded admission
- **AND** all216 is reported ready only after actual matching ACKs
- **AND** the UI SHALL explain added RAM/all-occupied eligible demand without generation or guaranteed admission
- **AND** conservative MiB/exact bytes/counts/budget/full-track exceptions/errors SHALL remain understandable
- **AND** unknown native/process overhead SHALL NOT be hidden in a PCM estimate
- **AND** these settings SHALL persist in ProjectState through the existing config

#### Scenario: Default off preserves intentional startup
- **WHEN** a new/older project starts with preload off
- **THEN** only required FullMix ranges prepare by default
- **AND** saved ALL STEMS desire remains visible and can request lazy loading explicitly
