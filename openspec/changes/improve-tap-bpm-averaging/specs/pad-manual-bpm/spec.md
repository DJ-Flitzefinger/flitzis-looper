## MODIFIED Requirements

### Requirement: Tap BPM Computes And Sets Manual BPM
The system SHALL provide a Tap BPM workflow that computes BPM from an explicit performer tap
measurement and sets it as the pad's manual BPM.

The Tap BPM workflow SHALL record taps only when the performer explicitly activates the Tap BPM
control. The first tap for a target pad SHALL start the current measurement series without setting
manual BPM. The second and every later monotonic tap in that series SHALL estimate one constant
tempo from all accepted tap timestamps in the current series, using a least-squares fit of tap index
to tap time, and SHALL update the pad's manual BPM immediately. The measurement series SHALL
continue until the performer changes the Tap BPM target pad or the next tap arrives more than
3.0 seconds after the previous accepted tap; after such a pause, that tap SHALL become the first tap
of a new measurement series.

The Tap BPM workflow SHALL NOT start or extend a measurement series merely because a song is
loaded, playing, analyzed, or displayed. Tap BPM measurement state remains Python/control-plane UI
state and SHALL NOT add disk I/O, Python/GIL access, logging, blocking work, heavy allocation, or
neural inference to the Rust audio callback.

#### Scenario: First tap starts measurement without BPM
- **GIVEN** a pad is the current Tap BPM target
- **AND** no Tap BPM measurement series is active for that pad
- **WHEN** the performer activates Tap BPM once
- **THEN** the system records the first tap in the current series
- **AND** no manual BPM value is set from that single tap

#### Scenario: Second tap computes BPM immediately
- **GIVEN** a pad is the current Tap BPM target
- **AND** the performer has activated Tap BPM once
- **WHEN** the performer activates Tap BPM again 0.5 seconds later
- **THEN** the computed BPM is approximately 120.0
- **AND** the pad's manual BPM is set to that computed value

#### Scenario: Long tap series fits one constant tempo from all accepted taps
- **GIVEN** a pad is the current Tap BPM target
- **WHEN** the performer activates Tap BPM six times with accepted consecutive intervals of
  0.5, 0.5, 0.5, 0.5, and 1.0 seconds
- **THEN** the computed BPM is approximately 105.0
- **AND** the computation uses all accepted tap timestamps in the current series rather than a
  fixed five-tap window or only the first and last timestamps

#### Scenario: Pause longer than three seconds resets measurement
- **GIVEN** a pad is the current Tap BPM target
- **AND** a Tap BPM measurement series already contains accepted taps
- **WHEN** the performer waits more than 3.0 seconds and activates Tap BPM again
- **THEN** the system starts a new measurement series with that tap
- **AND** no manual BPM value is set from that reset tap alone
- **WHEN** the performer activates Tap BPM again 0.5 seconds later
- **THEN** the computed BPM is approximately 120.0

#### Scenario: No background measurement without Tap BPM
- **GIVEN** a song is loaded or playing
- **WHEN** the performer does not activate the Tap BPM control
- **THEN** the Tap BPM measurement series is not started or extended
