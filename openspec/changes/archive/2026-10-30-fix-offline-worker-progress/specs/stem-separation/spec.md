## ADDED Requirements

### Requirement: Component preparation observes its original cancellation owner
Ordinary background stem work SHALL pass its original captured cancellation predicate into the existing bounded component range reader. Cancellation after one actual chunk SHALL stop further component reads and allocations at the next existing cancellation check, release transient reservations and SHALL NOT register a completed component set. It SHALL NOT recapture another source or publish cancellation as completion. Uncancelled work SHALL retain exact complete component PCM and its existing reader registration. No work SHALL enter realtime.

#### Scenario: Original work is cancelled at its first component range chunk
- **GIVEN** actual captured ordinary stem work with more than one component read chunk
- **WHEN** its original publication is cancelled immediately after the first positioned read
- **THEN** no later chunk or component is read or allocated
- **AND** no component set is registered and transient accounting returns to its prior state

#### Scenario: The original work remains current throughout preparation
- **GIVEN** the identical original source and four complete verified components
- **WHEN** background work completes without cancellation
- **THEN** all exact component ranges and PCM are retained and registered normally
