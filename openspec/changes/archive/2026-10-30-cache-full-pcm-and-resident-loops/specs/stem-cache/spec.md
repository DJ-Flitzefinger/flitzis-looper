## MODIFIED Requirements

### Requirement: Stem Generation And Replacement Require An Inactive Pad
The system SHALL separate offline generation and verified disk publication from active new-set adoption: generation MAY proceed for a playing pad under an immutable source lease, while active adoption SHALL remain fail-closed until the E11-05 and E11-19 new-generation continuity, ownership and own-native-ACK gates are implemented and proved.
This pending target supersedes the historical heading's inactive-generation restriction. P5a first residency of an already selected committed set and same-set relocation SHALL retain their distinct guarded identities; neither SHALL authorize a new generation, new timing evidence or another user's result. Existing source/cache/set/ticket/request/geometry/window/voice/DSP/history/FIFO leases and native ACKs SHALL remain required.

#### Scenario: Background generation while playback continues
- **WHEN** a playing pad requests a different selected model
- **THEN** the background job MAY generate and verify private immutable artifacts while actual old FullMix/stems and Native/FIFO/filter audio continues
- **AND** job completion alone SHALL NOT replace effective live audio or report new resident readiness

#### Scenario: Safe active replacement requires its own proof
- **WHEN** a verified different set requests active adoption
- **THEN** current source/timing/window/voice/history permits, new/old capacity and each affected pad's own ACK SHALL precede a bounded continuous transition
- **AND** unsupported, stale, cancelled or failed adoption SHALL preserve actual old effective audio and visible pending/error state

#### Scenario: First residency is not generation replacement
- **GIVEN** a current selected complete disk set exists without resident components
- **WHEN** ALL is requested during FullMix playback
- **THEN** P5a SHALL require its same-selected-set ranges, permits and native ACK
- **AND** E11-05/19 new-generation gates SHALL remain separate and unproved by that path

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
