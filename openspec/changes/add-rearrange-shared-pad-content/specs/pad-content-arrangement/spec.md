## ADDED Requirements

### Requirement: Stable placement and movable content have separate authority
The system SHALL distinguish fixed PadSlotId/slot-binding epoch, movable ContentInstanceId/nonreused lifetime, shared immutable material/analysis/StemSet versions and current source/timing/window/native action authority.

#### Scenario: Equal material does not grant copied current authority
- **WHEN** A is copied or its slot reused with equal source bytes
- **THEN** the new content gets fresh identity/lifetime/current guards and genuine native ACK
- **AND** material equality, copied ACK or historical path rewriting grants no live authority

### Requirement: Musical copies are independent stopped equal users
The system SHALL copy all musical source/analysis/selected-stems/loop/excerpt/grid/timing/manualTAP/correction/base/extra/KeyLock/playback/GainEQ/current mask/custom/preset/mutes/retrigger intent into fresh stopped content, sharing only verified immutable data.

#### Scenario: Copy and transposition preserve immutable sharing and independent state
- **WHEN** musical content is copied, transposed or independently played
- **THEN** the following complete normative contract applies:

The system SHALL copy all musical source/analysis/selected-stems/loop/excerpt/grid/timing/manualTAP/correction/base/extra/KeyLock/playback/GainEQ/current mask/custom/preset/mutes/retrigger intent into fresh stopped content, sharing only verified immutable data.

Voices/cursors/DSP state/meters/progress/pressed/hold/job/native tokens SHALL NOT
be copied. Suitable input ranges SHALL share RAM backing, varied ranges MAY need
separate views and all DSP/settings SHALL remain independent. Copy/transposition
SHALL NOT alone produce new original/PCM/stem files or full-source pitch PCM.

#### Scenario: Once prepared drums bass melody vocals workflow
- **WHEN** prepared A is copied to several pads and each selects its own stems/loops/pitch
- **THEN** copies play independently with no valid-data analysis/decode/separation or file duplication
- **AND** changing any copy leaves A and the others unchanged, including after origin-bank deletion/reopen

#### Scenario: Complete empty-target Copy has its own native transaction
- **GIVEN** a loaded source content and an empty target slot have matching current identities
- **WHEN** a complete musical CopySnapshot is prepared off-thread
- **THEN** its immutable source and suitable PCM/stem backing SHALL be shared through independent verified custody without import, complete decode, separation or file duplication
- **AND** the target SHALL receive a fresh durable content identity and nonreused runtime lifetime, distinct from the source's current permissions
- **AND** actual Native acceptance SHALL establish a stopped target with the complete captured source/timing/stem/mask/GainEQ/key intent before Copy reports completion
- **AND** the source's voices, cursor, DSP/history and hold effects SHALL remain unchanged
- **AND** preparation/admission rejection before claim SHALL preserve target occupancy/current epochs/config and release only prepared target owners
- **AND** irreversible claim without observed acknowledgement SHALL retain transaction owners and visibly fence target/config conflicts until actual reconciliation
- **AND** ordinary source-load acknowledgement alone SHALL NOT certify the complete CopySnapshot
- **AND** a known unsupported retained config schema SHALL reject Copy before Native capture or writer reservation

#### Scenario: Earlier target parameters cannot rewrite the copied image
- **GIVEN** Gain/EQ or other scalar target edits were queued before an empty-target Copy reservation
- **WHEN** the Native callback claims and installs the complete Copy image before draining those parameters
- **THEN** parameter admission and callback coalescing SHALL reject edits bound to the predecessor target epoch
- **AND** target edits admitted during a pending Copy SHALL fail before queue, epoch or durable intent mutation
- **AND** own Copy acknowledgement SHALL describe the captured complete target settings even when the config settlement runs concurrently

#### Scenario: Portable activation requires the complete Native Copy interface
- **WHEN** a newly built portable runtime is checked before activation
- **THEN** its Native class SHALL expose capture, preparation, publication, reconciliation, finish and cancellation of complete pad Copy
- **AND** missing operations SHALL reject activation without constructing an engine or opening an audio device

#### Scenario: Closing waits for real Copy settlement
- **WHEN** application close is requested while a complete Copy owns preparation or publication
- **THEN** new Copy admission SHALL close and an unclaimed Native transaction MAY be cancelled immediately
- **AND** controller assignment images, config writer and retirement reservations SHALL remain owned until the actual preparation worker has returned
- **AND** a claimed transaction SHALL settle through its own complete ACK and normal durable commit before dependent controllers, assignment custody or the audio engine shut down
- **AND** an unobserved ACK or failed durable commit SHALL keep close pending with a visible error rather than imply cancellation or successful completion

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
