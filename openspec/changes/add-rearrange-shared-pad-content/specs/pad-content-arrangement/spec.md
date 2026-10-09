## ADDED Requirements

### Requirement: Stable placement and movable content have separate authority
The system SHALL distinguish fixed PadSlotId/slot-binding epoch, movable ContentInstanceId/nonreused lifetime, shared immutable material/analysis/StemSet versions and current source/timing/window/native action authority.

#### Scenario: Equal material does not grant copied current authority
- **WHEN** A is copied or its slot reused with equal source bytes
- **THEN** the new content gets fresh identity/lifetime/current guards and genuine native ACK
- **AND** material equality, copied ACK or historical path rewriting grants no live authority

### Requirement: Musical copies are independent stopped equal users
The system SHALL copy all musical source/analysis/selected-stems/loop/excerpt/grid/timing/manualTAP/correction/base/extra/KeyLock/playback/GainEQ/current mask/custom/preset/mutes/retrigger intent into fresh stopped content, sharing only verified immutable data.

Voices/cursors/DSP state/meters/progress/pressed/hold/job/native tokens SHALL NOT
be copied. Suitable input ranges SHALL share RAM backing, varied ranges MAY need
separate views and all DSP/settings SHALL remain independent. Copy/transposition
SHALL NOT alone produce new original/PCM/stem files or full-source pitch PCM.

#### Scenario: Once prepared drums bass melody vocals workflow
- **WHEN** prepared A is copied to several pads and each selects its own stems/loops/pitch
- **THEN** copies play independently with no valid-data analysis/decode/separation or file duplication
- **AND** changing any copy leaves A and the others unchanged, including after origin-bank deletion/reopen

### Requirement: Pair mutation preserves living playback and selection
The system SHALL apply Move to empty or Swap to occupied target as one guarded native/content/project transaction preserving both surviving lifetimes, voices, cursor, DSP/history/FIFO/filter/ramps and selected content without restart.

#### Scenario: Move and swap two running contents
- **WHEN** left drag moves A to emptyT or swaps A with B
- **THEN** moved source becomes empty or both contents exchange placement atomically
- **AND** live playback and paused cohorts follow their content, selection/editor follows selected content
- **AND** fixed slot MIDI/controller layout stays unchanged

### Requirement: Copy overwrite removes only the old target
The system SHALL leave source content/playback/holds unchanged when copying to empty or occupied target and SHALL stop/fence only the removed target lifetime in the same atomic commit creating a fresh stopped copy.

#### Scenario: Playing source copied over playing target
- **WHEN** right drag copies A over B
- **THEN** A continues unchanged, B stops/retires and newC remains stopped with independent settings
- **AND** later B action/release/telemetry cannot affectC

### Requirement: Whole bank mutation is one complete guarded image
The system SHALL copy or clear exactly36 target slots including empty source slots through one prepared atomic native/project/config/journal transaction with all refs/capacity reserved before any old reference is released.

#### Scenario: Mixed source bank and live target bank
- **WHEN** confirmed BankCopy commits all36 from sourceS to otherT
- **THEN** nonempty copies are fresh stopped and empty source slots remove corresponding targets
- **AND** only removed target playback/holds retire; source voices/holds/selection stay unchanged

#### Scenario: Cancel failure or irreversible claim without ACK
- **WHEN** operation cancels or fails before native claim
- **THEN** occupancy/epochs/selection/voices/holds/owners/config stay unchanged
- **WHEN** claim becomes irreversible without observedACK
- **THEN** old/new/action pins remain and conflicts are fenced until genuine terminal recovery
- **AND** no guessed success/rollback, early release or36 partial unloads is reported

### Requirement: Arrangement failures and no-op gestures preserve state
The system SHALL treat cancelled/self/invalid/empty-source drops and failed pre-claim mutations as complete no-ops and SHALL reject unsafe contained paths or exhausted native/action/feedback/retirement capacity without partial mutation.

#### Scenario: Invalid drop while source is held
- **WHEN** a cancelled or invalid drop is attempted on heldA
- **THEN** layout/selection/playback/hold/ownership remains unchanged and normal release still settlesA
