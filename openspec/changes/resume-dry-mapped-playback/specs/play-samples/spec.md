## ADDED Requirements

### Requirement: Dry mapped pause preserves the actual source position
Supported dry mapped playback SHALL pause through the real content-bound
transport and caused waveform-hold paths. Pause MUST make only the intended
voices silent without advancing their source trajectory. Resume MUST continue
from the exact saved fractional source position at its actual output frame,
then follow the same original map using the authoritative transport. It MUST
NOT catch up by skipping unheard source content or retrigger the loop.

#### Scenario: Resume follows a long silent pause
- **GIVEN** a qualified dry Total0 Key-Lock-OFF mapped voice is playing
- **WHEN** it pauses at output frame P and resumes at a later output frame R
- **THEN** no source reads are fabricated during the pause
- **AND** its first resumed source position is bit-identical to the saved position
- **AND** subsequent samples, map knots and exact loop wrapping remain partition invariant

#### Scenario: Repeated pause or resume is harmless
- **WHEN** an already paused voice receives Pause or a playing voice receives Resume
- **THEN** its source position, continuity fences and history remain unchanged
- **AND** unrelated sample IDs remain unaffected

### Requirement: Mapped continuation is preflighted for the complete affected cohort
The callback SHALL preflight every affected voice before changing any audible
cohort state. It MUST retain the actual original map/source/content owner and
validate the current supported dry tuple, master epoch, source continuity and
exact representability. Missing, stale, unsupported or unrepresentable state
MUST refuse the effect without partially changing the cohort. Existing content
placement/transport revision, caused-pause masks/generations and retirement
rules MUST remain authoritative.

#### Scenario: One participant cannot resume safely
- **WHEN** a targeted cohort includes an unsupported or stale mapped participant
- **THEN** no targeted voice partially resumes or advances
- **AND** no Native history or retained input is relabelled as a new mapped epoch

#### Scenario: A waveform release follows content movement
- **WHEN** a caused-pause ticket is released after content moves or its intent is superseded
- **THEN** the existing own-effect and complete-cohort checks determine its authority
- **AND** a stale ticket cannot resume replacement content or another hold's paused voices

### Requirement: Pause feedback distinguishes request from actual audio state
Control feedback SHALL distinguish an enqueued pause/resume request from its
callback-effective state. Unsupported, stale or rejected effects MUST NOT be
represented as successful audio state merely because enqueue succeeded. The
existing Native content/placement and own-ACK paths SHALL be reused.

#### Scenario: An admitted request cannot take effect
- **WHEN** the callback refuses a pause or resume after queue admission
- **THEN** user-visible effective playback state follows Native observation
- **AND** the requested state does not manufacture a successful pause/resume
