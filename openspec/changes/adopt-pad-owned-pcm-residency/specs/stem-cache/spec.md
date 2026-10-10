## MODIFIED Requirements

### Requirement: Stem Cache Is Pad-Scoped And Deletable
The system SHALL store each immutable material version's five generated WAV artifacts beneath
`samples/materials/M<stable-id>/stems/` and aligned stem playback PCM beneath its
`.pcm-cache/`, with equal content assignments across slots1..216.

Immutable generation names and complete markers SHALL bind source/content/schema/
transform identities without overwriting leased files. Delete Stems and unload
SHALL revoke exact tracked eligibility immediately after control admission;
physical cleanup SHALL wait off-thread for final all-bank assignment/job/subscriber/action/queued/native-ACK/
voice/history/version owners. Legacy layouts SHALL migrate transactionally and SHALL NOT
remain a permanent obsolete legacy-cache wrapper or origin-pad dependency. Unknown files/newer generations and
other owners SHALL survive cleanup.

#### Scenario: Generated stems use the canonical material directory
- **WHEN** stopped pad1 generates a current set
- **THEN** WAVs use `samples/materials/M<id>/stems/` and aligned PCM uses `samples/materials/M<id>/.pcm-cache/`
- **AND** tracked source/set selection requires complete joint verification

#### Scenario: Generated stems use the pad label directory
- **GIVEN** an existing safe legacy set uses `samples/stems/#N/` before verified migration
- **WHEN** its matching legacy source and complete set are restored
- **THEN** the typed legacy reader preserves eligibility without aliases or rewriting leased artifacts
- **AND** new canonical material generation uses the material directory rather than creating another legacy container

#### Scenario: Unload removes pad stems
- **WHEN** a pad unloads
- **THEN** its old stem eligibility is revoked immediately after admission
- **AND** owned files retire off-thread only after final references/readers

#### Scenario: Manual stem deletion preserves full-mix playback
- **WHEN** stems are explicitly deleted for a loaded pad
- **THEN** only its owned final-reader retired generations are eligible for cleanup
- **AND** FullMix remains playable

#### Scenario: Material-owned generation commits a complete set
- **WHEN** an inactive pad1 finishes valid generation
- **THEN** five WAVs commit under `samples/materials/M<id>/stems/` and their complete PCM derivative set under `samples/materials/M<id>/.pcm-cache/`
- **AND** incomplete staging cannot become eligible or overwrite a live generation

#### Scenario: Deletion preserves FullMix and other generations
- **WHEN** the performer deletes the selected pad's stems
- **THEN** its stem intent/eligibility is revoked while FullMix remains usable
- **AND** only its exact owned retired generation is deleted after final readers

### Requirement: Stem Generation And Replacement Require An Inactive Pad
The system SHALL permit inference/generation and selection or adoption of a
different complete stem content set only while the target pad is inactive.

If the pad starts during generation, its result SHALL NOT replace the active set.
First **residency activation** of an already committed, selected, validated disk
set MAY occur while FullMix plays, using a dedicated guarded transaction that
cannot select new content or adopt an unfinished/late generation. Same-set window
relocation MAY retain the already accepted complete identity while active.
Both exceptions SHALL prove source/cache/set/ticket/request/geometry/window/
accepted timing, current voice/DSP/history/FIFO coverage, leases and native ACK;
they SHALL NOT establish new accepted timing or waive active replacement guards.

#### Scenario: Playing pad blocks generation
- **WHEN** a playing pad requests generation
- **THEN** the request is rejected/deferred and no inference runs in the callback

#### Scenario: Pad starts during generation
- **WHEN** an inactive generation job's pad starts before completion
- **THEN** its newly generated content cannot replace active buffers
- **AND** old effective audio remains intact

#### Scenario: Active generation and content replacement stay blocked
- **WHEN** generation is requested for an active pad or a job completes after it starts
- **THEN** inference/adoption follows the inactive rule
- **AND** old effective FullMix/stem audio remains valid

#### Scenario: First residency while FullMix plays
- **GIVEN** a current selected complete disk set exists but no component window is resident
- **WHEN** ALL STEMS is explicitly requested during FullMix playback
- **THEN** the system prepares verified PCM ranges off-thread
- **AND** native guarded first-residency ACK precedes the existing continuous live transition

#### Scenario: Different cache set cannot use residency exception
- **WHEN** a prepared residency transaction names different content, source or set selection
- **THEN** active adoption is rejected without stopping or restarting FullMix
- **AND** a queued mode command cannot report effective ALL STEMS

### Requirement: Prepared Stem Buffers Are Aligned For Playback
The system SHALL commit complete immutable aligned f32 playback PCM derivatives
of all five WAV artifacts with the current complete FullMix source model.

Descriptors SHALL bind actual source/five-WAV/PCM digests, exact rate/layout/full
frames/origin, executed transform and one versioned shared alignment offset.
The joint set commit SHALL become eligible only after both WAV and PCM generations
commit and reverify, with a final common descriptor binding their identities.
Crash/retry/rollback between the two areas SHALL preserve old selected sets and
SHALL NOT expose half-complete pairs or overwrite leased files.
Fresh leases SHALL verify full integrity; already verified retained immutable
descriptors SHALL support direct source-range reads without another complete WAV
decode/conversion/alignment. Four component windows SHALL form atomic runtime
readiness. Instrumental derivative SHALL remain disk-only unless an explicit
offline consumer requests it; `I` and `A` masks SHALL keep their four-component
musical meanings and complete five-artifact integrity SHALL remain mandatory.

#### Scenario: Prepared stems share the full-mix frame origin
- **WHEN** a current complete stem set becomes eligible for native playback
- **THEN** its prepared four-component views share FullMix loaded rate/layout/complete extent/origin
- **AND** all use the existing source-frame loop/playhead model

#### Scenario: Changed loop reuses complete aligned PCM
- **WHEN** a loop changes for a pad with verified retained stem PCM descriptors
- **THEN** only its required source ranges are read/prepared with matching FullMix geometry
- **AND** no repeated full WAV conversion/alignment is needed
- **AND** other occupied pad windows retain their identities and usability

#### Scenario: Partial derivative or incompatible transform is rejected
- **WHEN** a PCM set lacks one derivative or mismatches source/rate/layout/alignment/schema
- **THEN** it cannot be reused or reported ready
- **AND** bounded rebuilding occurs off-thread without overwriting readers

#### Scenario: Crash between WAV and PCM area commits
- **WHEN** only one material-owned artifact generation committed before a crash
- **THEN** the pair has no eligible joint marker and cannot become resident-ready
- **AND** bounded retry/rollback recognizes exact owned generations while preserving old selected sets

#### Scenario: One executed loaded-rate offset binds all five artifacts
- **WHEN** the existing exact-geometry PCM16 conversion and shared alignment execute once
- **THEN** every persisted f32 derivative SHALL match that executed result bit-for-bit
- **AND** the signed offset SHALL use complete playback frames at the loaded output rate, separately from source_zero_frame
- **AND** the descriptor SHALL bind the supported conversion/alignment revisions, full source identity and exact five WAV/PCM lengths, digests and EOF
- **AND** incompatible WAV rate/layout/extent SHALL fail rather than imply an unexecuted resampling policy

#### Scenario: Instrumental corruption invalidates complete pair reuse
- **GIVEN** the instrumental derivative is not resident for live playback
- **WHEN** any instrumental WAV or PCM leaf is missing, replaced, truncated, nonfinite or differs from the complete descriptor
- **THEN** fresh pair reuse and four-component readiness SHALL fail before native publication
- **AND** old selected verified readers SHALL remain protected

#### Scenario: Common eligibility leaf commits last
- **WHEN** a verified WAV generation and its complete PCM generation are prepared
- **THEN** a strict common descriptor under the material's `.pcm-cache/stems/v1/.pairs/` SHALL become eligible only after both immutable areas have flushed, renamed, reopened and fully reverified
- **AND** neither area rename alone SHALL grant complete selection or playback eligibility
- **AND** compatible retries SHALL verify and reuse the same logical source/transform/five-artifact pair without duplicate creation rights
- **AND** unknown metadata, unsupported revisions, extra children and half-pairs SHALL remain protected rather than be adopted or recursively deleted

#### Scenario: Shared complete pair has independent callback authority
- **GIVEN** pads #1 and #216 select the same fully verified immutable pair
- **WHEN** each prepares its current four-component source window
- **THEN** each SHALL require its own current source/timing/window permit and native callback ACK
- **AND** a durable descriptor, shared PCM or another subscriber's ACK SHALL grant no publication permission
- **AND** `I` SHALL remain Drums + Melody + Bass while instrumental remains disk/offline

## ADDED Requirements

### Requirement: Generated artifacts do not imply resident stems
The system SHALL release temporary generation/validation PCM after durable
WAV/PCM commit and SHALL retain component windows only for explicit ALL STEMS
demand, enabled preload policy or existing valid reader ownership.

#### Scenario: FullMix generation with preload off
- **WHEN** generation completes for an inactive FULL MIX pad without stem demand
- **THEN** complete WAV/PCM artifacts remain durable on disk
- **AND** unused temporary component buffers retire without losing existing live readers
- **AND** disk completion alone does not report resident-ready or effective ALL STEMS

#### Scenario: Background pair preparation permits simultaneous UI control
- **GIVEN** one pad is playing while another pad prepares its complete stem pair
- **WHEN** the background worker performs artifact reads, conversion, alignment or writes
- **THEN** it SHALL hold captured immutable source readers and current publication fences without retaining a Python borrow of the AudioEngine
- **AND** ordinary UI message reception and control commands SHALL remain callable on that same engine throughout preparation
- **AND** the returned pair SHALL still require its own current source, timing and window validation before callback adoption

#### Scenario: Instrumental offline access is a real paired reader
- **WHEN** an explicit offline consumer requests instrumental from a selected verified pair
- **THEN** the consumer SHALL acquire and retain the same complete pair before reading its instrumental PCM
- **AND** this request SHALL NOT create a fifth live layer or imply ALL STEMS residency
- **AND** FULL MIX with preload off SHALL release temporary five-buffer conversion/alignment data after durable selection


### Requirement: Prepared StemSet versions have equal independent users
The system SHALL bind a complete prepared immutable StemSet version independently of its origin slot and SHALL give each content its own current source/timing/window eligibility and native ACK.

#### Scenario: Existing StemSet users retain their version and authority
- **WHEN** a new version is produced, one assignment is removed or one user requests Delete Stems
- **THEN** existing users' selected data remain immutable
- **AND** no other interested subscriber is cancelled, shared files deleted or surviving content's accepted set revoked
- **AND** physical leases retire only after actual job/read/action/voice/native unload completion

#### Scenario: Origin removed while copies use an old version
- **GIVEN** A and copied C use V1 and a replacement V2 is prepared for A
- **WHEN** A selects V2 or unloads
- **THEN** C retains V1, its independent masks/loops/DSP and fresh current authority
- **AND** V1 survives until every owner and actual reader/native ACK retires

#### Scenario: Last subscriber cancels a pending job
- **WHEN** one subscriber removes its assignment while another still wants the shared job
- **THEN** the job continues for the remaining interest and late results cannot change reused slots
- **AND** only the final interest may request cancellation; lease release waits for real read end

#### Scenario: A newer pair selection supersedes pending migration work
- **GIVEN** a subscriber has captured a saved complete pair for material migration
- **WHEN** its current StemCache selection changes before preparation or publication
- **THEN** migration SHALL preserve the newer selection and SHALL NOT publish the captured older components
- **AND** unresolved work SHALL retain its real owners and current project settings instead of granting authority from the saved selection

#### Scenario: FullMix intent waits for a running pair worker to return
- **WHEN** FULL MIX is requested after pair preparation has entered the bounded worker pool
- **THEN** settlement SHALL wait for actual worker return and retain the verified disk selection without unnecessary resident components
- **AND** a late old publication ACK SHALL NOT recreate resident demand after a successfully ordered FULL MIX release
- **AND** a later ALL STEMS request SHALL prepare current components and require its own publication ACK

#### Scenario: Weak history retires its historical source lease
- **GIVEN** historical pair descriptors retain a canonical source lease for living logical readers
- **WHEN** the final voice, queued set, job and retirement sink release that logical identity
- **THEN** bounded control reconciliation SHALL remove the dead historical descriptor and release its source lease
- **AND** saved selections and other actual readers SHALL continue to protect their independent paired artifacts

#### Scenario: Standalone pair recovery readers remain coupled
- **WHEN** a verified PCM or common-descriptor recovery reader is reopened
- **THEN** it SHALL reverify the complete source, five WAVs and five PCM artifacts and pin any recognized matching common eligibility leaf
- **AND** it SHALL NOT carry a saved source, timing or stem publication ACK
- **AND** a recognized PCM half-pair without common eligibility SHALL remain a retry target without playback eligibility

#### Scenario: Legacy migration prepares its complete pair off the frame thread
- **GIVEN** a verified available legacy WAV selection has no saved complete-pair descriptor
- **WHEN** material migration copies that selection into its canonical material
- **THEN** the ordinary bounded stem worker SHALL prepare and return a fully verified pair from the copied canonical WAV generation before migration saves its strict selection
- **AND** initial FULL MIX SHALL request disk-only preparation, await actual worker return and avoid live component publication
- **AND** ALL STEMS requested during disk-only work SHALL await that return before fresh component work with an independent publication ticket and ACK
- **AND** worker failures and newer stem selections SHALL preserve current intent and unresolved owners without synchronous frame-thread conversion
