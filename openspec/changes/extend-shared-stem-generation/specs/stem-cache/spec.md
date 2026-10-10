## ADDED Requirements

### Requirement: E11-05 Offline Generation Is Independent Of Pad Playback
The system SHALL permit source-leased offline stem generation for a playing or
paused loaded pad after the new-path safety gates pass, while keeping disk
generation separate from that pad's fresh native adoption transaction.

Immutable source/job leases SHALL survive actual inference and complete
verification. Loading, unloading, conflicting analysis or generation intent and
stale source ownership SHALL remain guarded. Model loading/inference, file I/O,
JSON, hashing, Python/GIL/UI access, blocking locks, logging, unbounded work and
heavy allocation SHALL NOT occur in the realtime callback. The callback SHALL
continue using its existing available FullMix/stem source until own adoption.

#### Scenario: Playing pad generates without disturbing its voice
- **GIVEN** a current finite NormalLoop voice using FullMix or selected stems and a nonneutral KEYLOCK/rate context
- **WHEN** an admitted immutable-source job runs offline
- **THEN** current source position, loop geometry, rate, masks and actual Native/FIFO/filter chronology remain continuous
- **AND** unrelated shared-material users remain usable without callback inference or disk access

#### Scenario: Source changes or origin unloads during inference
- **GIVEN** a shared source job with independently current subscribers
- **WHEN** one subscriber unloads, replaces content or cancels its interest
- **THEN** its completion cannot mutate newer intent or consume a newer job
- **AND** surviving subscribers retain the job and its actual read leases

### Requirement: E11-05 New Stem Sets Adopt With Independent Native Acknowledgement
The system SHALL adopt a newly generated complete set during playback only
through a separately proved bounded transaction with fresh own source/request,
timing, window, actual voice/history permits and native acknowledgement.

Disk completeness and P5a selected-same-set residency SHALL NOT authorize new
generation adoption. Prospective fullmix/component coverage, actual Native/FIFO/
filter continuation and immutable reader ownership SHALL be verified before
effective change. Own ACK SHALL precede effective UI/mode publication. Pending,
stale, queue-full, unsupported-domain and failed adoption SHALL preserve previous
effective audio with visible status; no stop/retrigger or position jump SHALL be
used to claim successful live adoption. Old voice A SHALL retain its own frozen
source/set/timing/history even after current bank B changes.

#### Scenario: New set becomes effective on a running voice
- **GIVEN** a complete current verified replacement and matching finite NormalLoop permits
- **WHEN** that pad's own guarded new-set transaction is accepted
- **THEN** the existing bounded source-selection transition preserves source fraction, rate/ramp, loop and masks with actual nontrivial wet output
- **AND** later real native preparations/adoptions preserve continuous history and old readers retire only after their final use

#### Scenario: Another subscriber acknowledges first
- **GIVEN** equal material assigned to two pads with independent permits
- **WHEN** only the first pad acknowledges a prepared replacement
- **THEN** only its effective state can report the new set
- **AND** the second keeps its old effective audio and pending/error status until its own ACK

#### Scenario: Stale or unsupported new-set adoption
- **WHEN** source, request, timing, window, voice/history or capacity no longer matches, or the requested domain lacks a genuine coverage proof
- **THEN** native adoption rejects safely and the current FullMix/stem/KEYLOCK trajectory remains usable
- **AND** neither a complete disk descriptor nor a same-set residency permission bypasses the guard

### Requirement: E11-19 Selected Stem Model Has One Current Verified Set
The system SHALL maintain one current verified stem set per shared material and
atomically replace its complete five-WAV/five-PCM/common-descriptor selection
for a changed effective model/configuration without physically overwriting any
leased generation.

The selected descriptor SHALL bind full source identity, model/checkpoint/config
digests, output-affecting quality, rate/layout/extent and transform/alignment
revision. Verification and both artifact areas SHALL complete before selection
commit. Failure or Cancel before commit SHALL retain the old usable selection;
a postcommit Cancel SHALL report the committed result truthfully and detach only
pending interests. Current-model selection SHALL NOT expose a parallel model
library. Retained immutable predecessors SHALL protect independently assigned copies as well as old runtime/rollback readers until actual final retirement. Publishing one current material-model selection SHALL NOT force a non-requesting copy C off its separately selected V1 when requesting A adopts V2; C keeps V1 until explicit replacement/deletion or final release, without exposing a selectable parallel-model library.

#### Scenario: Changed model replaces the current set
- **GIVEN** one current verified set from model A
- **WHEN** the selected model B completes joint verification and atomic selection commit
- **THEN** material selection names one complete B set and each requesting current pad requires its own later effective adoption ACK
- **AND** A remains immutable and readable for actual old voice/history/job/rollback readers until they retire

#### Scenario: Replacement fails or is cancelled before commit
- **WHEN** a new generation fails WAV, PCM, descriptor or config publication, or Cancel wins before commit
- **THEN** the previous complete selected set remains usable and no mixed or partial generation becomes current
- **AND** only the failed job's contained private artifacts can retire after their actual readers end

#### Scenario: Model selection changes while work is running
- **WHEN** Settings changes the selected model during an admitted job
- **THEN** the job keeps its captured effective fingerprint
- **AND** its late completion cannot overwrite a newer current material/model intent

### Requirement: E11-20 Selected Model Fingerprint Prevents Duplicate Generation
The system SHALL disable Generate when a current valid source-bound set matches
the selected effective model/configuration fingerprint and SHALL enforce the
same duplicate decision in UI, controller, direct and batch request paths.

Duplicate complete requests SHALL return an explicit already-present no-op;
concurrent equal in-flight requests SHALL share one physical job with independent
interests. Selecting a different effective fingerprint SHALL enable generation,
and successful complete replacement SHALL disable it again. Legacy unknown
fingerprints SHALL NOT be silently promoted. Model-free validated restoration
SHALL remain usable without loading/downloading a selected model.

#### Scenario: Complete selected model already exists
- **WHEN** UI, a direct controller call or an all-loaded batch requests generation of the current valid selected fingerprint
- **THEN** no new inference or artifact writes start and the action reports Stems already present
- **AND** the Generate button is disabled for that valid selected set

#### Scenario: Model change and successful return
- **GIVEN** valid model A stems
- **WHEN** model B is selected, then B generation successfully commits
- **THEN** Generate is enabled before B completion and disabled after current B verification
- **AND** selecting A again permits a new replacement without retaining a selectable A/B library

### Requirement: E11-19 Stem Retirement Waits For Actual Last Readers
The system SHALL retire replaced or deleted stem files off-thread only after
their actual final all-bank assignment, job/subscriber, action/version, queued
publication, native unload ACK, bank/voice, Native/FIFO/history and retained
complete reader releases its own ownership.

Admission SHALL reserve bounded rollback/retirement capacity before changing
intent, and new owners SHALL be acquired before old owners release. Cleanup
SHALL serialize with reader admission, check resolved containment, retry sharing
failures and preserve unknown, external, newer-generation and still-owned files.
Original FullMix/source PCM SHALL remain usable after stem deletion.

#### Scenario: Replacement keeps an old native history reader
- **WHEN** all visible pads select a new set but an old voice or native history still reads the predecessor
- **THEN** predecessor WAV/PCM/descriptor files remain immutable and protected
- **AND** off-thread cleanup occurs only after that actual final reader retires

#### Scenario: Cleanup races a new generation or fails sharing
- **WHEN** cleanup encounters a new generation, unknown file, external path, reparse point or sharing denial
- **THEN** it preserves non-owned content and defers/report failures through bounded control status
- **AND** no callback deletion or immediate recursive material removal occurs

## MODIFIED Requirements

### Requirement: Stem Generation And Replacement Require An Inactive Pad
The system SHALL separate source-leased offline generation from guarded native
new-set adoption and SHALL permit offline generation regardless of pad playback
after the E11-05 safety gates pass.

This inherited heading identifies the superseded main requirement. Activity
alone SHALL NOT block immutable offline work. A generated set SHALL change
effective audio only through fresh own source/request/timing/window/voice/history
permits and a separately proved bounded native transaction with its own ACK.
Unproved active new-set domains SHALL remain guarded. Same-set residency or
relocation SHALL NOT authorize new complete content. Failure, stale completion
or Cancel SHALL preserve current FullMix/stem playback and reader ownership;
no model inference or preparation SHALL execute in the callback.

#### Scenario: Playing pad starts offline generation
- **GIVEN** a playing pad with a current immutable source and no conflicting job intent
- **WHEN** generation is requested after offline safety proof
- **THEN** generation runs through the source-leased bounded background path
- **AND** current playback continues while later native adoption remains independently guarded

#### Scenario: Pad starts before generated result returns
- **WHEN** a pad begins playing before complete verification finishes
- **THEN** the verified result can commit on disk for still-current material/model intent
- **AND** effective new stems require the pad's own proved active transaction and ACK, or remain pending with old audio usable

## ADDED Requirements

### Requirement: E11-19 Independent copied assignments retain selected predecessors
The system SHALL preserve a non-requesting copied assignment's separately selected immutable predecessor after another equal material user commits a new model, without advertising multiple selectable model libraries or deleting the predecessor before actual final use.

#### Scenario: A adopts V2 while C remains on V1
- **GIVEN** A and independent copy C share verified V1
- **WHEN** A requests model B and commits V2 while C has no replacement request
- **THEN** only A's own accepted replacement SHALL change its effective selection
- **AND** C SHALL retain usable V1 files, intent and readers through origin deletion/reopen until its explicit replacement/deletion or actual final release
