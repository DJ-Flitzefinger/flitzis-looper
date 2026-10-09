## MODIFIED Requirements

### Requirement: Persist Stem Cache Metadata
The system SHALL persist per-pad source-version and complete pad-owned stem
artifact references separately from transient residency and effective native mode.

Restore SHALL validate current source and complete immutable artifacts before
disk eligibility. Default restore SHALL NOT eagerly publish stem windows merely
because artifacts or saved ALL STEMS intent exist. Matching FullMix restoration
and native ACK SHALL remain usable while explicit or enabled-policy stem demand
prepares ranges. Rejection/missing/stale/incomplete artifacts SHALL report truthful
disk/pending/error/readiness state and preserve FullMix without erasing durable
ALL STEMS intent. Native resident and effective-mode feedback SHALL establish
their own current-bound completion; persisted files SHALL NOT store runtime ACKs.

#### Scenario: Current stem cache metadata restores as available
- **WHEN** saved metadata matches the restored source and complete verified disk set
- **THEN** it may become disk-eligible, separately from resident readiness
- **AND** native rejection preserves FullMix and durable desire

#### Scenario: Restored current stems are published after full mix load
- **GIVEN** complete current disk metadata, matching FullMix load and explicit or enabled preload demand
- **WHEN** required stem ranges finish preparation
- **THEN** publication follows matching FullMix source ACK and its own guarded native ACK
- **AND** without that demand default restore does not eagerly publish stems

#### Scenario: Rejected restored stems become unavailable
- **WHEN** native adoption rejects restored stem ranges
- **THEN** they cannot be reported resident-ready or effective ALL STEMS
- **AND** FullMix and saved ALL STEMS desire survive for visible retry

#### Scenario: Stale stem cache metadata is not restored as playable
- **WHEN** saved cache identity does not match the restored source
- **THEN** those stems cannot become playable on that source
- **AND** FullMix remains usable without erasing durable ALL STEMS desire

#### Scenario: Lazy restore preserves saved ALL STEMS
- **GIVEN** a project saves ALL STEMS and complete current stem artifacts
- **AND** startup preload is off
- **WHEN** its FullMix source restores successfully
- **THEN** only required FullMix PCM is resident by default
- **AND** ALL STEMS remains saved intent with FullMix effective until an explicit request is acknowledged

#### Scenario: Explicit click repeats saved desire
- **WHEN** ALL STEMS is clicked while the same desire is already saved and valid disk artifacts exist
- **THEN** the request prepares missing component windows and obtains fresh native ACKs
- **AND** equality of saved intent does not suppress preparation

#### Scenario: Rejected restore keeps intent and audio
- **WHEN** disk validation or native residency publication fails
- **THEN** the error is visible and FullMix remains usable
- **AND** durable ALL STEMS is retained for retry rather than silently reset

### Requirement: Persist Durable Stem Mix Preferences
The system SHALL persist durable per-pad FullMix/ALL STEMS desire independently
of selected disk artifacts, pending work, resident readiness and effective mode.

New/older projects without a preference SHALL default to FullMix. Passive restore,
validation/preparation failure and resource pressure SHALL NOT erase saved ALL
STEMS. Explicit FULL MIX, Delete Stems, unload and genuine source replacement
SHALL follow their intentional existing track-bound reset semantics. Momentary
solo/mute, enabled masks, progress, blocked reasons/errors and native ACK state
SHALL remain session-only.

#### Scenario: Stem mix preference round-trips
- **WHEN** a project saved with ALL STEMS is restored
- **THEN** its durable desire remains ALL STEMS
- **AND** FullMix stays effective until demanded valid readiness and mode adoption

#### Scenario: Older project defaults to full mix
- **WHEN** a project contains no saved stem preference
- **THEN** each pad defaults to FullMix

#### Scenario: Runtime stem progress is not persisted
- **WHEN** saving occurs during generation/preparation
- **THEN** progress, blocked reason and transient errors are not durable settings

#### Scenario: Runtime stem mask is not persisted
- **WHEN** saving occurs after performer mask changes
- **THEN** enabled masks remain session-only and durable mode is independent

#### Scenario: Session progress does not become durable intent
- **WHEN** a project is saved during pending ALL STEMS preparation
- **THEN** desired mode is saved independently
- **AND** progress, masks, transient errors and native completion tokens are not persisted

#### Scenario: Older project and explicit unload defaults
- **WHEN** an older project has no mode preference or a pad explicitly unloads
- **THEN** its mode uses FullMix defaults
- **AND** passive cache failure for another ALL STEMS pad does not reset that desire

## ADDED Requirements

### Requirement: Transactional pad asset migration preserves current intent
The system SHALL copy and verify existing original/PCM/WAV assets into pad-owned
immutable generations before atomic project-reference migration and SHALL use
genuinely fresh native source/timing ownership and ACK for changed path identities.

#### Scenario: Migration encounters a newer edit
- **WHEN** session settings or source intent changes after migration captures config
- **THEN** reference commit cannot overwrite that newer intent
- **AND** retry reconciles verified copies with the actual current project
- **AND** the journal SHALL preserve actual old/new lineage/current revision/raw analysis/manual/TAP/accepted records/markers/settings and serialize with autosave
- **AND** string-rewritten SourceVersion or historical acceptance SHALL NOT replace complete evidence verification/new adoption

#### Scenario: Crash or cancellation during copy and native adoption
- **WHEN** migration fails or restarts at a copy/commit/adoption boundary
- **THEN** only recognized owned staging is recoverable and previous references remain usable
- **AND** new runtime ownership is re-established by fresh ACK rather than journalled old ACK
- **AND** files leased by another pad/project/voice are not deleted
- **AND** rollback/retry/recovery SHALL preserve old config/assets until all referencing owners retire
- **AND** unconfirmed native claims SHALL stay fenced without invented safe rollback

#### Scenario: Original loss is not hidden by session reset
- **WHEN** migration finds a missing/corrupt original
- **THEN** it SHALL report loss and preserve available old assets/config
- **AND** it SHALL NOT silently run NEW analysis or restore historical settings to hide the loss

### Requirement: Persist deliberate startup preload and residency budget
The system SHALL persist global `preload_stem_loops_on_startup` and
`resident_pcm_budget_mib` in existing ProjectState/config with defaults false
and512 respectively for old/new projects.

The new aggregate budget SHALL accept integer128..16384 MiB separately from
timing/analysis/per-job bounds and constrain unique backing plus pending/live overlap.

#### Scenario: Old projects keep deliberate low-residency startup
- **WHEN** an older config lacks both fields
- **THEN** preload is off and the aggregate residency budget is512MiB
- **AND** existing preparation bounds and saved stem mode remain independent

#### Scenario: Deliberate RAM allowance round-trips
- **WHEN** the performer enables startup preload and selects a supported larger budget
- **THEN** those validated values persist through the existing config path
- **AND** invalid/noninteger/out-of-range budgets fail without applying an unbounded limit

#### Scenario: Lower allowance does not discard live readers
- **WHEN** a reduced budget is below already pinned ownership
- **THEN** it SHALL preserve pinned readers and show temporary over-budget state until safe retirement
- **AND** these fields SHALL use ProjectState without machine/credential configuration or persisted runtime readiness
