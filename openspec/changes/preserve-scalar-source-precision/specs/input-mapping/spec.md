## ADDED Requirements

### Requirement: Runtime Tempo Signatures Follow Effective Resolver
The system SHALL build MIDI runtime tempo publication and its invalidation signature from
the same authoritative effective BPM resolver used by pad control timing.

Changed effective tempo, source/analysis replacement or relevant loop/origin metadata SHALL
invalidate runtime publication as appropriate; raw detected BPM SHALL NOT bypass a manual
override. Future accepted timing revisions SHALL extend this identity before adoption.

#### Scenario: Manual override controls runtime metadata
- **GIVEN** detected BPM 120.00128936767578 and manual BPM 119.999
- **WHEN** MIDI runtime metadata is published
- **THEN** its tempo and signature use effective BPM 119.999
- **AND** changing or clearing the override causes a new publication
