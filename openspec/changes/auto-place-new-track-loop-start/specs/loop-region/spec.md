## MODIFIED Requirements

### Requirement: Loaded pad loop defaults use track start and 8 bars
The system SHALL initialize a genuinely new track assignment with auto-loop
enabled, an 8.0-bar count and a sample-addressed loop start at its first detected
signal activity including bounded leading pre-roll, or at 0.0 seconds when no
valid activity candidate exists.

Detection SHALL inspect immutable loaded PCM outside the realtime callback and
UI thread, preserving complete source audio and its loaded-rate frame origin.
The system SHALL inspect activity across channels without mono cancellation.
The system SHALL NOT musically snap the automatic initial candidate or use it
to modify BPM, beat/downbeat labels, signed grid origin or another pad.

The first signal activity SHALL NOT be treated as a certified beat or downbeat.
If effective BPM is unavailable, auto-loop SHALL remain enabled at 8.0 bars
with no musical end until a BPM becomes available. Missing or invalid optional
candidate metadata SHALL use the zero fallback.

#### Scenario: New track has leading blank audio
- **GIVEN** a new assignment has leading blank frames followed by signal activity
- **WHEN** background loading completes for its current source request
- **THEN** the initial loop start uses the detected loaded-frame position with
  bounded pre-roll clamped to zero
- **AND** auto-loop is enabled with 8.0 bars
- **AND** the source audio and independent musical grid remain unchanged

#### Scenario: Activity is confined to one channel or anti-phase stereo
- **GIVEN** a new track contains valid signal that would cancel under mono mixing
- **WHEN** initial activity is detected
- **THEN** the individual channel activity determines the initial candidate
- **AND** the signal is not classified as silent because of channel cancellation

#### Scenario: First activity is a pickup rather than a downbeat
- **GIVEN** a new track starts with a sample or pickup before its musical downbeat
- **WHEN** the candidate initializes its loop start
- **THEN** signal activity may set that start
- **AND** no beat/downbeat identity or grid correction is inferred from it
- **AND** the performer can subsequently adjust the loop and grid independently

#### Scenario: Silence or unavailable activity metadata
- **GIVEN** a new assignment has no valid activity candidate
- **WHEN** loop defaults are initialized
- **THEN** loop start is 0.0 seconds and auto-loop remains enabled at 8.0 bars

#### Scenario: No BPM accompanies a valid activity candidate
- **GIVEN** a new track has a valid signal candidate but no effective BPM
- **WHEN** its loop defaults are initialized
- **THEN** the candidate initializes the start without inventing a BPM
- **AND** no musical loop end is computed until BPM is available

## ADDED Requirements

### Requirement: Initial activity placement preserves saved and manual intent
The system SHALL apply automatic activity placement only when assigning a new
track and SHALL preserve saved/manual loop, BPM and signed grid intent during
project restoration, analysis refresh and subsequent grid or marker edits.

The candidate SHALL share the load request/source identity and stale-result
rejection already used for source publication. A superseded load candidate
SHALL NOT overwrite replacement audio or newer performer intent.

#### Scenario: Restore preserves a saved loop before the detected activity
- **GIVEN** a saved project intentionally starts its loop before first activity
- **WHEN** that source is restored
- **THEN** its stored loop marker remains unchanged
- **AND** automatic activity placement does not override it

#### Scenario: Reanalysis or a grid edit preserves the loop marker
- **GIVEN** a loaded pad has an accepted manual loop start
- **WHEN** analysis or its musical grid is updated
- **THEN** first-activity detection does not reinitialize its loop marker

#### Scenario: A stale load result arrives after source replacement
- **GIVEN** an earlier load result contains an activity candidate
- **WHEN** that result no longer matches the active load request
- **THEN** its loop candidate and source result are rejected together
- **AND** the current pad intent remains unchanged
