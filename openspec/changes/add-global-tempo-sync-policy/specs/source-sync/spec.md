## ADDED Requirements

### Requirement: Global tempo tracking and SYNC remain independent choices
The application SHALL expose independent global Realtime BPM and SYNC controls
through GUI and Learn/MIDI using the same controller path. Their persisted strict
choices and checked revisions SHALL remain independent of PAD preferences, map
selection, Key Lock, requested BPM Lock, speed and trigger quantization. Source
tempo tracking SHALL NOT change the established master tempo or add another clock.

#### Scenario: The user selects fixed BPM
- **WHEN** Realtime BPM is switched off
- **THEN** timing policy SHALL use the original current constant detected, manual or TAP timing rather than the current variable-map segment
- **AND** selected maps, raw evidence and corrections SHALL remain intact
- **AND** master tempo and unrelated requested controls SHALL remain unchanged

#### Scenario: Grid tracking remains enabled while audio SYNC is disabled
- **WHEN** qualified variable source geometry is selected and only global SYNC is OFF
- **THEN** Realtime BPM MAY continue to supply the source grid and explicit automatic-bar extent without BPM matching the audio
- **AND** missing variable geometry SHALL remain unavailable as a grid rather than inventing a successful qualification
- **AND** unsynchronized playback MAY retain its existing known physical region without requiring unused map custody

### Requirement: Global SYNC resolves neutral content without erasing preferences
Explicit global SYNC OFF SHALL disable synchronization requests for all PADs.
Global ON SHALL enable eligible neutral PADs while respecting explicit PAD
opt-outs. Only false/revision0 SHALL be neutral; valid explicit true/revision0
SHALL retain its intent. Global actions MUST NOT rewrite per-content revisions.
Historical projects without global choices SHALL retain their prior behavior.

#### Scenario: An inherited PAD is excluded
- **WHEN** global SYNC is ON and the user switches a neutral PAD OFF
- **THEN** a checked explicit PAD OFF revision SHALL be recorded
- **AND** other PAD requests and all global choices SHALL remain unchanged

#### Scenario: A historical project is restored
- **WHEN** a project without new global policy metadata loads beside saved PAD requests
- **THEN** untouched PADs SHALL NOT become synchronized automatically
- **AND** prior explicit ON and OFF preferences SHALL remain behavior-compatible
- **AND** historical complete image bytes and seals SHALL remain unchanged

### Requirement: Explicit policy selects fixed variable or unsynchronized playback
Global SYNC OFF SHALL bypass both mapped adaptation and scalar BPM matching while
retaining master tempo. Explicit SYNC ON with Realtime BPM OFF SHALL use fixed
source-period/master-period matching. Both ON SHALL require the existing qualified
variable map. Missing or unsupported variable geometry MUST NOT silently select
fixed matching. GUI, MIDI, runtime publication and loop/grid resolution SHALL
consume the same policy.

#### Scenario: All four explicit combinations are used
- **WHEN** the two global choices are independently set to every ON/OFF combination
- **THEN** OFF/any SHALL remain unsynchronized, ON/fixed SHALL use current initial constant timing and ON/variable SHALL require qualified variable geometry
- **AND** existing source, rate, pitch, preparation and renderer guards SHALL remain enforced

#### Scenario: Fixed matching cannot reach the requested ratio
- **WHEN** current source/master timing is unavailable or its required unclamped ratio is outside the supported range
- **THEN** explicit fixed synchronization SHALL remain unavailable rather than acknowledging a clamped approximation
- **AND** a scalar rate glide SHALL NOT count as settled matching before the actual adopted rate has settled

### Requirement: Global policy adoption uses original Native ownership and ACK
Captured future starts and their cancellation fences SHALL include current global
policy identity. Actual fixed, variable and disabled observations SHALL come from
the callback-owned voice. A changed global request MUST NOT inherit an older
policy's ACK. Active old audio MAY remain until supported actual adoption or
stop/retrigger, with truthful pending state and retained original owners.

#### Scenario: Policy changes while a mapped loop remains active
- **WHEN** global SYNC is disabled or tempo mode changes before a supported replacement is adopted
- **THEN** the new choice SHALL remain requested/pending
- **AND** feedback SHALL retain the actual original voice policy and ownership until real transition or stop
- **AND** OFF/ON toggling SHALL NOT acknowledge the new request from an old equal-looking voice

#### Scenario: A global request encounters reserved arrangement rows
- **WHEN** whole rows remain owned by a pending Pair or Bank ACK/mirror
- **THEN** their complete previous publication SHALL remain intact
- **AND** no partially updated cohort SHALL be reported as globally adopted

### Requirement: New global policy preserves strict persistence and open gates
Python, Native and Portable project validators SHALL agree on strict optional
global metadata and comparison-only historical omissions. Malformed policy SHALL
retain original Config bytes and prevent unsafe overwrite while preserving other
valid performer settings. All automatic map qualification, B2 cutover, continuous
Native rendering, onset, resources and Human/device gates SHALL remain required.

#### Scenario: Invalid global policy is found on disk
- **WHEN** its types, fields, version or checked revision are invalid
- **THEN** restoration SHALL recover only understood settings without rewriting the original invalid evidence
- **AND** runtime recovery SHALL use visible safe OFF choices rather than interpreting invalid metadata as a successful original choice or Native ACK
- **AND** invalid PAD intent under a valid global ON SHALL recover to explicit safe OFF rather than inherited ON
- **AND** the original save fence and Native source/adoption authority SHALL remain required
