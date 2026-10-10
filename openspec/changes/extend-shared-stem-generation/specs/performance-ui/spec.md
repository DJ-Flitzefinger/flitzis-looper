## ADDED Requirements

### Requirement: E11-06 All Loaded Stem Actions Require Scoped Confirmation
The system SHALL expose Generate Stems of all loaded PADs and Delete Stems of
all loaded PADs in global Settings with warning confirmation binding their
all-bank captured content/material/set/model scope and affected pad counts.

Cancel before acceptance SHALL have no effect. A stale dialog SHALL NOT act on
new content, model or set versions. Generate SHALL show bounded progress, partial
success/error and a Cancel control whose status follows actual interest/process
settlement. Disk-ready, pending native adoption and effective stems SHALL be
distinguished; neither complete files nor another pad's ACK SHALL imply success
for an unacknowledged pad.

#### Scenario: Warning is cancelled or target changes
- **WHEN** the performer cancels the warning, or captured content/model/set identity changes before acceptance
- **THEN** Cancel mutates nothing and changed targets are skipped with explicit status
- **AND** newly loaded content is never a substitute target

#### Scenario: Batch completes only partly
- **WHEN** some targets succeed and others fail, remain pending or are cancelled
- **THEN** Settings reports their exact distinct states and retains successful committed results
- **AND** UI rendering performs no inference, long-file hashing or blocking wait

### Requirement: E11-07 All Loaded Stem Deletion Uses Confirmed Shared-Material Scope
The system SHALL delete stems for all still-matching warning-confirmed loaded
pad assignments across all 216 slots, deduplicating exact shared material/set
versions and selecting safe FullMix through the existing guarded native path.

Deletion SHALL revoke only targeted stem eligibility and pending interests, not
external originals, FullMix source PCM or newer/unknown/still-owned artifacts.
Active selections/transitions and old voices SHALL retain actual reader owners
through their bounded FullMix handover; physical cleanup SHALL wait for actual
last readers. Other non-target users SHALL remain unchanged, repeated deletion
SHALL be a truthful no-op and deferred cleanup failures SHALL remain visible.

#### Scenario: Shared stems are deleted during playback
- **GIVEN** multiple loaded pads in different banks share a current set and active or paused voices retain it
- **WHEN** the matching all-loaded deletion is confirmed
- **THEN** targeted current intent safely returns to FullMix without transport restart or source loss
- **AND** shared files remain until every actual voice/job/history/reader retires

#### Scenario: Old delete dialog races replacement
- **WHEN** a target receives a new source or stem generation after the deletion snapshot
- **THEN** deletion skips that new identity and preserves its current files and owners
- **AND** cleanup targets only exact contained retired versions with no recursive material overwrite
