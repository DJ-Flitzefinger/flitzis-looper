## ADDED Requirements

### Requirement: Persisted pad tables always match the fixed pad capacity
Every accepted persisted per-pad table SHALL contain exactly the configured pad capacity. Omitted tables SHALL retain their valid default/migration behavior; explicitly truncated or oversized tables SHALL enter the existing invalid-config handling path before any indexed controller or UI access. The system SHALL NOT silently shift content between pad IDs or reset valid Human data to repair length.

#### Scenario: A saved table is truncated or exceeds the pad count
- **GIVEN** a supplied per-pad path/duration/analysis/BPM table has invalid length
- **WHEN** configuration validation runs
- **THEN** the invalid configuration is rejected through the existing policy
- **AND** no accepted model can later fail solely because that table lacks a pad row
