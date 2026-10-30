## ADDED Requirements

### Requirement: Pending placement does not cancel a valid cold source request

The cold loader SHALL distinguish transient Pending placement publication from definitive source replacement or retirement. A valid original cold request MUST retain its original source capture, prepared data and transaction authority through unrelated settings or placement publication. Final publication SHALL use the original request, source epoch, proposal and Native claim, and SHALL perform any bounded waiting only on the existing non-realtime worker with control locks released between retries.

#### Scenario: Key Lock start behavior changes during a valid cold load

- **GIVEN** a valid cold import or restore with its original slot and request still current
- **WHEN** a global Key Lock start-behavior edit temporarily holds the shared placement publication marker
- **THEN** the load does not report terminal cancellation solely because its placement observation is Pending
- **AND** after publication clears, the same original load can publish and receive the real callback acknowledgement
- **AND** the acknowledged pad remains startable

#### Scenario: Genuine cancellation wins while placement is pending

- **GIVEN** an original cold request observing a transient Pending marker
- **WHEN** shutdown, explicit cancellation, original proposal cancellation, request or source-epoch supersession, or definitive slot replacement invalidates that request
- **THEN** the loader rejects that original request without publishing stale PCM or capturing replacement authority
