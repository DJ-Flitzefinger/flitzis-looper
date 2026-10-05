## MODIFIED Requirements

### Requirement: Loaded pad loop defaults use track start and 8 bars
The system SHALL initialize genuinely new track assignments with Auto enabled,
8.0 bars and a loaded-frame loop start immediately before the first finite
any-channel amplitude strictly exceeding 0.01 full scale, clamped to frame zero.
The symmetric deadzone SHALL be fixed independently of track peak amplitude.
Both symmetric tolerance boundaries SHALL
count as near-zero. There SHALL be no fixed millisecond pre-roll.

Detection SHALL inspect immutable loaded PCM outside the UI and realtime callback,
without channel cancellation, audio trimming, padding or source-origin changes.
The tolerance SHALL remain an initial testable heuristic, not exact-silence or
musical-downbeat certification. Nonfinite values SHALL NOT establish activity.
The system SHALL set an independent scalar grid base at the same valid candidate,
with zero manual grid offset, without musical snapping or rewriting BPM or raw
beat/downbeat analysis. Other pads SHALL remain unchanged.

Absent or invalid optional candidates SHALL retain loop-zero and legacy grid-base
fallback. Without BPM, Auto SHALL remain enabled at 8.0 bars with no musical end.

#### Scenario: Small precursor ripples stay inside the widened deadzone
- **GIVEN** precursor samples remain between -0.005 and +0.005 full scale
- **AND** frame N is the first finite channel amplitude outside [-0.01, +0.01]
- **WHEN** new-track activity is detected
- **THEN** the candidate is frame N-1, clamped to zero
- **AND** a later loud peak does not change the deadzone or selected candidate

#### Scenario: The entire track stays inside the deadzone
- **GIVEN** all finite samples lie within [-0.01, +0.01], including the boundaries
- **WHEN** initial activity detection runs
- **THEN** no activity candidate is returned
- **AND** the ordinary zero-loop and legacy grid fallback applies
- **AND** the threshold is not silently reduced relative to a quiet track's peak

#### Scenario: Low-level residue precedes an attack
- **GIVEN** all finite channel amplitudes remain inside the tolerance until frame N
- **AND** a finite channel first exceeds tolerance at frame N greater than zero
- **WHEN** new loading completes for its current source request
- **THEN** loop and grid initialize at loaded frame N-1 with zero manual offset
- **AND** Auto is enabled with 8.0 bars and source audio remains unchanged

#### Scenario: Activity begins at frame zero
- **GIVEN** first-frame activity already exceeds tolerance
- **WHEN** initial placement runs
- **THEN** loop and grid start at zero without adding audio or negative markers

#### Scenario: Stereo activity would cancel in mono
- **GIVEN** valid activity is anti-phase or present in only one channel
- **WHEN** detection runs
- **THEN** individual channel amplitudes establish the first crossing

#### Scenario: Pickup or noise precedes a musical downbeat
- **GIVEN** the first crossing is a pickup or noise event
- **WHEN** it initializes loop and grid
- **THEN** the initial grid is an editable seed rather than a certified downbeat
- **AND** raw beat/downbeat labels and BPM are unchanged

#### Scenario: Silence or invalid metadata has no candidate
- **WHEN** new-track loading provides no valid activity candidate
- **THEN** loop start is zero with Auto 8 bars
- **AND** absent grid-base intent keeps the analysis/zero fallback

#### Scenario: BPM is not yet available
- **GIVEN** new loading has a valid activity candidate but no BPM
- **WHEN** defaults are initialized
- **THEN** loop and grid base retain that position without inventing BPM
- **AND** musical loop end remains unavailable until BPM is supplied

## ADDED Requirements

### Requirement: Initial activity placement preserves saved and manual intent
The system SHALL apply activity placement only to new assignments and SHALL
preserve saved/manual loop, grid-base, offset and BPM intent on project restore,
same-source reload, reanalysis and subsequent manual loop edits.
The candidate SHALL share existing source/request stale-result rejection.
The independent base SHALL remain unaffected by the manual offset's one-bar clamp.

#### Scenario: Restoration preserves manually chosen source positions
- **GIVEN** saved loop and grid intent differ from newly detected activity
- **WHEN** the project or the same cached source is restored
- **THEN** stored intent is retained without automatic reinitialization

#### Scenario: Analysis or BPM changes after initialization
- **GIVEN** activity begins after more than one bar of leading silence
- **WHEN** analysis arrives or BPM changes
- **THEN** the independent grid base stays at its saved source position
- **AND** only the manual grid offset is subject to its existing clamp

#### Scenario: Loop is moved later
- **WHEN** the performer selects a later loop start
- **THEN** grid base and manual offset remain unchanged
- **AND** displayed counting uses the newly selected loop reference

#### Scenario: A superseded load completes late
- **WHEN** an activity candidate no longer matches the current source request
- **THEN** it is rejected with the stale load result
- **AND** it cannot replace the current pad's loop or grid intent
