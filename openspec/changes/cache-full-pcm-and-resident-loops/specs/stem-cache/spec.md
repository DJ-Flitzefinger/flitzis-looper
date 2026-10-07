## MODIFIED Requirements

### Requirement: Stem Generation And Replacement Require An Inactive Pad
The system SHALL allow stem generation and adoption of a new complete stem set
only while the target pad is not currently playing.

If playback starts during generation, completed results SHALL wait for the pad
to stop and still pass current source/request/timing ownership before adoption.
Finite resident-window relocation MAY occur while playing only when it retains
the identical complete source and already accepted complete StemSet identity,
matching source/window revisions and proved shared read/native/history context.
This narrow relocation SHALL require transactional native ACK and SHALL NOT
introduce new generated samples, complete-set content or accepted timing evidence.

#### Scenario: Playing pad blocks generation
- **WHEN** stem generation is requested for a playing pad
- **THEN** the request is rejected or deferred outside the callback

#### Scenario: Pad starts during generation
- **WHEN** a pad begins playing before a new generated set completes
- **THEN** the new set cannot replace its active prepared audio
- **AND** the previous effective full-mix/stem audio remains valid

#### Scenario: Active same-set window relocation
- **WHEN** a live loop edit, seek or ALL needs another window of the same accepted StemSet
- **THEN** the matching finite full-mix/component transaction may adopt after native ACK
- **AND** old effective audio remains valid before adoption

#### Scenario: Different set cannot use the relocation exception
- **WHEN** a pending window supplies a different complete source or StemSet digest
- **THEN** active adoption is rejected and the inactive-only set rule remains in force

### Requirement: Stem Cache Is Pad-Scoped And Deletable
The system SHALL store project stem artifacts under the pad-labelled container
`samples/stems/#N/` for pads 1 through 216, with generation-specific ownership.

New complete sets SHALL publish as immutable generation directories with their
complete content marker written last; metadata SHALL name the exact generation.
Previously saved canonical sets MAY restore after existing full validation,
but their cleanup SHALL target only declared known files, preserving unknown
content and any newer generation in the pad container.

Delete Stems and unload SHALL immediately revoke the pad's tracked stem eligibility
and metadata. Physical deletion SHALL occur off-thread only after that generation's
last assignment and reader/job/queued/voice owner retires. A cleanup job SHALL
delete only resolved owned paths of its generation, preserve other pad/shared
owners and never delete a newer generation or external original. Any pad-container
removal SHALL require that no generation, reader or unknown content remains;
an empty container MAY remain as harmless directory metadata.

#### Scenario: Generated stems use the pad label directory
- **WHEN** inactive pad 1 generates a current stem set
- **THEN** its owned generation artifacts are under `samples/stems/#1/`

#### Scenario: Unload removes pad stems
- **WHEN** pad 1 unloads
- **THEN** its old stems immediately become ineligible
- **AND** retired generation files are deleted after their final reader off-thread

#### Scenario: Manual stem deletion preserves full-mix playback
- **WHEN** stems are deleted for a loaded pad
- **THEN** the pad remains playable through full mix
- **AND** only its owned retired stem generation is eligible for physical cleanup

#### Scenario: Old cleanup races a new source assignment
- **WHEN** a newer generation now owns files under the same pad-labelled container
- **THEN** the old cleanup cannot remove those files or the live container

#### Scenario: Complete generation becomes the selected set
- **WHEN** a current inactive-pad job completes its five validated stem artifacts
- **THEN** the complete marker and immutable generation become eligible together
- **AND** native current-source/timing adoption ACK still governs playback availability

#### Scenario: Rejected replacement restores a saved canonical legacy set
- **WHEN** a replacement is rejected after retirement of the previous canonical set was deferred
- **THEN** restoring the saved legacy assignment revokes retirement of only its six declared direct files
- **AND** cleanup of the failed newer generation remains scheduled after its last owner retires
