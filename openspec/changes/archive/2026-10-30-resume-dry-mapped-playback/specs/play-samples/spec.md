## ADDED Requirements

### Requirement: Dry mapped pause preserves the actual source position
Supported dry mapped playback SHALL pause through the real content-bound
transport and caused waveform-hold paths. Pause MUST make only the intended
voices silent without advancing their source trajectory. Resume MUST continue
from the exact saved fractional source position at its actual output frame,
then follow the same original map using the authoritative transport. It MUST
NOT catch up by skipping unheard source content or retrigger the loop.

#### Scenario: Resume follows a long silent pause
- **GIVEN** a qualified dry Total0 Key-Lock-OFF mapped voice is playing
- **WHEN** it pauses at output frame P and resumes at a later output frame R
- **THEN** no source reads are fabricated during the pause
- **AND** its first resumed source position is bit-identical to the saved position
- **AND** subsequent samples, map knots and exact loop wrapping remain partition invariant

#### Scenario: Repeated pause or resume is harmless
- **WHEN** an already paused voice receives Pause or a playing voice receives Resume
- **THEN** its source position, continuity fences and history remain unchanged
- **AND** unrelated sample IDs remain unaffected

#### Scenario: Ordinary Pause takes over an earlier Hold pause
- **WHEN** ordinary Pause takes authority over mapped lanes already frozen by a Hold
- **THEN** ordinary Resume uses each changed lane's own saved next-read coordinate
- **AND** it does not replace that coordinate with the later ordinary Pause command frame
- **AND** the separate Hold release still requires its own original effect and mask

### Requirement: Mapped continuation is preflighted for the complete affected cohort
The callback SHALL preflight every affected voice before changing any audible
cohort state. It MUST retain the actual original map/source/content owner and
validate the current supported dry tuple, master epoch, source continuity and
exact representability. Missing, stale, unsupported or unrepresentable state
MUST refuse the effect without partially changing the cohort. Existing content
placement/transport revision, caused-pause masks/generations and retirement
rules MUST remain authoritative.
An ordinary audible effect MUST validate all mapped participants before taking
Hold authority, including participants already in the requested pause state.
The existing supersession caused by an accepted ordinary admission
remains separate and MUST NOT revive an old Hold release after refusal.

#### Scenario: One participant cannot resume safely
- **WHEN** a targeted cohort includes an unsupported or stale mapped participant
- **THEN** no targeted voice partially resumes or advances
- **AND** no Native history or retained input is relabelled as a new mapped epoch

#### Scenario: A waveform release follows content movement
- **WHEN** a caused-pause ticket is released after content moves or its intent is superseded
- **THEN** the existing own-effect and complete-cohort checks determine its authority
- **AND** a stale ticket cannot resume replacement content or another hold's paused voices

#### Scenario: No voice would change but mapped custody is invalid
- **WHEN** ordinary Pause or Resume targets an unsupported mapped cohort already in the requested state
- **THEN** no cohort trajectory or continuity fence changes and the audible effect is refused
- **AND** no-effect status cannot bypass the required mapped qualification
- **AND** an already accepted intent still fences an older Hold release under the existing supersession policy

### Requirement: Pause feedback distinguishes request from actual audio state
Control feedback SHALL distinguish an enqueued pause/resume request from its
callback-effective state. Unsupported, stale or rejected effects MUST NOT be
represented as successful audio state merely because enqueue succeeded. The
existing Native content/placement and own-ACK paths SHALL be reused.

#### Scenario: An admitted request cannot take effect
- **WHEN** the callback refuses a pause or resume after queue admission
- **THEN** user-visible effective playback state follows Native observation
- **AND** the requested state does not manufacture a successful pause/resume

### Requirement: Ordinary transport completion is lossless and preserves its origin
An ordinary Pause, Resume or Seek for living current content SHALL retain its
original admitted content and transport revision through callback completion.
The callback MUST reserve completion capacity before performing the command's
effect, revoking its Hold registry or publishing its observation. Without capacity
it MUST defer the exact original command without these changes; later ordinary
commands MUST NOT overtake it. Completion MUST distinguish applied, no-effect and
refused outcomes and capture their actual originating fields inside the same
claimed content effect. A newer admission MUST NOT retag an older completion.

#### Scenario: Output feedback is full
- **WHEN** an ordinary transport command reaches the callback with no completion capacity
- **THEN** no part of that command's audible effect or Hold-registry mutation occurs
- **AND** the same command and original revision retry ahead of later ordinary commands
- **AND** render cleanup and an earlier queued release retain the exact superseded Hold cause for that ordered command without restoring old release authority

#### Scenario: Ordinary admission races render cleanup
- **WHEN** an ordinary command is admitted after render cleanup begins inspecting an owned Hold but before cleanup decides that the Hold was superseded
- **THEN** cleanup MUST NOT retire its exact caused pause using a custody snapshot older than the superseding admission
- **AND** the pending ordinary command retains that cause through feedback pressure until its original claimed disposition
- **AND** actual content, lifetime or cohort replacement still retires invalid Hold ownership

#### Scenario: Admission races the completed effect
- **WHEN** another transport gesture is admitted after an effect but before its message is pushed
- **THEN** the completion still identifies the effect's original admission
- **AND** its state is never relabelled as the newer gesture

### Requirement: Current ordinary observations have independent bounded freshness
Native SHALL maintain bounded callback-owned ordinary observation metadata per
stable content cell, separate from audio/time authority and global intent
admission. Observations MUST identify content ownership, lifetime, semantic
source, original command revision and a monotone observation sequence. Old or
foreign observations MUST NOT apply after replacement, retirement, a newer actual
observation or same-origin lifecycle changes. Sequence and semantic revocation
fences MUST NOT wrap or reset into a previously usable identity. Compatible
same-source resident-window refresh MUST NOT revoke ordinary observation custody.
The existing generic content-feedback resolver MUST retain its exact-current
transport-revision contract.

#### Scenario: Seek follows Pause before the UI polls
- **WHEN** the callback completes Pause and then Seek for the same living content
- **THEN** the current Seek observation carries the confirmed ordinary pause state
- **AND** it retains the exact latest completed Pause/Resume admission frontier
- **AND** an older raw completion cannot overwrite that current observation

#### Scenario: A stream or source is revoked and later republished identically
- **WHEN** an earlier observation crosses actual source revocation or stream restart
- **THEN** identical later source fields do not resurrect that observation
- **AND** the observation sequence is never reset to make an old receipt current

#### Scenario: Observation authority reaches its terminal numeric bound
- **WHEN** no further ordinary observation can be published safely
- **THEN** ordinary transport cannot mutate voices or consume a Hold without that authority
- **AND** future admission reports explicit unavailability while the last successfully published current observation remains readable
- **AND** permanent invalidation or exhausted semantic source custody makes queries explicitly unavailable
- **AND** lifecycle Stop or retirement remains possible and no old receipt becomes current

### Requirement: Ordinary state remains observable across lifecycle and Hold actions
The current ordinary projection SHALL remain durably queryable through Native
even if a legacy lifecycle event is unavailable. Actual Stop, claimed new birth,
source-image adoption and render termination MUST publish or fence the applicable
current-content projection. Move/Swap MUST preserve its stable content ownership.
Nonreplacement same-content source adoption MUST preserve the confirmed ordinary
pause cause of its surviving exact content cohort, while invalidating old source
receipts and resetting the new source's completion frontier. It MUST NOT infer
ordinary pause from Hold-only or arbitrary paused voice flags.
Waveform Hold alone MUST NOT change ordinary pause intent or its completion
frontier. Intentional ordinary Pause during a Hold MAY take ordinary pause
authority. A scalar Seek that actually consumes a validated Hold MAY take over
its still-paused exact cohort; unrelated or early-refused Seek MUST NOT infer
ordinary pause from arbitrary voice flags. Existing mapped Seek refusal remains.
If an accepted ordinary admission supersedes an exact validated Hold cohort,
the callback MUST confirm the remaining paused state from that original effect
even when mapped qualification refuses the audible command. Its outcome MUST
remain refused; it MUST NOT claim that the requested audible effect applied.

#### Scenario: A lifecycle event is lost after confirmed Pause
- **WHEN** the same content actually restarts or stops after ordinary Pause
- **THEN** querying the current Native observation exposes that actual lifecycle state
- **AND** an old pause observation cannot restore stale UI pause state

#### Scenario: Seek first establishes ordinary pause custody
- **WHEN** scalar Seek takes over a Hold pause without a preceding ordinary Pause/Resume admission
- **THEN** its current Native observation retains bounded durable query custody
- **AND** a later actual restart or Stop remains observable if legacy lifecycle feedback is unavailable

#### Scenario: A mapped Seek is refused while Hold remains paused
- **WHEN** mapped Seek qualification fails while a validated Hold cohort remains paused
- **THEN** its audible seek effect is refused without changing the source trajectory
- **AND** prior accepted intent supersession still prevents the older Hold release
- **AND** remaining ordinary pause custody is confirmed only from an exact validated superseded Hold effect
- **AND** no ordinary pause is invented merely from paused voice flags

#### Scenario: Warm source adoption retains an ordinary-paused voice
- **WHEN** a nonreplacement same-content source publication retains an ordinary-paused old-source voice
- **THEN** the new observation confirms its actual active and ordinary-paused state
- **AND** old source admissions remain invalid and the new source has no inherited completion frontier
- **AND** later Seek cannot report the still-paused voice as running

#### Scenario: Source changes while ordinary supersession waits for capacity
- **WHEN** a source change makes a deferred ordinary command stale while its exact Hold-paused cohort survives under the same content key
- **THEN** the old command cannot mutate audio or acquire new-source completion authority
- **AND** its original-source refusal retains the exact pause cause for actual source adoption
- **AND** the new source confirms surviving pause without inheriting the old admission frontier

### Requirement: Fast pause gestures retain bounded requested and confirmed state
The control layer SHALL retain requested Pause/Resume intent under the existing
living content instance, bound to its exact Native admission. It MUST change
confirmed Session pause state only from a current Native observation projected
through the existing assignment, placement, Pair, arrangement and bank fences.
It MUST project that state before settling a matching completed request. A current
observation MAY carry the exact completed Pause/Resume frontier across Seek or
lifecycle; matching only a boolean outcome is insufficient. Completed admissions
MAY remain as bounded observation monitors. A Seek-only observation MUST retain
equivalent query custody when no Pause/Resume admission exists. Pending observations MUST coalesce
only by stable Native owner/cell and strictly newer sequence, within the existing
living-content capacity. Stale lifetimes MUST NOT consume extra retention slots.
Incompatible Native admission/observation APIs MUST fail preflight before App start.
The control layer MUST durably query bounded living content slots through a new
currently validated Native observation even without an existing monitor or raw
event. It MUST fence queries before execution and verify the captured Python
content identity again before projection. Old source admissions remain invalid.

#### Scenario: Pause then Resume then another click precede feedback projection
- **WHEN** several pause gestures are admitted before their observations reach Session
- **THEN** each next toggle uses the latest still-pending exact request
- **AND** an older running observation cannot settle a newer Resume
- **AND** a later actual completion is projected before removing its requested overlay

#### Scenario: Arrangement fences delay many ordinary observations
- **WHEN** more raw observations arrive than the legacy feedback deque can retain
- **THEN** the newest ordinary observation for each stable Native cell remains retained or durably queryable
- **AND** projected state follows living content through Move/Swap without affecting a replacement occupant

#### Scenario: Source changes without changing the content lifetime
- **WHEN** a current observation for a replacement semantic source arrives before the old monitor is polled
- **THEN** the new observation retains durable monitoring of that current source
- **AND** only an admission with current Native source membership may retain a requested overlay
- **AND** later lifecycle changes remain observable without restoring the old source's intent

#### Scenario: New source or first Seek has no raw observation bootstrap
- **WHEN** a current source-adoption or Seek/lifecycle snapshot exists without a usable raw event or preceding Pause/Resume monitor
- **THEN** bounded current-state polling obtains a new validated Native observation
- **AND** Session and the next toggle follow that state after the existing content fences
- **AND** a temporary absent snapshot does not settle a still-valid pending admission
