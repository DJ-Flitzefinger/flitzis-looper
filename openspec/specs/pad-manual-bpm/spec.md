# pad-manual-bpm Specification

## Purpose
To define manual per-pad BPM entry, Tap BPM behavior, and effective BPM precedence for playback and UI timing.
## Requirements
### Requirement: Store Manual BPM Per Pad
The system SHALL allow a performer to set an optional manual BPM value per pad.

When a manual BPM is set for a pad, it SHALL be treated as the pad’s effective BPM for UI display and subsequent tempo workflows.

When a manual BPM is cleared for a pad, the effective BPM SHALL fall back to the detected BPM (when available).

#### Scenario: Manual BPM overrides detected BPM
- **GIVEN** a pad has a detected BPM of 123.4
- **WHEN** the performer sets the pad’s manual BPM to 120.0
- **THEN** the pad’s effective BPM becomes 120.0

#### Scenario: Clearing manual BPM falls back to detected BPM
- **GIVEN** a pad has a detected BPM of 123.4
- **AND** the performer previously set a manual BPM of 120.0
- **WHEN** the performer clears the pad’s manual BPM
- **THEN** the pad’s effective BPM becomes 123.4

### Requirement: Tap BPM Computes And Sets Manual BPM
The system SHALL provide a Tap BPM workflow that computes a BPM from user taps and sets it as the pad’s manual BPM.

The Tap BPM workflow SHALL:
- Record a tap on **left mouse button down**.
- Retain the complete current target's measurement series until a gap greater than three seconds, a target switch or source replacement resets it.
- Fit a constant period to original tap indices. Starting with four taps, score median long-span indexed slopes and adjacent-interval seeds by the same positive inlier predicate, preferring the long-span seed on a tie. Use its median phase, bounded robust reweighting and repeated final inlier fits.
- Reassess all original timestamps at every tap; excluded taps may re-enter and SHALL NOT be deleted or renumbered.
- Require a strict majority of original taps spanning at least half the indexed series before publishing a robust estimate. Ambiguous/unstable fits SHALL leave the previous BPM unchanged.
- Derive the outlier gate from robust prediction jitter, with a 3ms resolution floor and a quarter-period cap. The gate SHALL NOT shrink with slope standard error and reject ordinary timing noise as history grows.
- Preserve the existing two/three-tap initialization, source admission, manual intent, grid/loop residency and master-clock path.

#### Scenario: Three taps compute BPM
- **GIVEN** a pad is the current Tap BPM target
- **WHEN** the performer taps three times with an interval of 0.5 seconds between taps
- **THEN** the computed BPM is approximately 120.0
- **AND** the pad’s manual BPM is set to that computed value

#### Scenario: Long measurement rejects isolated late taps
- **GIVEN** a pad is the current Tap BPM target
- **WHEN** the performer taps 40 times and four isolated timestamps are delayed
- **THEN** all 40 original timestamps remain available for reassessment
- **AND** a qualified fit uses the original indices of the consistent taps to reduce tempo error without including those isolated delays
- **AND** finite human taps remain an estimate, not proof of the source audio's exact tempo

### Requirement: Half And Double BPM Use Current Manual Correction
The pad BPM controls SHALL expose `BPM /2` and `BPM x2`. They SHALL resolve current
effective BPM inside the controller action after assignment readiness, multiply
by exactly one half or two without rounding, and use the existing source-fenced
manual-BPM admission once. Unavailable, nonfinite, nonpositive, overflowed or
source-stale corrections SHALL not change the stored BPM wish. Clear Manual BPM
SHALL restore the detected estimate. These numeric corrections SHALL NOT rewrite
detector observations or selected map quarter coordinates or certify SYNC.

#### Scenario: Fractional double-tempo detection is corrected
- **GIVEN** a current fractional effective BPM
- **WHEN** the performer presses `BPM /2`
- **THEN** its exact half is stored as a manual correction through existing timing and residency controls
- **AND** the original analysis, key, explicit loop markers and grid offset remain available
