## ADDED Requirements

### Requirement: Scalar Physical Markers Use Absolute Projection
The system SHALL derive scalar snapped markers and automatic loop ends from the shared
effective-period/source-origin projection and round each absolute physical boundary once.

An automatic loop end SHALL advance the selected source start by 4*bars beats; an off-grid
start SHALL retain its selected phase. Source bounds, manual loop intent and existing restore,
reanalysis and loaded-rate behavior SHALL remain in force without a persistence migration.

#### Scenario: Fractional automatic loop endpoint
- **GIVEN** an off-grid physical loop start and genuine fractional BPM
- **WHEN** the auto-loop bar count changes
- **THEN** the endpoint is evaluated from that start plus the exact musical duration
- **AND** physical endpoint error is at most half a loaded frame before source-bound clipping
- **AND** the source grid origin remains unchanged
