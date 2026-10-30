## ADDED Requirements

### Requirement: Shared SYNC preserves intentional whole-beat phrase offsets
With explicit source Grid Offset g, the shared SYNC path SHALL use the
adopt-live-source-sync-policy beat-phase rule modulo1, without chasing phrase
position modulo the complete loop Q. Absent-g launch-relative behavior SHALL
remain unchanged. Full source/grid identity and current actual-read confirmation
SHALL remain required independently of mode adoption and map readiness.

#### Scenario: Different loop lengths already have matching beat phase
- **WHEN** explicit-g long and short loops differ by a whole number of beats
- **THEN** shared SYNC SHALL retain that phrase offset while folding each own loop
- **AND** it SHALL NOT introduce a whole-loop phase chase or claim unverified bar phase

### Requirement: Timing controls retain preferences under a shared parent hierarchy
The application SHALL resolve MULTI LOOP -> BPM LOCK -> optional SYNC through the existing GUI, Learn/MIDI, runtime and Native paths. Each child SHALL be unavailable for editing when its requested parent is unavailable or off. Stored BPM Lock, SYNC, historical Realtime BPM and content preferences SHALL remain intact while a parent is off. The runtime SHALL project realtime_bpm=true, including neutral revision0, without rewriting saved schema1 preferences or historical seals. Legacy tracker actions SHALL address SYNC. Control availability SHALL use qualified requested-parent state rather than an actual playing/settled acknowledgement, allowing configuration before playback. Key Lock, map selection, speed and trigger quantization SHALL remain separate controls. Source tempo tracking SHALL NOT add another master clock.

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

#### Scenario: The selected reference slot is empty at activation
- **WHEN** BPM Lock is requested before the captured selected PAD has any content
- **THEN** the pending request SHALL retain that selected slot and MAY bind its first observed current assignment identity exactly once before any submission attempt or unobserved admission
- **AND** selection changes SHALL NOT elect another reference; ordinary loading or a qualified Pair filling that empty slot SHALL permit the same pending request to progress
- **AND** the Pair control mirror SHALL preserve this still-unbound slot while continuing to move an already bound original anchor with its content
- **AND** after first binding, replacement or loss of that original content SHALL NOT transfer its qualification to another source, and Native admission SHALL still require the current original source binding
- **AND** an incomplete assignment SHALL NOT bind its predecessor; missing timing acknowledgement or failed runtime publication after first binding SHALL NOT erase that identity

#### Scenario: The original BPM reference becomes ready without a preceding master
- **WHEN** a qualified initial ON target can apply under the original global and content claims
- **THEN** Native SHALL derive the master from that original content's actual current source period and current speed, preparing endangered histories and the existing Transport bootstrap before mutation
- **AND** BPM Lock, initial master and eligible one-time bootstrap SHALL apply together before the actual receipt, without a second unqualified Python master or bootstrap command
- **AND** Python Session master fields SHALL mirror coherent actual Native timing rather than admission

#### Scenario: An applied BPM reference recovers from a transient observation failure
- **WHEN** an already applied and mirrored BPM request encounters unavailable feedback or an original-source Hold, and a later normal poll observes its own matching Applied receipt with the original source currently qualified again
- **THEN** its transient pending/error presentation SHALL recover without another BPM, master or bootstrap effect
- **AND** a foreign receipt, pending effect, missing qualification or replacement content SHALL NOT clear that pending/error state merely because feedback becomes readable
- **AND** normal same-content timing reconciliation SHALL retain its existing qualified master path and expose any new failure

#### Scenario: A later master or speed request waits while its source changes
- **WHEN** the existing global-rate coordinator admits a master-bearing request derived from an original source binding
- **THEN** its fast and delayed history effects SHALL retain and validate that same content/source/timing qualification under the original global and content claims, including when no voice runs
- **AND** Pair movement of the same content SHALL remain eligible, while replacement or stale source timing SHALL reject the complete tuple without partial speed, master or bootstrap effects
- **AND** speed-only coalescing SHALL preserve a still-pending master qualification and SHALL NOT revive a previously rejected master value
- **AND** a fresh qualified master-only request without pending rate work SHALL retain actual speed rather than revive a previously rejected unqualified speed preference
- **AND** the original hierarchy witness SHALL reject parent-superseded master work even before its delayed rate capture begins, while unrelated speed edits SHALL preserve that witness
- **AND** later same-content timing or speed changes SHALL freshly qualify their own master effect without invalidating a historical applied BPM receipt or blocking loop-only refresh

#### Scenario: Actual master provenance changes after automatic timing
- **WHEN** a qualified Manual, TAP, Legacy or accepted-timing master publication actually succeeds
- **THEN** Native SHALL publish its original-source correlation and actual accepted revision, or qualified absence of an accepted revision, in the same coherent observation as the actual rate tuple
- **AND** requested, preparing, rejected, superseded and speed-only observations SHALL preserve the preceding actual master provenance; coalescing SHALL retain a still-pending explicit master publication
- **AND** a successful qualified publication SHALL update provenance even when its numeric period is unchanged, while an actual unqualified master publication SHALL remain explicitly unknown
- **AND** original-content Pair movement and ordinary resident-window adoption of the same full source SHALL preserve correlation; a replacement source, content lifetime, Engine or generation SHALL NOT inherit that correlation
- **AND** the correlation SHALL NOT grant source, window, claim or execution authority
- **AND** Python SHALL mirror the last own qualified master provenance only from actual Native evidence, keeping missing or foreign qualification unconfirmed without another master owner or blocking the historical BPM receipt needed for recovery
- **AND** accepted-refresh completion SHALL observe that same fresh actual evidence rather than overwrite a later actual master with its earlier request values after the ticket-current check
- **AND** a terminally rejected unqualified master request SHALL NOT supply the period for a later speed-only effect; a newer request already coalesced with a genuinely pending master SHALL retain that intended master publication
- **AND** BPM Lock OFF MAY clear the qualified BPM reference but SHALL NOT reset Transport time or its retained tempo
- **AND** an incompatible Native provenance API SHALL be rejected by reflection before application or audio-device construction

#### Scenario: Stream construction binds existing timing owners
- **WHEN** the Native stream is constructed with existing source ownership, timing acknowledgements and preparation epochs
- **THEN** construction SHALL preserve their exact identities and existing source, status, provenance and epoch values without creating replacement authority
- **AND** the existing admission invalidation SHALL occur exactly once after successful construction; a failed construction SHALL leave the supplied admission policy unchanged
- **AND** the existing production constructor and callback stack bounds SHALL remain required

#### Scenario: Anchor qualification is unavailable or changes during a speed edit
- **WHEN** resolved BPM Lock is requested and its matching applied original master owner or current anchor timing cannot qualify the requested speed
- **THEN** the edit SHALL refuse before publication rather than fall back to an unqualified speed-only command
- **AND** one coherent timing snapshot SHALL determine the target and its source binding; fresh Native qualification MAY reject a changed snapshot but SHALL NOT silently replace it with another target
- **AND** a cached earlier actual-OFF observation SHALL NOT authorize that fallback across a pending initial ON effect

#### Scenario: BPM Lock OFF encounters an actual Fixed voice
- **WHEN** an OFF request remains pending while Fixed synchronization is still actual
- **THEN** the original policy owner SHALL first adopt the allowed Legacy representation or retire that voice before actual BPM Lock becomes off
- **AND** prospective tempo/pitch/source-history safeguards SHALL remain required for the final parent effect, preserving preceding actual audio while unavailable

#### Scenario: A new hierarchy supersedes a deferred BPM Lock effect before replacement admission
- **WHEN** a hierarchy publication succeeds while an older BPM Lock ON or OFF effect remains unapplied, but the replacement command cannot yet be admitted
- **THEN** the older effect SHALL retain its original hierarchy witness and SHALL NOT later apply against the new hierarchy
- **AND** observations SHALL distinguish its stale reference from the preceding actual audio state
- **AND** the control owner SHALL requalify the current resolved target through one successful replacement admission per stale receipt, retaining normal bounded retry for failed submissions
- **AND** reserved cancellation followed by complete publication SHALL recover even when the desired Boolean and source fingerprint are unchanged; unrelated speed changes SHALL NOT invalidate a current target
- **AND** a previously applied effect SHALL remain a historical actual fact rather than being retroactively cancelled

#### Scenario: Parent changes return to equal-looking choices
- **WHEN** an accepted parent request changes OFF then ON without changing either stored child choice
- **THEN** a checked hierarchy request revision SHALL distinguish both changes and invalidate old captures and acknowledgements
- **AND** revision exhaustion SHALL refuse before Native or project mutation, unchanged setters SHALL remain no-ops, and reserved cohorts SHALL retain their complete preceding publication until their original cancellation or ACK boundary
- **AND** a hierarchy revision preflight SHALL preserve ordinary MultiLoop cancellation/supersession of an already reserved Pair/Bank launch

#### Scenario: The user selects fixed BPM
- **WHEN** SYNC is switched off
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
An unavailable or disabled SYNC request SHALL select the established Legacy path: original constant detected/manual/TAP timing and existing BPM matching only when actual BPM Lock is enabled. A remembered global SYNC preference SHALL NOT force matching while the parent is inactive. Qualified requested SYNC SHALL use the existing source-qualified mapped path regardless of the historical tracker preference. Fixed and mapped actual consumers SHALL require actual BPM Lock, not merely a retained master period. Native runtime projection SHALL resolve its captured requested MultiLoop and BPM-Lock parents before choosing a timing target while preserving raw intent across omitted-row retention; actual-parent guards alone SHALL NOT replace requested-parent resolution. Missing or unsupported variable geometry MUST NOT silently select fixed synchronization. GUI, MIDI, runtime publication, loop/grid and automatic timing publication SHALL consume the same resolved policy.

#### Scenario: One SYNC preference selects the effective source timing
- **WHEN** SYNC is requested with its parents available
- **THEN** the application SHALL project the mapped target with historical tracker preferences preserved only as saved data
- **AND** SYNC OFF SHALL use the original constant source timing without map prerequisites

### Requirement: Automatic map selection retains original source custody
Normal SYNC SHALL reuse a pristine automatic map from original analysis or acquire one through the existing map worker. Fresh standalone acquisition SHALL require requested SYNC and defer primary analysis. Saved automatic selections SHALL restore with fresh source verification without another NN inference. Manual selection or Clear SHALL supersede late automatic work. Map geometry SHALL preserve original decoder seconds; physical loop markers SHALL use the loaded rate. Operational loop bars SHALL contain four detected quarters without asserting verified meter or downbeats; gaps and uncertainty SHALL remain unavailable.

#### Scenario: Optional map completion precedes metadata and worker retirement
- **WHEN** an original Cold/Task map claim becomes available
- **THEN** the application SHALL consume it once before tempo/key callbacks and bind it to the recorded current content afterward
- **AND** placement Pending SHALL retry before core mutations, while other optional map failures SHALL NOT abort successful core load or analysis
- **AND** a map-only job SHALL remain owned until actual terminal retirement after export and SHALL then release naturally
- **AND** effective attachment SHALL require source/content/intent currentness before, inside and after successful durable publication; false publication SHALL NOT grant an ACK

#### Scenario: A stopped third PAD is loaded during active SYNC
- **WHEN** a fresh unfenced Native observation proves the loaded source inactive
- **THEN** effective, pending, detail and phase aggregation SHALL exclude that PAD consistently
- **AND** unknown or pending-launch participants SHALL remain included, and an empty participant set SHALL grant no effective or phase confirmation

#### Scenario: Parent and child combinations are used
- **WHEN** parent availability and remembered child choices are resolved
- **THEN** inactive SYNC SHALL retain Legacy behavior, static SYNC SHALL use original constant timing, and variable SYNC SHALL require qualified geometry
- **AND** existing source, rate, pitch, preparation and renderer guards SHALL remain enforced

#### Scenario: A variable loop edit captures its source map
- **WHEN** an explicit bar, marker or resnap operation captures a qualified immutable source map under available variable timing parents
- **THEN** its physical start, automatic endpoint and musical snapping SHALL use that same captured map and sample rate without fetching another map through general requested-region resolution
- **AND** existing content, intent, source-generation, coverage, meter and publication guards SHALL preserve atomic refusal
- **AND** a maximum-bars query SHALL retain its read-only snapshot path without acquiring edit or residency ownership

#### Scenario: Global or explicit PAD SYNC OFF finishes live adoption
- **WHEN** an active voice settles in Legacy under the current global policy and PAD intent after global or explicit PAD SYNC OFF
- **THEN** requested-mode resolution and adoption matching SHALL accept that actual Legacy target
- **AND** the settled OFF request SHALL NOT remain pending or require another restart solely because an obsolete Off mode name was expected

### Requirement: Grid phase confirmation is distinct from timing mode acknowledgement
Settled fixed ratio, mapped custody, map readiness and actual variable/static rendering SHALL remain observable mode facts but SHALL NOT certify common-grid phase alignment. Presentation SHALL retain a separate nullable phase qualification, unknown until an actual qualified Native algorithm supplies a current source/master/request witness. Requested active SYNC with mode-only evidence SHALL remain phase-pending/unconfirmed, preserving genuine refusal and tempo-settling guidance. Global SYNC and PAD labels SHALL NOT display complete synchronization from the earlier mode-only acknowledgement.

#### Scenario: A fixed target settles or a mapped mode is current
- **WHEN** existing callback evidence confirms the requested mode without a qualified common-grid phase witness
- **THEN** the UI SHALL distinguish applied mode from unconfirmed grid phase rather than display fully synchronized audio
- **AND** stale source, parent, map or request evidence SHALL NOT confirm either the new target or its phase

#### Scenario: Realtime BPM settles while grid phase remains unconfirmed
- **WHEN** current callback evidence confirms the requested timing mode and actual Realtime BPM state, with only common-grid phase qualification missing
- **THEN** SYNC SHALL retain phase-pending guidance while Realtime BPM SHALL display its confirmed actual state without borrowing phase-only pending guidance
- **AND** genuine policy, source, map, settlement, adoption or refusal mismatches SHALL retain their pending or unconfirmed state

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
Startup preflight SHALL require source-qualified BPM Lock submission and its
callback receipt API before importing the application or creating an audio device.
Reflection SHALL NOT invoke the engine or submit control work.

#### Scenario: A compatibility flag transition has no qualified master reference
- **WHEN** an unqualified compatibility BPM-Lock call changes its matching flag
- **THEN** the complete original Transport clock and bootstrap state SHALL remain unchanged, including at a nonzero execution frame
- **AND** that flag receipt SHALL NOT certify source-qualified master adoption or grid phase

#### Scenario: Explicit OFF clears an old actual matching master
- **WHEN** actual BPM Lock is already false but an earlier public rate effect retains an actual matching master
- **THEN** explicit OFF SHALL still require the established rate, map, history, drain and final ownership guards before clearing that master
- **AND** its actual acknowledgement SHALL preserve historical rate request identity and the original Transport clock
- **AND** later speed-only edits SHALL NOT resurrect the cleared matching master

#### Scenario: A foreign history lane prevents original rate capture
- **WHEN** an original rate request reaches pre-capture service while another owner occupies required history or preparation capacity
- **THEN** the request SHALL reject without changing participant epochs, worker ownership, productive audio or the complete actual rate tuple
- **AND** transient completed-drain or final-claim contention MAY instead retain the same pending request without reclassifying a foreign owner as retryable capacity

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

#### Scenario: A queued loop-only refresh encounters a PAD-SYNC or map change
- **WHEN** PAD-SYNC, its selected map or a hierarchy parent changes after derived loop geometry was captured
- **THEN** the original content/lifetime and precise request/map cancellation witness SHALL reject the old cuts at admission, callback claim and ticket observation
- **AND** reserved parent cancellation SHALL invalidate old hierarchy work even before the complete new policy can publish
- **AND** ordinary trigger admission, the refresh's own authority-rebased binding publication and unrelated completed speed edits SHALL preserve otherwise current loop-only refresh liveness

#### Scenario: A claim-pending master refresh crosses a callback boundary
- **WHEN** an originally qualified master refresh retains its scheduled owner because a final claim is temporarily busy
- **THEN** the next normal callback SHALL preserve an already completed, revision-matching control-drain witness only during its bounded deferred retry when the control queue is empty and no deferred control remains
- **AND** releasing claim pressure SHALL allow the same original owner to apply without another admission, while unread controls SHALL NOT inherit that completed witness
- **AND** competing rate work, an incomplete drain basis or an inconsistent actual master target SHALL reject the refresh through a pure check without modifying the other rate owner, allowing that owner and the control queue to progress
- **AND** normal drain invalidation and completion SHALL resume after the deferred retry; no new scheduler, clock or callback retry loop SHALL be introduced

#### Scenario: Accepted anchor timing changes after a requested speed is delayed or rejected
- **WHEN** accepted-derived refresh needs a master effect for the already master-ready original anchor
- **THEN** the existing waiting owner SHALL wait for a coherent terminal rate observation and derive the master from actual effective speed rather than the stored requested speed
- **AND** queued, preparing, locally unobserved or unknown rate confirmation SHALL NOT terminally consume that waiting master update
- **AND** a rejected rate request SHALL retain its error and requested preference while a separately qualified refresh MAY use the known preceding actual speed
- **AND** loop-only refresh before the initial BPM acknowledgement or for another PAD SHALL retain its established independent path

#### Scenario: Manual anchor timing changes after a rejected speed request
- **WHEN** the already applied original anchor receives current Manual, TAP or Legacy timing while requested and actual speed differ
- **THEN** the existing control owner SHALL qualify the new master using coherent terminal actual speed without retrying the rejected speed preference
- **AND** queued or unobserved rate work SHALL retain that reconciliation through the existing owner, while automatic accepted timing SHALL remain on its existing refresh path
- **AND** periodic reconciliation SHALL NOT resubmit an unchanged rejected master and source qualification; a new explicit gesture or fresh source/timing qualification MAY admit a new request

#### Scenario: A global request encounters reserved arrangement rows
- **WHEN** whole rows remain owned by a pending Pair or Bank ACK/mirror
- **THEN** their complete previous publication SHALL remain intact
- **AND** a MultiLoop cancellation stage SHALL retain the corresponding original requested-pitch journal together with every prior row, including terminal requests on unreserved PADs, so later ordinary reconciliation can still apply or roll back their original effects
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
