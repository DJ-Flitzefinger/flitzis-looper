## ADDED Requirements

### Requirement: STOP launch revocation tolerates brief publication contention
Native control-side launch cancellation and ordered STOP admission SHALL retry only typed
Pending placement contention for at most 64 attempts while retaining the same sole-producer
exclusion. Every attempt SHALL preflight and claim the complete participant set again;
no participant may be skipped or partially mutated. New control/MIDI publications SHALL NOT
overtake the original cancellation inside this retry boundary.

The retry SHALL remain outside the realtime callback, SHALL contain no sleeps or unbounded
waits, and SHALL leave stale/source/capacity/poison or other terminal failures unchanged.
Persistent Pending SHALL remain an honest admission failure. Cancellation SHALL retain
admitted target flags until the existing ordered STOP handles possibly claimed start tails.
No markerless raw STOP or empty successful cancellation result may replace rejection.

#### Scenario: An unrelated source publication completes during STOP admission
- **GIVEN** PAD1 has an admitted launch and PAD2 holds the actual publication marker
- **WHEN** global cancellation observes Pending and PAD2 completes within the bounded retry
- **THEN** cancellation atomically revokes the original permits and reports admitted targets
- **AND** the original producer exclusion prevents a newer start from overtaking the claim
- **AND** existing guarded STOP can stop claimed tails without changing source or timing

#### Scenario: Publication remains unavailable or STOP capacity is exhausted
- **GIVEN** the marker stays occupied through all attempts or the command ring is full
- **WHEN** STOP admission returns
- **THEN** no partial participant mutation or successful STOP acknowledgement is reported
- **AND** current content and source/launch ownership guards remain intact
