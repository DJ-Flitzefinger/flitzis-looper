## ADDED Requirements

### Requirement: Copied mapped time projection retains the sole master authority
Mapped clock epochs SHALL copy only a fixed immutable projection extracted from the actual TransportTimeline. The projection SHALL retain the exact sample rate, present master period, beat-anchor frame and beat-anchor position used by that authority's clock comparison and evaluation. It SHALL preserve the existing binary64 subtraction/division/addition order and epoch identity, independently of unrelated output-progress, grid/downbeat or bootstrap bookkeeping. Missing master SHALL refuse projection construction. This copied data SHALL NOT create a second clock, owner, source permit or live-adoption permission.

#### Scenario: A frozen mapped epoch survives unrelated transport progress
- **WHEN** output progress or unrelated bootstrap/display state changes while the actual beat epoch remains unchanged
- **THEN** copied beat queries and epoch comparisons SHALL remain exactly compatible with the original authoritative projection
- **AND** copying SHALL require no allocation or owning storage

#### Scenario: The actual master epoch changes or disappears
- **WHEN** a current transport has a different period, sample rate, beat anchor or missing master
- **THEN** comparison with the preceding copied projection SHALL reject that mismatch
- **AND** it SHALL NOT refresh the old mapped epoch from unrelated current callback time or infer a master from a global policy choice
