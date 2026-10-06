## ADDED Requirements

### Requirement: Accepted Constant Timing Requires Explicit Source-Bound Acceptance
The system SHALL construct immutable accepted constant timing outside realtime
processing only from complete bound backend evidence, a freshly recomputed uniquely
supported verified count interpretation and an explicit named acceptance assertion
with provenance. It SHALL reject unsupported, unverified or ambiguous candidates
and SHALL NOT treat a good fit as independent musical acceptance.

#### Scenario: A mutable diagnostic report claims support
- **GIVEN** caller-editable summary or refinement diagnostics
- **WHEN** accepted timing is requested
- **THEN** construction recomputes the evidence path instead of trusting changed diagnostics
- **AND** it requires independent count evidence and explicit acceptance provenance

#### Scenario: Repeated attacks lack musical quarter evidence
- **GIVEN** comparable PCM attacks and viable half/normal/double interpretations
- **WHEN** no independent quarter assertion resolves those interpretations
- **THEN** no accepted timing record is produced

### Requirement: Accepted Timing Revision Identifies Counts Policies Period Error And Origin
The system SHALL assign accepted constant timing a versioned canonical revision
that binds complete source/PCM/backend/request evidence, selected and evaluated
counts including denominator and provenance, fit/feature policies and diagnostics,
accepted binary64 period/error state, acceptance assertion and independently chosen
origin with provenance. It SHALL preserve exact floating bits and keep the fitted
intercept separate from source zero and the selected grid origin.

#### Scenario: One raw revision has two verified musical interpretations
- **GIVEN** identical raw evidence assessed separately under different explicit rational count interpretations
- **WHEN** each interpretation is independently supported and explicitly accepted
- **THEN** their accepted revisions differ even though their raw revision is the same
- **AND** each retains its selected denominator, counts and provenance

#### Scenario: Origin or uncertainty provenance changes
- **GIVEN** the same evidence and period with another independently chosen origin or declared timing-bound provenance
- **WHEN** accepted timing is constructed
- **THEN** its accepted revision changes
- **AND** the original source coordinates and raw evidence remain intact

#### Scenario: A true fractional period is accepted
- **GIVEN** independently verified constant evidence at a fractional tempo
- **WHEN** the accepted record is constructed
- **THEN** it retains the fitted binary64 period without a binary32 BPM roundtrip or integer-tempo snap

### Requirement: Control Adoption Rejects Stale Source Request And Timing Intent
The system SHALL provide a non-realtime guard checking accepted records against
verified current binding and guard-specific tickets. New requests, replacement, unload, intent changes and
successful adoption SHALL revoke tickets. It SHALL reject wrap and preserve
state on failure. Manual/TAP/Legacy SHALL block automatic adoption. Live
publication SHALL require actual engine source/request/intent ownership from
the integration slice.

#### Scenario: An old result returns after a new request or source change
- **GIVEN** a ticket issued for an earlier source/request revision
- **WHEN** a newer request, replacement or unload occurs before adoption
- **THEN** the earlier result is rejected without changing accepted state

#### Scenario: Manual timing supersedes pending automatic work
- **GIVEN** a pending automatic timing ticket
- **WHEN** the current intent becomes manual, TAP or legacy restoration
- **THEN** the ticket cannot publish automatic timing
- **AND** returning to automatic intent does not revive the old ticket

#### Scenario: A ticket belongs to another guard
- **GIVEN** two guards with matching source bindings and revision counters
- **WHEN** one receives the other's ticket
- **THEN** it rejects the ticket and retains its current accepted state
