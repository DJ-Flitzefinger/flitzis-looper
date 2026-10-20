## ADDED Requirements

### Requirement: Detached timing publication belongs to its captured stream
Accepted timing publication and saved timing restore SHALL first enqueue only while their captured producer remains the current stream producer. A completion after shutdown or stream replacement SHALL reject before changing publication epochs, ticket acceptance or pending timing records, and SHALL NOT recapture or replay the operation against a replacement stream.

#### Scenario: Shutdown finishes before detached completion admission
- **GIVEN** captured timing work has not acquired queue admission
- **WHEN** shutdown completes while that work is detached
- **THEN** completion rejects without enqueueing into the old producer or creating pending timing records
- **AND** source request and timing epochs remain at their post-shutdown values

#### Scenario: A new stream exists when old work returns
- **GIVEN** detached timing work captured an earlier producer
- **WHEN** a replacement stream becomes current before that work first publishes
- **THEN** the old work rejects without writing to either producer or mutating the new stream's publication state
