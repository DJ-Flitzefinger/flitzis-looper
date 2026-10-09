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


### Requirement: Prepared StemSet versions have equal independent users
The system SHALL bind a complete prepared immutable StemSet version independently of its origin slot and SHALL give each content its own current source/timing/window eligibility and native ACK.

New versions SHALL leave existing users' selected data immutable. Removing one
assignment or Delete Stems request SHALL NOT cancel another interested subscriber,
delete shared files, or revoke a surviving content's accepted set. Physical leases
SHALL retire only after actual job/read/action/voice/native unload completion.

#### Scenario: Origin removed while copies use an old version
- **GIVEN** A and copied C use V1 and a replacement V2 is prepared for A
- **WHEN** A selects V2 or unloads
- **THEN** C retains V1, its independent masks/loops/DSP and fresh current authority
- **AND** V1 survives until every owner and actual reader/native ACK retires

#### Scenario: Last subscriber cancels a pending job
- **WHEN** one subscriber removes its assignment while another still wants the shared job
- **THEN** the job continues for the remaining interest and late results cannot change reused slots
- **AND** only the final interest may request cancellation; lease release waits for real read end
