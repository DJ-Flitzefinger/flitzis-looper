# pad-manual-bpm Specification

## Purpose
TBD - created by archiving change add-pad-manual-bpm. Update Purpose after archive.
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
- Maintain a window of the most recent 5 tap timestamps for the current target pad.
- Compute BPM from the average interval between consecutive taps in that window.

#### Scenario: Three taps compute BPM
- **GIVEN** a pad is the current Tap BPM target
- **WHEN** the performer taps three times with an interval of 0.5 seconds between taps
- **THEN** the computed BPM is approximately 120.0
- **AND** the pad’s manual BPM is set to that computed value

#### Scenario: Tap window is capped at 5 timestamps
- **GIVEN** a pad is the current Tap BPM target
- **WHEN** the performer taps 10 times
- **THEN** only the 5 most recent tap timestamps are used to compute BPM
- **AND** the computed BPM is based on the average interval between consecutive taps in that 5-tap window

