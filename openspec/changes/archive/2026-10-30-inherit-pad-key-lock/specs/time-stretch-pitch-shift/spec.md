## MODIFIED Requirements

### Requirement: Per-Pad Key Lock State Drives Active Voices
The system SHALL resolve each loaded pad's requested Key Lock mode from one global boolean default
and that pad's nullable override: an explicit boolean SHALL take precedence, and `null` SHALL
inherit the current global default. One authoritative control-intent resolver SHALL serve UI,
persistence restore, new-source completion and live publication.

Global Key Lock changes SHALL update the default and publish changed effective requests only for
inheriting loaded pads. They SHALL NOT rewrite explicit overrides. Per-pad ON, OFF and inherit
requests SHALL change only the addressed loaded pad's override. Unloaded pads SHALL retain no
override and SHALL use disabled neutral live state until a successful source load is published.

Requested intent and confirmed Native live state SHALL remain separate. Failed enqueue, preparation
or Native acceptance SHALL retain the requested origin and previous confirmed Native state, and
SHALL expose failure rather than replacing an override with an observed audio boolean. Acceptance
and pending/error projections SHALL be associated with the matching pad/source request so stale
results cannot confirm newer intent. Updating intent SHALL NOT stop, retrigger, reload, regenerate
stems, reanalyze pads, time-slip active voices, or move source loop positions.

#### Scenario: Different pads use different Key Lock modes
- **GIVEN** Pad 1 and Pad 3 are playing
- **AND** Pad 1 has an accepted explicit ON override
- **AND** Pad 3 has an accepted explicit OFF override
- **WHEN** the performer changes global speed or BPM Lock creates a non-neutral tempo ratio
- **THEN** Pad 1 uses Key Lock pitch-preservation processing
- **AND** Pad 3 uses varispeed playback
- **AND** neither pad is retriggered by the mode difference

#### Scenario: Global update changes only inherited pad requests
- **GIVEN** loaded Pad 1 inherits the disabled global default
- **AND** loaded Pad 3 has explicit OFF and loaded Pad 4 has explicit ON
- **WHEN** the performer enables global Key Lock
- **THEN** Pad 1 requests enabled Key Lock while retaining a `null` override
- **AND** Pad 3 remains explicitly OFF and Pad 4 remains explicitly ON
- **AND** only changed inherited effective requests are published
- **AND** accepted active voices adopt the change without stopping or retriggering

#### Scenario: Per-pad update affects only one active pad
- **GIVEN** global Key Lock has been enabled
- **AND** Pad 3 and Pad 4 are playing and inherit that default
- **WHEN** the performer disables Key Lock for Pad 3
- **THEN** Pad 3 retains explicit OFF and uses varispeed after Native acceptance
- **AND** Pad 4 continues using Key Lock pitch-preservation processing
- **AND** the update uses bounded audio-engine state

#### Scenario: Clearing an override restores the current default
- **GIVEN** global Key Lock is enabled and loaded Pad 3 has explicit OFF
- **WHEN** the performer clears Pad 3's override
- **THEN** Pad 3 retains `null` intent and requests enabled Key Lock
- **AND** later global changes affect Pad 3 without recreating an explicit override
- **AND** the global default and every other pad's override remain unchanged

#### Scenario: Native rejection preserves requested origin
- **GIVEN** Pad 3's confirmed Native state is OFF
- **WHEN** the performer requests explicit ON and publication or preparation fails
- **THEN** Pad 3 retains explicit ON intent and the reported failure
- **AND** its confirmed Native state remains OFF until a matching acceptance is confirmed
- **AND** failure or telemetry does not rewrite the override to OFF or inheritance

#### Scenario: Per-pad update ignores unloaded pad
- **GIVEN** Pad 3 has no loaded audio
- **WHEN** the performer or controller requests Key Lock for Pad 3
- **THEN** Pad 3 retains a `null` override and disabled neutral live state
- **AND** no active voice can inherit stale Key Lock state from the unloaded pad
