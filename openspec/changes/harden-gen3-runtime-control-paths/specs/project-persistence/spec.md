## ADDED Requirements

### Requirement: Startup restore publishes only loaded pad audio intent
The system SHALL publish restored per-pad live-audio state during startup only for pads with valid restored sample assignments, while preserving explicit neutralization when pads are unloaded or cleared.

Per-pad gain, EQ, BPM, timing metadata, loop region, Key Lock, stem mode, and stem mask publication SHALL be skipped for empty pads during normal startup projection. Controller paths that unload, clear, or force-reset a pad SHALL still publish the bounded neutral state needed to remove stale Rust live-audio state for that pad.

#### Scenario: Startup skips empty pad per-pad settings
- **GIVEN** a restored project contains valid cached audio for pad 1 only
- **AND** persisted per-pad settings for empty pads differ from defaults
- **WHEN** the application starts and projects restored state to Rust
- **THEN** per-pad live-audio settings are published for pad 1 as needed
- **AND** normal startup projection does not publish per-pad gain, EQ, BPM, timing, loop, Key Lock, stem mode, or stem mask state for the empty pads

#### Scenario: Missing restored sample still neutralizes that pad
- **GIVEN** a restored project references a cached audio file for pad 2
- **AND** the cached file is missing or unusable
- **WHEN** startup clears pad 2
- **THEN** the controller publishes the bounded neutral state required to clear Rust live state for pad 2
- **AND** the system does not publish restored non-default per-pad settings for pad 2 as if the sample were loaded

#### Scenario: Explicit unload keeps force-reset behavior
- **GIVEN** pad 3 is loaded and has non-default live per-pad state
- **WHEN** the performer unloads pad 3
- **THEN** the controller clears project/session state for pad 3
- **AND** the controller publishes bounded neutral live-audio state for pad 3
- **AND** future startup projection treats pad 3 as empty unless it has a valid restored sample assignment
