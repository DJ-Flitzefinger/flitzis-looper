## ADDED Requirements

### Requirement: Replace a dry mapped master epoch without changing current phase
At an accepted dry mapped global rate boundary E, the system SHALL preserve the shared TransportTimeline master beat and bar phase, output-frame clock and each mapped voice's exact canonical source position. The replacement epoch SHALL retain that voice's original marker and creative phase and SHALL borrow its original immutable map. Every fallible projection and coverage check SHALL complete before effective stores. The system SHALL NOT reset, retrigger, implicitly anchor to a pad or move the original launch target.

For a voice that has completed qualified dry waveform-hold continuation under
resume-dry-mapped-playback, the same creative source phase MAY use an explicit
claim-frame projection preserving old Q(E) exactly. This representation SHALL
retain the original marker/map and the one actual transport; previous actual
read spans MUST keep their previous epochs. Never-paused epochs SHALL retain
their existing evaluation. No new rate admission while paused is authorized.

#### Scenario: Repeated master-period changes cross map knots and wraps
- **WHEN** qualified dry mapped voices accept repeated master-period changes at actual boundaries
- **THEN** their source positions at each boundary SHALL remain unchanged
- **AND** subsequent coordinates SHALL follow the new piecewise authoritative master beat through knots and wraps independently of caller partitions
- **AND** historical actual-read receipts SHALL retain their own exact epoch rather than being reinterpreted through the latest epoch

### Requirement: Apply the actual master period once
Mapped playback SHALL derive tempo solely from the actual master beat trajectory. A coupled speed/master-period request SHALL NOT multiply the stored speed into that trajectory a second time. A speed-only observation change with unchanged actual master period SHALL preserve mapped coordinates and epoch. Scalar dry peers SHALL retain their established rate glide; mapped coordinates SHALL follow the accepted master slope directly.

#### Scenario: Speed observation changes without a new master period
- **WHEN** the stored speed changes while the actual master period is unchanged
- **THEN** mapped source reads and the actual epoch SHALL remain unchanged
- **AND** the system SHALL NOT infer another transport tempo from the speed observation

#### Scenario: A queued start belongs to the previous actual epoch
- **WHEN** a pending mapped start reaches its original target after a real master-epoch replacement
- **THEN** its unchanged original capture SHALL refuse through the existing stale-epoch checks without moving its target
- **AND** an equal-period or repeated tuple SHALL NOT falsely re-anchor or invalidate that target
