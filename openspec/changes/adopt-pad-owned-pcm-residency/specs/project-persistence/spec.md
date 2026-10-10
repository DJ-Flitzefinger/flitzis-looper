## ADDED Requirements

### Requirement: Persist durable material and content identity
The system SHALL persist durable content-instance IDs and optional material IDs
beside exact project original references. New assignments SHALL receive fresh IDs;
valid restore SHALL preserve IDs without dirtying unchanged intent. Legacy identity
upgrade SHALL occur only after successful native restoration. Identity, artifact
kind and delivered owner admission SHALL precede durable tuple mutation; terminal
failure SHALL settle loading and preserve prior intent and ownership.

#### Scenario: Durable assignment reopens unchanged
- **WHEN** a project with matching original, material and content IDs reopens
- **THEN** those durable IDs remain equal after fresh runtime loading and native ACK
- **AND** unchanged restore alone does not dirty the project

#### Scenario: Legacy assignment acquires durable content identity
- **WHEN** a safe legacy original without content identity restores successfully
- **THEN** a fresh content ID with a legacy material binding is saved once
- **AND** existing source settings and original bytes remain unchanged

#### Scenario: Saved identity or delivered owner is rejected
- **WHEN** a saved material ID disagrees with its original or final owner admission fails
- **THEN** previous durable path, identity, settings and saved owner remain intact
- **AND** terminal loading/progress/request state is settled with a visible error
- **AND** saved material/artifact validation precedes native restore admission and delivered owner validation precedes project path/identity/settings mutation

## MODIFIED Requirements

### Requirement: Persist Stem Cache Metadata
The system SHALL persist per-pad source-version and complete shared material-version stem
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
SHALL follow their intentional existing track-bound reset semantics. Current musical enabled/custom masks, presets and mute choices, including choices
currently derived from SessionState, SHALL become independent durable content intent
and SHALL be included in CopySnapshot. Physical pressed/solo gesture tokens, progress,
blocked reasons/errors, temporary job handles and native ACK state SHALL remain transient.

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
- **WHEN** a project is saved with a runtime effective mask or temporary gesture override
- **THEN** that effective/gesture state is not stored as runtime authority
- **AND** the performer's durable musical mask and mix preference remain independently saved

#### Scenario: Musical stem selection round-trips independently
- **WHEN** saving or copying occurs after performer mask/custom-preset/mute choices
- **THEN** those musical choices round-trip as independent content intent
- **AND** changing the copy does not change its source or another material user

#### Scenario: Session progress does not become durable intent
- **WHEN** a project is saved during pending ALL STEMS preparation
- **THEN** desired mode is saved independently
- **AND** physical gesture tokens, progress, transient errors and native completion tokens are not persisted

#### Scenario: Older project and explicit unload defaults
- **WHEN** an older project has no mode preference or a pad explicitly unloads
- **THEN** its mode uses FullMix defaults
- **AND** passive cache failure for another ALL STEMS pad does not reset that desire

## ADDED Requirements

### Requirement: Transactional pad asset migration preserves current intent
The system SHALL copy and verify existing original/PCM/WAV assets once per distinct verified material into canonical
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

#### Scenario: Serialized current-reference commit
- **GIVEN** one journalled material transaction and its captured project revision
- **WHEN** preparation and fresh source/timing acknowledgements finish
- **THEN** the sole project writer SHALL build the related-reference snapshot from current performer intent and atomically commit it under the migration fence
- **AND** ordinary, debounced and shutdown saves SHALL obey that same fence, and a later dirty revision SHALL NOT be cleared by an earlier write
- **AND** content instance IDs and complete frozen key intent, including exhausted metadata epochs, SHALL remain unchanged without new analysis admission

#### Scenario: Actual acknowledgement survives superseded feedback
- **WHEN** native source work was claimed or acknowledged but metadata delivery is superseded or absent
- **THEN** the held actual adoption phase and source owner SHALL determine settlement, independently of an Error event
- **AND** target and rollback owners SHALL remain retained while outcome is unresolved, with dependent starts fenced
- **AND** a new process SHALL reverify journalled bytes and acquire new source and timing acknowledgements before treating the transaction as current

#### Scenario: One legacy material is reachable after normal startup restoration
- **GIVEN** startup captured an existing legacy original and its related current assignments
- **WHEN** ordinary source restores and owned stem jobs have settled
- **THEN** the application SHALL admit at most one distinct legacy material transaction at a time, including all matching current assignments across all banks
- **AND** canonical originals SHALL not start another migration
- **AND** changed current references, unknown journals, missing originals and unresolved claims SHALL remain visible and protected without repeated automatic retry
- **AND** this bounded admission SHALL not assert that remaining legacy materials or program-wide reconciliation are complete

#### Scenario: Remaining recognized legacy materials reconcile serially
- **GIVEN** a bounded startup inventory of current assignments in all banks
- **WHEN** one recognized material transaction and its bounded preparation worker have genuinely settled
- **THEN** the system SHALL recheck the next captured current reference and reconcile each remaining recognized legacy material serially without admitting two active material transactions
- **AND** newer paths and content identities SHALL win, canonical materials SHALL be skipped, and unknown or unresolved outcomes SHALL stop automatic progress visibly while preserving their assets
- **AND** final legacy retirement SHALL require the complete known reference/process inventory and true last physical owner rather than completion of only the first material

#### Scenario: Missing stems retain desire through original migration
- **GIVEN** a current assignment desires ALL STEMS but its saved stem set is unavailable
- **WHEN** its verified original and FullMix PCM migrate
- **THEN** the system SHALL preserve ALL STEMS desire and truthful unavailable stem metadata without separation or fabricated ready files
- **AND** corrupt files advertised as available SHALL visibly reject migration while old references and bytes remain protected

#### Scenario: Equivalent verified legacy stem sets share one target generation
- **GIVEN** related assignments reference different old stem directories with the same completely verified five WAV digests and compatible source, rate and geometry
- **WHEN** their material transaction prepares immutable target assets
- **THEN** the system SHALL reuse one verified canonical set generation, including concurrent subscriber preparation, without copying that set again
- **AND** different complete set contents or incompatible geometry SHALL remain separate
- **AND** complete WAV hashing and decoding SHALL run on bounded preparation workers rather than the host polling thread

#### Scenario: Journal intent belongs to the selected project config
- **GIVEN** two project configs in the same samples root contain identical bytes
- **WHEN** one config has a journalled unsaved migration intent
- **THEN** recovery SHALL bind that intent to the selected actual config reference and SHALL NOT apply it or its fences to the other known config
- **AND** missing or unrecognized project bindings SHALL remain visibly unresolved without restoring runtime permission

#### Scenario: Interrupted transaction resumes from current intent
- **GIVEN** a complete recognized journal bound to the selected actual config
- **WHEN** a new process reconciles its related current assignments
- **THEN** the system SHALL serialize a fresh child transaction under the same writer fence, reverify complete material bytes, and obtain its own source and Automatic timing acknowledgements
- **AND** the current content IDs, complete key metadata and current edits SHALL win; a different config digest SHALL NOT admit an old unsaved snapshot
- **AND** unknown, incomplete or exhausted journals SHALL stay visible and protected rather than creating runtime rights or being overwritten

#### Scenario: Durable artifact provenance is reverified before retirement
- **WHEN** rollback or target artifacts become candidates for recovery cleanup
- **THEN** the system SHALL reopen a complete bounded receipt containing the guarded samples-root identity, exact object identity and every recognized leaf identity, byte length and full digest
- **AND** target cleanup SHALL require actual created provenance; reused targets and unreceipted staging SHALL remain protected
- **AND** changed roots, files, contents, unknown children and partial generations SHALL reject cleanup without recursive deletion

#### Scenario: Global final use includes saved unavailable references
- **WHEN** a settled material transaction requests obsolete legacy retirement
- **THEN** the system SHALL inventory bounded known project configs and actual process owners under the same exclusive gate used by new process registration
- **AND** all saved original and stem references, including unavailable stem metadata, SHALL protect their files
- **AND** another live or unqueryable process, unknown inventory, a physical job, voice, queued or history reader, sealed reader or retry owner SHALL retain readable files until its actual last use ends
- **AND** the inventory gate SHALL remain held while queued retirement still waits for physical readers; only exact checked leaves and then checked empty containers may disappear off-thread

#### Scenario: Full recognized inventory resumes within existing metadata limits
- **GIVEN** 216 recognized interrupted material aliases and journals bound to the selected current project
- **WHEN** each material gains a genuinely committed current successor
- **THEN** the system SHALL replace only its matching parent alias within the existing 216-alias bound
- **AND** the successor SHALL durably retain all exact reverified artifact lineage before the recognized parent journal is compacted
- **AND** compaction SHALL recheck every current record and exact file identity and digest, preserving unknown, gapped, changed or unresolved history
- **AND** serial recovery SHALL stay within the existing bounded journal inventory without silently raising capacity or discarding pending material

#### Scenario: Published P2a history gains fresh old-material proof
- **GIVEN** an existing committed P2a journal has no artifact ledger
- **WHEN** its current selected config and alias still bind the transaction
- **THEN** the system SHALL freshly verify complete old Original, compatible PCM and old-source WAV artifacts before considering retirement
- **AND** historical paths and phases SHALL supply no runtime ACK or deletion permission
- **AND** new or reused target material SHALL remain protected even after its last current project reference is removed
- **AND** current ALL STEMS desire, unavailable metadata and every saved setting SHALL be preserved

#### Scenario: Physical queue completion outlives callers
- **WHEN** a fully verified artifact enters existing physical retirement
- **THEN** that actual queue entry SHALL retain its inventory gate independently of caller release or garbage collection until physical completion or terminal preservation failure
- **AND** a new saved assignment, delivered owner or reclaim SHALL cancel obsolete retirement with a visible terminal preservation outcome
- **AND** completion SHALL follow the exact leaf deletion and bounded empty-container pass; a pending reader or error SHALL NOT be reported as successful deletion

#### Scenario: Config inventory distinguishes access from replacement
- **WHEN** the known project reference inventory reads a config
- **THEN** it SHALL bind the actual opened file and current path identity and reject detected writes, replacements or reparse changes during that read
- **AND** an access-time update caused by reading alone SHALL NOT invalidate an unchanged config

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
