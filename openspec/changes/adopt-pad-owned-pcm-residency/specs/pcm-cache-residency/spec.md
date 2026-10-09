## ADDED Requirements

### Requirement: Shared material complete PCM and retained range readers
The system SHALL own complete versioned FullMix PCM and all aligned complete
stem PCM beneath each immutable material version's `.pcm-cache` while
retaining original/content/rate/transform/schema/generation and assignment identities.

#### Scenario: Verified descriptors supply a new window
- **WHEN** a current pad requests a different proved finite range
- **THEN** complete source identity and retained descriptor lineage stay unchanged
- **AND** only requested ranges/required context are loaded under byte reservation
- **AND** adoption still requires guarded native ACK and final-reader retirement
- **AND** fresh leases SHALL verify complete integrity while retained verified descriptors supply ranges without full WAV decode/alignment or hidden complete f32 allocation
- **AND** compatible backing MAY share in-process with equal independent content owners without duplicated files
- **AND** reads/validation/allocation/retirement SHALL remain bounded and off-thread

#### Scenario: Cross-area complete set ownership remains coupled
- **WHEN** WAVs reside in the material version's `stems` and all PCM resides in that version's `.pcm-cache`
- **THEN** common descriptors SHALL bind both immutable generations/source/transforms/digests
- **AND** joint publication/final-reader retirement SHALL prevent half-complete eligible pairs

### Requirement: Complete 216-slot residency and local refresh
The system SHALL support actual usable acknowledged FullMix ranges for all216
occupied slots and four-component ranges for all216 eligible stem-demand slots
within a declared sufficient supported resource configuration.

#### Scenario: All216 unique pads reach actual readiness
- **WHEN** all216 occupied slots have valid bounded geometry and a sufficient declared budget
- **THEN** bounded scheduling eventually obtains actual ACKed usable windows for all216
- **AND** off/preload-on and explicit all-stems cases are independently exercised
- **AND** readiness requires no216-voice claim

#### Scenario: One edit preserves215 valid pad windows
- **WHEN** one pad's loop is changed after all216 are ready
- **THEN** only its affected windows prepare and adopt
- **AND** the other215 views retain handles/revisions and remain usable
- **AND** source/loop setup SHALL reconcile every occupied slot while an edit renews only affected read sets

#### Scenario: Resource failure is incomplete readiness
- **WHEN** a required window cannot fit configured bytes or owner capacity
- **THEN** it reports finite recoverable preparation/admission failure
- **AND** partial ready count cannot satisfy the all216 acceptance obligation
- **AND** requested/queued/preparing/ACKed/error and exact-byte totals SHALL remain distinct

### Requirement: Finite DSP coverage is a feature completion gate
The system SHALL prove and implement finite source-domain supply for the supported
loop/DSP context before claiming completion of shared-material loop residency.

#### Scenario: Dry parity is insufficient for KEYLOCK completion
- **WHEN** dry interpolation passes but finite native continuation remains unproved
- **THEN** the supported labelled fallback or prior effective audio remains available
- **AND** finite-DSP and complete-feature acceptance stay open
- **AND** admitted full-track KEYLOCK fallback MAY remain temporarily but SHALL NOT close finite supply

#### Scenario: Actual finite continuation is adopted
- **WHEN** a bounded native/history read plan proves all required source inputs and state
- **THEN** its complete-buffer regression oracle and causality checks precede native ACK
- **AND** no hidden callback disk read, long scan or missing-sample silence is introduced
- **AND** proof SHALL cover interpolation/P-H seam/rate smoothing/Rubber Band feed/lookahead/native/FIFO/history/filter/current and pinned voices/both transition sides
- **AND** guessed halos or unavailable future input SHALL NOT authorize readiness
- **AND** accepted P/integer H/one rate owner and acoustic/latency/1598-frame33.292-ms fixtures SHALL remain intact

### Requirement: Separate aggregate residency and preparation accounting
The system SHALL reserve unique resident backing and simultaneous pending/old
live resident allocations against the explicit aggregate ProjectState budget
before mutation while preserving separate existing bounded preparation limits.

#### Scenario: Shared backing and pinned overlap are accounted
- **WHEN** compatible pads share backing while an old window remains pinned
- **THEN** backing bytes are charged once with each owner retained
- **AND** new/old overlap is reserved without evicting live readers
- **AND** PCM byte counters are not reported as whole-process RSS
- **AND** exact PCM costs/conservative estimates/job overlap/measured process resources SHALL be separate

#### Scenario: Budget reduction meets active ownership
- **WHEN** a smaller budget is selected while pinned data exceeds it
- **THEN** old effective audio remains valid and above-budget state is visible
- **AND** idle retirement and new admission honor the bound without deleting active owners

### Requirement: Existing bounded lanes and exact ownership capacity
The system SHALL keep two workers,32 queued/reserved jobs,eight startup admissions
per poll,1GiB transient PCM per job and ordinary512MiB timing/analysis preparation.
Assignment/descriptor capacity SHALL cover216 plus bounded pending/old owners
with checked reservation, overflow rejection and off-thread retirement.

#### Scenario: Owner capacity is sufficient without multiplying voice lanes
- **WHEN** all216 unique FullMix and StemSet owners plus bounded overlap are exercised
- **THEN** actual owner accounting SHALL determine any justified capacity increase
- **AND** the96 native handles for32 voice lanes SHALL NOT be treated as a216-pad pool

### Requirement: Measured warm reuse and full lifecycle acceptance
The system SHALL support performance/RAM claims only with actual source/runtime-
bound paired measurements and causal path checks for the implemented feature.

#### Scenario: Warm trigger has no cold work
- **WHEN** valid resident PCM receives repeated unchanged starts with a saturated cold lane
- **THEN** genuine native ACK and guarded launch use the retained handles
- **AND** instrumentation shows no new disk/decode/alignment/preparation work
- **AND** measured startup or latency claims use their actual separate observations
- **AND** unchanged starts SHALL preserve delivered ACK/guarded reuse without a cold dependency

#### Scenario: Complete measurement scope is retained
- **WHEN** feature performance/RAM acceptance is evaluated
- **THEN** actual repeated runs SHALL include216 unique/shared content, preload off/on, cold/new-process/in-process warm, first live switch, ready triggers, single edits, disk/integrity and final-owner lifecycle
- **AND** actual logs/negative runs/process scope and human/device gates SHALL remain explicit


### Requirement: True last use governs shared material retirement
The system SHALL acquire all new assignment/action/version references before retiring old references and SHALL reclaim managed material only after every all-bank assignment, voice/old voice, reader, job/subscriber, queued action, history/FIFO and native unload ACK owner has ended.

Slot removal SHALL revoke playback authority without granting a queued old action
authority over a replacement. Cleanup SHALL resolve verified contained owned file
identities off-thread and SHALL preserve external/private/unknown files. Unconfirmed
irreversible native claims SHALL keep old/new pins and fence conflicting input.

#### Scenario: Swap and overwrite do not create a zero-owner gap
- **WHEN** contents sharing a material move/swap or a stopped copy overwrites a target
- **THEN** new refs/reservations are secured before old refs retire
- **AND** shared material never becomes temporarily eligible for deletion

#### Scenario: Assignment count zero still has a real reader
- **WHEN** the final assignment disappears while an old voice/job/action/native ACK is pending
- **THEN** bytes remain pinned until actual terminal/read end
- **AND** cleanup runs only after that last use, without changing external user files

#### Scenario: Copies share input without sharing DSP
- **WHEN** compatible copies request the same resident interval
- **THEN** verified immutable input backing is shared once in accounting
- **AND** each copy retains independent DSP/voice/settings and varied ranges remain valid separate views
- **AND** Copy/transposition creates no original/PCM/stem file duplicates or complete pitch PCM
