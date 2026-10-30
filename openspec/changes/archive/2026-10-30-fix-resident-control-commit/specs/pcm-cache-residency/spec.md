## ADDED Requirements

### Requirement: Resident physical controls mutate DSP only after transaction ownership
The callback SHALL acquire the original resident transaction's placement and publication claim before exchanging live Native handles, clearing FIFO or source history, resetting EQ, or applying a physical loop or seek cut. Reserve readiness SHALL be preflighted without destructive mutation, and claimed exchanges SHALL use the existing bounded ownership lanes without allocation, waiting, or Native preparation.

#### Scenario: An unrelated placement writer wins after callback preflight
- **GIVEN** a playing resident source with a valid loop or seek transaction and existing DSP history
- **WHEN** an unrelated writer holds placement publication after callback preflight but before the transaction claim
- **THEN** the callback defers the original payload without changing live DSP owners, FIFO, source history, EQ or playback PCM
- **AND** after the writer finishes, that same payload can acquire its own claim and apply the requested control

#### Scenario: Original permit is cancelled after callback preflight
- **GIVEN** a valid resident loop transaction for a playing source
- **WHEN** its original permit is cancelled before the placement claim
- **THEN** the transaction is rejected without a destructive DSP cut or mutation of the current voice
