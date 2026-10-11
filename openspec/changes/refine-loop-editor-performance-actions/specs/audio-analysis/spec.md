## MODIFIED Requirements

### Requirement: Analysis Can Be Triggered Automatically And Manually
The system SHALL run analysis automatically as part of the existing sample-load
workflow and SHALL require a warning confirmation before the performer's manual
Analyze Audio action enqueues analysis for already-loaded content (E11-16).

Manual analysis SHALL remain analysis-only background work without repeated
decoding, resampling, channel mapping or sample publication. Its warning SHALL
use the same transient confirmation boundary as Unload, through sidebar and
existing mapped keyboard/MIDI actions. Learn SHALL remain capture-only. The
warning SHALL identify its bound pad/source and capture action, pad ID,
content-instance ID, path and the actual native generation/digest/frames/rate.
Controller acceptance SHALL consume the intent once and revalidate that same
content and current eligibility. Selection changes SHALL NOT retarget the
request; equal-path/equal-byte reload, rebind/rearrangement/content-instance
change and lost/stale native source identity SHALL invalidate it.

Cancel, Escape, dismissal, stale or duplicate acceptance SHALL enqueue nothing
and mutate no loader/project/playback/resource state. Warning dismissal SHALL
clear only its own intent, without cancelling Residency/KEYLOCK work. Same-content
mode/window-readiness changes SHALL NOT invalidate the warning or invent a
KEYLOCK-pending analysis veto. Loading pads SHALL remain ineligible. Valid restored
analysis SHALL retain the existing restore path without automatic reanalysis.

#### Scenario: Manual analysis re-runs detection
- **GIVEN** loaded eligible pad A has prior analysis results
- **WHEN** the performer activates Analyze Audio
- **THEN** a warning appears and no analysis task starts
- **WHEN** the performer confirms while captured A content remains eligible
- **THEN** one analysis-only task is enqueued for that content
- **AND** the current manual/grid/loop preservation policy remains in force

#### Scenario: Cancel replacement or loading prevents manual analysis
- **GIVEN** a manual-analysis warning captures pad A content X
- **WHEN** the warning is cancelled, X changes, or A becomes ineligible/loading
- **THEN** no task is admitted from that warning
- **AND** another selected pad cannot become its target

#### Scenario: Manual analysis is blocked while loading
- **GIVEN** a pad is currently loading
- **WHEN** the performer requests or accepts a manual Analyze Audio warning for it
- **THEN** the request is blocked and no analysis-only task starts

#### Scenario: Mapped analysis and same-path reload cannot bypass warning
- **GIVEN** Analyze Audio is mapped to keyboard or MIDI for loaded eligible pad A
- **WHEN** the performer activates it outside Learn
- **THEN** the same content-bound warning appears without enqueueing analysis
- **WHEN** A is reloaded from the same path/bytes before acceptance
- **THEN** the old warning admits no analysis for that new assignment
- **AND** Learn captures its gesture without a warning or analysis task

#### Scenario: Pending mode work survives warning cancellation
- **GIVEN** a current analysis warning and pending or claimed KEYLOCK work for the same assignment
- **WHEN** the warning is cancelled, escaped or dismissed
- **THEN** no analysis starts and mode transaction ownership/readers/feedback/playback remain unchanged
- **AND** unchanged content remains eligible according to the existing analysis policy despite mode/window-readiness changes

#### Scenario: Automatic analysis runs on load
- **WHEN** a normal sample load completes successfully for a pad
- **THEN** the existing automatic load analysis runs before the load is considered complete
- **AND** no performer confirmation is invented for that internal load work

#### Scenario: Analysis results are restored from project state
- **GIVEN** a project contains valid persisted analysis and its sample source exists
- **WHEN** that sample is restored during project loading
- **THEN** the saved results are restored without reanalysis and are immediately available
- **AND** no performer confirmation is invented for the saved-result restore path
