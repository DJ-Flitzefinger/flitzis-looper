## Authoritative paths and compatibility

Reuse ProjectState, existing project persistence/recovery and strict historical
Pair/Copy/Portable journal validation. Global choices are project settings, not
content snapshot fields. Keep per-content PadSourceSyncIntent bytes and revision
semantics, Copy/Move/Swap/Bank behavior and historical seals unchanged.

Exactly false/revision0 is neutral PAD intent. Explicit true/revision0 remains
valid; explicit OFF at a later revision remains an opt-out. Global ON can enable
neutral eligible content; global OFF suppresses every PAD without overwriting
its stored preference. A missing global choice preserves prior behavior rather
than automatically enabling old untouched content or suppressing old explicit ON.
Use a strict optional global policy with checked revisioning for this compatibility
state, not extra per-PAD sentinels or revision rewrites. PAD toggle/set operations
must compare resolved preference so inherited ON can become explicit OFF.

The strict project DTO is schema1 with required `sync: bool|null`,
`realtime_bpm: bool|null` and checked `revision: u64`. Both choices null at revision0
are the neutral legacy policy. Native receives optional keyword-only
`timing_policy_json=None` on existing `set_input_runtime_state`; validate at most
1024 UTF-8 bytes and the exact schema before mutation. Omitted/neutral policy
retains legacy calls. Preserve positional arguments and existing map-row custody.

## One timing-policy resolver

Keep initial constant timing from BpmController/current accepted timing, manual,
TAP and the existing legacy path. Do not freeze the current variable segment when
Realtime BPM is turned off. Preserve selected map data, corrections and jobs.
Qualified variable geometry continues to use existing SourceBeatmap/Native delivery.
Source tempo policy does not feed local map BPM back into master tempo.

Explicit SYNC OFF bypasses both variable mapped adaptation and fixed BPM matching.
SYNC ON with Realtime BPM OFF selects existing fixed source-period/master-period
matching. Both ON require qualified variable geometry and retain its strict
unavailable/gap/unsupported behavior. Fixed matching is an explicit policy, never
an automatic fallback from failed variable qualification. Key Lock, speed, BPM
Lock requested state and trigger quantization remain independent. Existing
TransportTimeline remains the only master authority.

GUI, MIDI/Learn, input runtime rows and loop/grid resolution must consume the same
resolved policy. Native and control feedback must distinguish fixed, variable and
off trajectories; an ordinary scalar voice cannot impersonate qualified mapped
adoption. Global policy identity participates in capture cancellation and current
feedback matching so OFF/ON or fixed/variable changes cannot borrow an old ACK.
Persisted revisions remain data, not Native source or audio ownership authority.

Actual admitted voices freeze a small Legacy/Off/Fixed/Mapped request containing
both raw global choices, global policy revision and PAD intent revision independently of optional mapped
custody. Off and Fixed must affect actual scalar rendering, preparation, snapshots
and global-rate target resolution, rather than merely labelling a launch. Fixed
requires finite current source/master periods and an unclamped supported ratio;
Legacy preserves existing clamping. Fixed effective feedback waits for actual
rate settlement and has no fabricated map revision. Mapped retains its original
map identity. Off can acknowledge its actual mode/revision despite effective=false.
Feedback compares both frozen nullable choices as well as the mode and revisions;
neutral null/null/0 cannot acknowledge explicit false/false/0. Missing feedback
fields are compatible only with unchanged neutral legacy policy.

An explicit global SYNC ON mapped request may satisfy the existing dry mapped
matching-enable guard even if requested BPM Lock is off. The same valid existing
master and every pitch, delay, Key Lock, source, epoch and whole-map rate guard
still apply. Global-null legacy requests retain the old BPM Lock requirement.
The existing dry rate coordinator uses each voice's own frozen matching choice;
this introduces no policy conversion of active voices or master-follow behavior.
The current GUI establishes a master through its existing BPM LOCK reference.
With BPM Lock OFF, existing control recomputation may clear the master; global
SYNC ON alone does not invent one. The Native OFF-lock cases require an actually
established master and are not proof that a fresh GUI project without a reference
can synchronize. Missing master remains a concrete refusal. A future separate
master-establishment control must use the same clock and real rate coordinator.

## Publication, active audio and feedback

Reuse the complete current input-runtime publication, original binding fences,
captured GUI/MIDI/GLOBAL groups, existing rate coordination and bounded feedback.
Reserve capacity before publishing cancellation/revision changes. Reserved
arrangement rows retain their complete previous state; a global change waits for
the existing actual ACK/mirror rather than partially changing that cohort.

Existing living mapped voices intentionally retain their original mapping until
supported real adoption or stop/retrigger. Global intent and pending state must
be visible immediately, but no control flag, enqueue or map readiness can count
as an actual switch. Do not reset SourcePlayback to an integer coordinate, discard
its owning future, bypass mapped conversion guards or destroy an owner in the
callback. Full continuous live switching remains an explicit wider SYNC gate.

## Modularity and verification

Put policy DTO/resolution, global controller behavior and policy rendering in
focused modules; Engine/Mixer receive necessary delegation. Keep worker acquisition
and musical qualification independent. Check migration and strict malformed input,
neutral/explicit PAD preferences, GUI/MIDI equivalence, all four explicit switch
combinations, original source/policy/capture fences, fixed versus variable actual
feedback, active pending transitions, Copy/Bank/restore and historical seals.
No protocol double or software pass substitutes for music/device/Human gates.
