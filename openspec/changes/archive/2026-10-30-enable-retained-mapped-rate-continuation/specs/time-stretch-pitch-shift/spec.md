## ADDED Requirements

### Requirement: Preserve qualified equal-endpoint mapped Native rate continuity
The system SHALL permit an original qualified global-rate claim to continue an
already productive mapped Native owner when its exact current logical and actual
input checkpoints are equal, while retaining the complete Native/FIFO history.

#### Scenario: A productive mapped voice changes master period
- **GIVEN** an ordinary mapped voice has source-bound first-mix confirmation and private current productive ownership
- **AND** its canonical logical and actual checkpoints are exactly equal at E without pause, phase or transition
- **WHEN** the complete original global claim accepts a master-continuous replacement
- **THEN** both source trajectories SHALL continue at their exact original first pair using the new mapped projection
- **AND** Native identity, FIFO contents, input serial and an existing partial block's first-input pitch SHALL remain unchanged
- **AND** immutable birth/raw provenance SHALL use its original projection while cumulative raw output and the current continuation advance separately

#### Scenario: Old descriptors and unsupported endpoints cannot cross a cut
- **WHEN** a real mapped continuation is installed without intervening input
- **THEN** a checked non-reused continuation revision SHALL reject every earlier minted descriptor before adapter mutation
- **AND** overflow, unequal endpoints, unconfirmed first mix, phase, pause, transition or unavailable readers SHALL refuse before any cohort effect
- **AND** a speed-only effect with unchanged mapped epoch SHALL preserve the existing clock and descriptor revision without applying speed twice
