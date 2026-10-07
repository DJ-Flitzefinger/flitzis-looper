## MODIFIED Requirements

### Requirement: Loop region updates apply immediately during playback
The system SHALL accept live loop edits without an explicit apply/save action
and apply already resident, fully prepared edits through existing immediate
bounded native publication.

For a nonresident target or missing proved processing context, the system SHALL
prepare a finite matching transaction and expose pending/error separately from
effective playback. The old effective region/audio SHALL remain valid until
matching native adoption ACK. A stale or failed preparation SHALL not partially
apply new region, source offsets, timing or stems.

#### Scenario: Live loop updates
- **GIVEN** a pad is playing with all required target context resident
- **WHEN** the performer changes a loop marker
- **THEN** subsequent playback follows the updated region without stopping the pad

#### Scenario: Live edit moves outside resident context
- **WHEN** a live edit needs nonresident PCM or DSP state
- **THEN** the requested edit becomes pending while previous effective audio continues
- **AND** only matching ready adoption applies the entire edit

### Requirement: ALL sets explicit full-track loop region
The system SHALL provide ALL as explicit manual loop intent with start `0.0`,
end equal to full-source duration and `auto_loop_enabled = false`.

ALL SHALL use full-source metadata rather than resident window length and apply
immediately when complete required PCM/context is resident. Otherwise it SHALL
prepare an admitted complete-track resident exception and retain previous
effective audio until matching native adoption ACK.

#### Scenario: ALL stores full-track manual loop region
- **GIVEN** a loaded full source lasts 42 seconds
- **WHEN** the performer activates ALL
- **THEN** requested start/end are 0/42 seconds with auto-loop disabled
- **AND** native effective publication occurs when the matching full-track context is ready

#### Scenario: ALL is unavailable without loaded duration
- **WHEN** no valid full-source duration exists
- **THEN** loop intent remains unchanged and no native region update is sent

#### Scenario: ALL on a short resident loop
- **WHEN** ALL requires a complete-track exception
- **THEN** the UI reports pending preparation while the prior region plays
- **AND** failure preserves the prior effective state without pretending ALL was adopted
