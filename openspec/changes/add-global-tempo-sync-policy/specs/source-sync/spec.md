## ADDED Requirements

### Requirement: Timing controls retain preferences under a shared parent hierarchy
The application SHALL resolve MULTI LOOP -> BPM LOCK -> SYNC -> REALTIME BPM through the existing GUI, Learn/MIDI, runtime and Native paths. Each child SHALL be unavailable for editing when its requested parent is unavailable or off. Stored BPM Lock, SYNC, Realtime BPM and content preferences SHALL remain intact while a parent is off. Control availability SHALL use qualified requested-parent state rather than an actual playing/settled acknowledgement, allowing configuration before playback. Key Lock, map selection, speed and trigger quantization SHALL remain separate controls. Source tempo tracking SHALL NOT add another master clock.

#### Scenario: A parent is disabled and enabled again
- **WHEN** MultiLoop or BPM Lock is changed while dependent preferences remain on
- **THEN** the complete newly resolved target SHALL be republished through the original authority without erasing child preferences
- **AND** reactivation SHALL freshly qualify the selected BPM-Lock reference, sources and current playback before adopting dependent effects
- **AND** Transport time and source playheads SHALL NOT be reset or sought to manufacture phase alignment
- **AND** an unsupported current representation SHALL retain preceding actual audio with truthful pending/refusal until safe conversion is qualified
- **AND** the existing control owner SHALL reconcile a deferred parent target when its blocking representation/rate work retires or the required reference becomes available, without blind queue flooding or another scheduler

#### Scenario: Reference qualification changes after BPM Lock ON is admitted
- **WHEN** an admitted ON request is still awaiting actual application and its original qualified reference changes or becomes unavailable
- **THEN** the deferred effect SHALL NOT borrow another content/source/accepted-timing qualification
- **AND** a Pair move of the same content SHALL retain its original identity, while genuine requalification SHALL require a new current receipt
- **AND** requested and actual BPM Lock SHALL remain separately observable without treating enqueue as effect

#### Scenario: BPM Lock OFF encounters an actual Fixed voice
- **WHEN** an OFF request remains pending while Fixed synchronization is still actual
- **THEN** the original policy owner SHALL first adopt the allowed Legacy representation or retire that voice before actual BPM Lock becomes off
- **AND** prospective tempo/pitch/source-history safeguards SHALL remain required for the final parent effect, preserving preceding actual audio while unavailable

#### Scenario: Parent changes return to equal-looking choices
- **WHEN** an accepted parent request changes OFF then ON without changing either stored child choice
- **THEN** a checked hierarchy request revision SHALL distinguish both changes and invalidate old captures and acknowledgements
- **AND** revision exhaustion SHALL refuse before Native or project mutation, unchanged setters SHALL remain no-ops, and reserved cohorts SHALL retain their complete preceding publication until their original cancellation or ACK boundary
- **AND** a hierarchy revision preflight SHALL preserve ordinary MultiLoop cancellation/supersession of an already reserved Pair/Bank launch

#### Scenario: The user selects fixed BPM
- **WHEN** Realtime BPM is switched off
- **THEN** timing policy SHALL use the original current constant detected, manual or TAP timing rather than the current variable-map segment
- **AND** selected maps, raw evidence and corrections SHALL remain intact
- **AND** master tempo and unrelated requested controls SHALL remain unchanged

#### Scenario: A remembered variable preference has an inactive parent
- **WHEN** MultiLoop, BPM Lock or the resolved PAD SYNC request is off
- **THEN** source grids, automatic extents and new audio targets SHALL use original static/Legacy timing rather than a remembered variable preference
- **AND** selected maps, corrections, raw preferences and physical source markers SHALL remain intact without requiring unused map custody
- **AND** disabled GUI and MIDI actions SHALL NOT silently edit the remembered child preference

### Requirement: Global SYNC resolves neutral content without erasing preferences
Explicit global SYNC OFF SHALL disable synchronization requests for all PADs.
Global ON SHALL enable eligible neutral PADs while respecting explicit PAD
opt-outs. Only false/revision0 SHALL be neutral; valid explicit true/revision0
SHALL retain its intent. Global actions MUST NOT rewrite per-content revisions.
Historical projects without global choices SHALL retain their raw prior preferences while the latest parent hierarchy controls their availability.

#### Scenario: An inherited PAD is excluded
- **WHEN** global SYNC is ON and the user switches a neutral PAD OFF
- **THEN** a checked explicit PAD OFF revision SHALL be recorded
- **AND** other PAD requests and all global choices SHALL remain unchanged

#### Scenario: A historical project is restored
- **WHEN** a project without new global policy metadata loads beside saved PAD requests
- **THEN** untouched PADs SHALL NOT become synchronized automatically
- **AND** prior explicit ON and OFF preferences SHALL remain stored and become effective only under the qualified parent hierarchy
- **AND** historical complete image bytes and seals SHALL remain unchanged

### Requirement: Resolved policy selects Legacy static or variable source timing
An unavailable or disabled SYNC request SHALL select the established Legacy path: original constant detected/manual/TAP timing and existing BPM matching only when actual BPM Lock is enabled. A remembered global SYNC preference SHALL NOT force matching while the parent is inactive. Qualified SYNC with Realtime BPM off SHALL select static-grid synchronization; both on SHALL require the existing qualified variable map. Fixed and mapped actual consumers SHALL require actual BPM Lock, not merely a retained master period. Native runtime projection SHALL resolve its captured requested MultiLoop and BPM-Lock parents before choosing a timing target while preserving raw intent across omitted-row retention; actual-parent guards alone SHALL NOT replace requested-parent resolution. Missing or unsupported variable geometry MUST NOT silently select fixed synchronization. GUI, MIDI, runtime publication, loop/grid and automatic timing publication SHALL consume the same resolved policy.

#### Scenario: Parent and child combinations are used
- **WHEN** parent availability and remembered child choices are resolved
- **THEN** inactive SYNC SHALL retain Legacy behavior, static SYNC SHALL use original constant timing, and variable SYNC SHALL require qualified geometry
- **AND** existing source, rate, pitch, preparation and renderer guards SHALL remain enforced

### Requirement: Grid phase confirmation is distinct from timing mode acknowledgement
Settled fixed ratio, mapped custody, map readiness and actual variable/static rendering SHALL remain observable mode facts but SHALL NOT certify common-grid phase alignment. Presentation SHALL retain a separate nullable phase qualification, unknown until an actual qualified Native algorithm supplies a current source/master/request witness. Requested active SYNC with mode-only evidence SHALL remain phase-pending/unconfirmed, preserving genuine refusal and tempo-settling guidance. Global SYNC and PAD labels SHALL NOT display complete synchronization from the earlier mode-only acknowledgement.

#### Scenario: A fixed target settles or a mapped mode is current
- **WHEN** existing callback evidence confirms the requested mode without a qualified common-grid phase witness
- **THEN** the UI SHALL distinguish applied mode from unconfirmed grid phase rather than display fully synchronized audio
- **AND** stale source, parent, map or request evidence SHALL NOT confirm either the new target or its phase

### Requirement: Static and variable synchronization share the unwrapped master
Completed static SYNC SHALL continuously align declared static grids; completed variable SYNC SHALL align the qualified variable source map to the same original unwrapped Transport reference. Eight-bar loops, sixty-four-bar loops and one-shots from their selected start to EOF SHALL retain a common beat reference with source-specific modulo only for repeating loops. The phase-convergence contract SHALL be defined and proved before phase confirmation; it SHALL preserve the actual activation cursor/first PCM, source ownership, both genuine start modes and the existing Key-Lock-OFF tempo/pitch coupling without periodic stop/restart or seek. Existing scalar period matching and anchored creative-offset preservation alone SHALL NOT discharge this requirement.

#### Scenario: A short loop repeats beside a long loop and a one-shot
- **WHEN** their qualified synchronization modes run together
- **THEN** short-loop wrap SHALL NOT rewind the common master, and the long loop and one-shot SHALL retain beat alignment
- **AND** the one-shot SHALL reach its original EOF without an invented loop wrap

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

#### Scenario: Accepted timing refresh overlaps a parent change or content move
- **WHEN** a queued or in-flight accepted-timing refresh overlaps parent OFF/ON, timing-policy recovery or a Pair content move/swap
- **THEN** Python publication, Pair mirror projection and Native refresh claim SHALL agree on the current hierarchy identity
- **AND** equal-looking raw BPM-Lock preferences SHALL NOT permit stale anchor/master publication

#### Scenario: A hierarchy publication races the final accepted master effect
- **WHEN** a parent/global timing publication and an accepted master refresh reach their final mutation boundary concurrently
- **THEN** original global and source/content claims SHALL linearize the complete loop/master effect against publication, without blocking or callback retry loops
- **AND** a pending claim SHALL retain the original scheduled owner, a stale identity SHALL reject, and no failed qualification SHALL partially apply loop cuts or master timing
- **AND** each Native admission SHALL refuse exhausted counters or capacity before its own mutation; a later admission after accepted cancellation MAY remain explicitly pending/refused without claiming whole-transition atomicity

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
- **AND** runtime recovery SHALL suspend timing effects through an explicit nonserialized recovery gate rather than treating a legitimate False/False/revision0 choice as a corruption sentinel
- **AND** the gate SHALL be set from the actual global-policy validation error before restore, retain valid raw BPM-Lock/parent preferences, survive in-place mirrors and parent toggles, and prevent master-coupled publication even if an independent launch hold cannot be acquired
- **AND** unrelated metadata errors or historical omissions SHALL NOT set this specific gate, and a fresh valid project load SHALL use normal availability
- **AND** invalid PAD intent under a valid global ON SHALL recover to explicit safe OFF rather than inherited ON
- **AND** the original save fence and Native source/adoption authority SHALL remain required
