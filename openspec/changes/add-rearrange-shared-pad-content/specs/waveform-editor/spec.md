## ADDED Requirements

### Requirement: Accepted hold releases bind the caused content pause
The system SHALL bind each admitted waveform PauseHold and its later matching release to the same ContentInstanceId, lifetime generation and native caused-pause effect, independently of its current PadSlotId or UI selection.

The bounded effect SHALL identify the native current playback cohort/control
revision that this hold actually changed from playing to paused. Material equality,
slot reuse and Python paused-set membership SHALL NOT confer release authority.
An already paused or stopped content SHALL create no owned pause effect. Duplicate
press/release observations for the same physical hold SHALL NOT create extra actions.

#### Scenario: HC-01 Basic caused pause resumes only its native cohort
- **GIVEN** A is playing and B is independently playing
- **WHEN** A's waveform right-Pause hold is admitted and actually pauses A
- **AND** its matching right-button release is accepted
- **THEN** A's owned cohort resumes once from retained position/history
- **AND** B's playback, DSP and pause state are unchanged

#### Scenario: HC-02 Already paused and stopped contents own no resume
- **GIVEN** A is already paused or is stopped
- **WHEN** right-Pause press and release are observed for A
- **THEN** no owned pause-effect token is created
- **AND** release neither resumes nor starts A or any other content

### Requirement: Surviving holds follow atomic Move and Swap
The system SHALL preserve the same accepted hold, lifetime and caused-pause effect when its living content participates in Move or Swap, and SHALL validate a later release against that identity at native application.

The native content projection, voice/DSP/history ownership and slot-binding epochs
SHALL change atomically; a resolved numeric slot alone SHALL NOT authorize release.

#### Scenario: HC-03 Move then outside release with reused old slot
- **GIVEN** admitted hold H actually paused A at slot s
- **WHEN** A moves atomically to empty t and B then occupies s
- **AND** H is released outside the original control
- **THEN** only A at t resumes its preserved owned cohort
- **AND** B at s is unchanged

#### Scenario: HC-04 Swap then outside release with paused other content
- **GIVEN** H actually paused A at s and B at t was independently paused
- **WHEN** A and B swap atomically
- **AND** H is released outside the original control
- **THEN** only A at t resumes without restart or DSP/history reset
- **AND** B at s stays paused

#### Scenario: HC-05 Repeated moves and swaps preserve one hold identity
- **GIVEN** H owns A's current paused cohort
- **WHEN** A participates in several successful moves/swaps before release
- **THEN** H follows that same instance through every complete native projection
- **AND** the single release cannot affect an earlier slot's later occupant

### Requirement: Removed lifetimes fence old releases and actions
The system SHALL terminally fence HoldRelease and all other accepted content-bound actions for every removed lifetime in the same guarded native transaction that deletes, reassigns or overwrites that content.

Successful source replacement SHALL use a fresh lifetime even for equal bytes or
equal paths. Failed preparation/admission SHALL preserve the prior content and
claim. Current successful unload's hold clearing SHALL be retained; the system
SHALL NOT reinterpret an old release using the removed slot's new occupant.

#### Scenario: HC-06 Delete then refill then outside release
- **GIVEN** H actually paused A at s
- **WHEN** admitted Delete removes A and C later fills s
- **AND** H's old outside-release is delivered
- **THEN** the old claim settles as retired/no-effect
- **AND** C is not resumed, paused, started, stopped or otherwise changed

#### Scenario: HC-07 Reassign equal bytes with new lifetime
- **GIVEN** H owns A's pause at s
- **WHEN** a successful source reassignment replaces A with fresh C, even with identical bytes
- **AND** a delayed old release or pause ACK arrives
- **THEN** its old action/lifetime is fenced and cannot change C or restore H

#### Scenario: HC-08 Copy overwrite retires only the old target claim
- **GIVEN** source A remains live and H actually paused target B
- **WHEN** Copy-overwrite atomically replaces B with fresh stopped C
- **AND** H later releases
- **THEN** C remains stopped and A is unaffected
- **AND** B's old action terminally retires without affecting C

#### Scenario: HC-09 All36 BankCopy includes empty-source target removals
- **GIVEN** source bank contains live A and target bank has held B in a corresponding empty source slot
- **WHEN** another-bank Copy commits the complete36-slot image
- **AND** the old hold for B later releases
- **THEN** B's lifetime is retired and that target stays empty
- **AND** nonempty copied targets are fresh stopped contents with no held tokens
- **AND** A and every source-bank hold remain unchanged

#### Scenario: HC-10 BankClear and slot reuse
- **GIVEN** H owns a pause in the clicked bank
- **WHEN** confirmed BankClear atomically removes that bank and its old slots are refilled
- **AND** H later releases
- **THEN** no refilled content is changed and the old claim settles as retired

#### Scenario: HC-11 Shared survivor outside cleared bank
- **GIVEN** held A outside the clicked bank shares immutable material with contents inside it
- **WHEN** confirmed BankClear removes the clicked bank
- **AND** A's hold releases
- **THEN** A resumes its own cohort and its shared data remain usable
- **AND** no Original-to-Copy dependency authorizes A's removal

### Requirement: Copy excludes transient holds and running state
The system SHALL create copied content with a fresh independent stopped instance and SHALL exclude accepted holds, pressed tokens, pause effects, live voice/cursor/DSP state and temporary job handles from its musical CopySnapshot.

Durable musical settings and suitable immutable source/analysis/stem/resident
backing SHALL remain shared or independently copied according to the full human
contract; this hold requirement SHALL NOT reduce that snapshot.

#### Scenario: HC-12 Copy of held source has no hold
- **GIVEN** H owns A's pause and musical settings
- **WHEN** A is copied to empty t
- **THEN** C has the musical snapshot, fresh guards and independent stopped DSP
- **AND** C has no H, pressed token, inherited pause, cursor or voice
- **WHEN** H releases
- **THEN** only A resumes and C remains stopped

### Requirement: Release remains observable outside the original controls
The system SHALL deliver a matching release or explicit input-cancellation request through the existing non-realtime input/control tick even when the original control is not hovered, rendered or selected.

Selection, waveform-editor target change, editor close and focus cancellation
SHALL NOT redirect the target or strand a still-live owned pause. Removal may
settle the old claim through guarded retirement instead of resumption. This
SHALL NOT introduce a second scheduler or persist physical hold state.

#### Scenario: HC-13 Selection/editor target changes before outside release
- **GIVEN** H caused A to pause
- **WHEN** selected pad and waveform editor target change to B
- **AND** right-button-up occurs outside both controls
- **THEN** the existing control tick releases H against A only
- **AND** B is unchanged even if B is paused

#### Scenario: HC-14 Editor closed or input cancelled while held
- **GIVEN** H owns a live pause of A
- **WHEN** the editor closes or input focus cancellation occurs before its normal render release
- **THEN** one matching release request is retained/delivered by the control path
- **AND** A resumes only if its owned effect is still current

#### Scenario: HC-15 Layout cancelled or admission fails before claim
- **GIVEN** H owns A's pause
- **WHEN** a drag is cancelled/self/invalid/empty-source or pair/bank mutation fails before native claim
- **THEN** contents, epochs, voices, holds, selection and owners are unchanged
- **WHEN** H releases
- **THEN** A resumes as before the attempted mutation

### Requirement: Release-before-ACK and capacity failure settle honestly
The system SHALL retain one bounded identity-bound release-requested record when release precedes pause-effect ACK or complete release admission, and SHALL settle it through the existing ordered native command/ACK path.

Admission failure SHALL NOT optimistically create a pause effect or discard the
only release obligation. Delayed/stale feedback SHALL NOT resurrect a consumed
or retired claim. Claim-without-ACK SHALL retain declared old/new pins and fence
conflicting input rather than invent a completed layout or rollback.

#### Scenario: HC-16 Release received before pause effect ACK
- **GIVEN** H's complete guarded pause has been admitted but effect ACK has not arrived
- **WHEN** its matching release arrives
- **THEN** the same record retains release-requested state
- **AND** ordered application either resumes only H's actually caused cohort or reports no-effect/retired
- **AND** no owned pause remains stranded after terminal native settlement

#### Scenario: HC-17 Release queue saturation retries same identity
- **GIVEN** H owns A's pause and release admission capacity is temporarily full
- **WHEN** release is observed repeatedly
- **THEN** one bounded release-requested record and its guards/pins are retained with visible pending/error state
- **AND** later complete admission releases A only once or settles retired
- **AND** no new occupant or new accepted pitch attack is coalesced into H

#### Scenario: HC-18 Release versus layout commit both linearization orders
- **GIVEN** H owns A's pause and a Move/Swap/removal is concurrently prepared
- **WHEN** release and layout commit execute in either deterministic order
- **THEN** release affects the same surviving owned cohort or safely settles removed lifetime
- **AND** no replacement receives the old effect and all complete projections match native ACK

#### Scenario: HC-19 Unload failure and delayed/stale ACK
- **GIVEN** H owns A and native unload capacity is full
- **WHEN** unload is rejected before authority revocation
- **THEN** A, H and all source/session ownership remain intact
- **WHEN** a later successful removal occurs and old pause/release ACK arrives
- **THEN** feedback cannot resurrect H or paused state at a replacement slot

### Requirement: Later accepted transport intent supersedes an old pause claim
The system SHALL invalidate an old HoldRelease pause claim when a later accepted STOP, retrigger, explicit resume, intentional pause or cohort-replacing action supersedes that effect.

Only operations preserving the exact native cohort/effect may keep its claim.
Release SHALL NOT undo later intent or alter a newly created voice, even when the
musical ContentInstance and source bytes remain equal.

#### Scenario: HC-20 STOP/retrigger/manualPause/resume wins over old release
- **GIVEN** H caused a pause for A
- **WHEN** a later accepted STOP, retrigger, explicit resume or intentional pause supersedes that effect
- **AND** H's old release arrives
- **THEN** that old claim settles without changing the later transport intent or new cohort

#### Scenario: HC-21 Continuity-preserving native refresh preserves owned pause
- **GIVEN** H owns A's paused native cohort
- **WHEN** same-source timing/metadata or resident relocation preserves that exact cohort and effect
- **THEN** native continuity guards prove H remains owned
- **AND** H's release resumes only that cohort without position/DSP/history reset
- **AND** a refresh that replaces the cohort instead invalidates H

### Requirement: MIDI input does not invent or redirect holds
The system SHALL preserve the existing NoteOff/NoteOn-velocity0 nonattack/nonreset semantics and fixed-slot binding layout while future accepted pitch actions bind their captured selected ContentInstance and lifetime through direct, fallback and pending execution.

No new MIDI PauseHold action is introduced by this plan. NoteOff SHALL NOT resume
a waveform hold or reset chosen pitch/highlight. Accepted attacks SHALL remain
distinct under the existing Quantize/SYNC event semantics; preparation alone may
coalesce. Unaccepted queued events SHALL have an explicit authoritative admission
point rather than be falsely described as already content-bound.

#### Scenario: HC-22 MIDI direct/fallback/pending capture versus mutation
- **GIVEN** selected A's immutable pitch/action tuple is captured at authoritative admission through direct native or controller fallback
- **WHEN** selection changes, A moves/swaps, or its lifetime is removed/reassigned before execution
- **THEN** the admitted action follows live A or fences the removed lifetime
- **AND** retries do not late-resolve another selected pad or slot occupant
- **AND** each accepted repeated attack remains a distinct existing scheduler event

#### Scenario: HC-23 MIDI release preserves pitch and waveform hold
- **GIVEN** a chosen pitch remains highlighted and waveform hold H owns a pause
- **WHEN** NoteOff or NoteOn velocity0 is received on existing device/channel routing
- **THEN** it causes no attack, pitch reset, highlight reset or waveform HoldRelease
- **AND** later matching mouse release still settles H correctly

### Requirement: Related pressed projections and last-user release remain bounded
The system SHALL preserve distinct content, gesture, global-output and immutable-resource ownership, and SHALL retire material only after every assignment, voice, reader, job, accepted action and native ACK owner has ended.

Move/Swap SHALL acquire new references before retiring old ones. Copy SHALL NOT
clone pressed/session authority. Native callback work SHALL be fixed/bounded and
free of Python/GIL/UI, blocking locks, I/O, inference, heavy allocation/destruction
and unbounded scans; all large preparation/cleanup SHALL stay off the callback.

#### Scenario: HC-24 Drag release and global mute regression
- **GIVEN** Re-Arrange and an existing output-global middle-hold mute
- **WHEN** pad drag hovers over a bank and releases, or a held content moves
- **THEN** no normal pad trigger/stop or bank action leaks from that drag
- **AND** global mute remains global and restores the latest intended volume on its own outside-release
- **AND** copied visual pressed state confers no action authority

#### Scenario: HC-25 True last user includes accepted actions and native unload ACK
- **GIVEN** copied contents across banks share material and an old voice/reader/job/action still owns an immutable version
- **WHEN** assignments are removed or overwritten
- **THEN** new refs are secured before old refs are dropped and shared jobs retain remaining interests
- **AND** files/PCM are reclaimed only after genuine final voice/reader/job/action/native-ACK retirement
- **AND** external/private user files remain protected

#### Scenario: HC-26 Shutdown/reopen and paused feedback projection
- **GIVEN** a live or awaiting-ACK hold exists while source-bound telemetry/restore state is delayed
- **WHEN** shutdown settles native stop/retirement and the project reopens
- **THEN** no physical hold or historical ACK is restored as current authority
- **AND** saved musical copies and immutable versions restore independently of deleted origins
- **AND** late slot-only started/stopped/paused feedback cannot mutate a different lifetime

Every scenario remains a future OPEN requirement. Actual software tests must
bind source/fixture/native EXE/installed PYD roles and record counts, exclusions,
negative attempts and native ACK/effect identity. R2/R4 proof precedes layout
release; P6/V0 revalidate resource/performance and the newer pitch workload.
Distinct nonauthor slice closures and real H-LIVE/H-FINAL evidence remain required.
