## Current authority and real gaps

Productive dry variable-rate playback and global all-dry master claims already
exist. Preserve SourcePlayback, original MappedLoopGeometry owners and the actual
TransportTimeline. Public ordinary Pause/Resume uses TransportForPlacement;
cfg(test) slot helpers are not that route. Actual waveform-hold actions use
NativeHoldAction/NativePauseEffect with own acknowledgement, changed/cohort masks,
voice generations, transport revisions and off-thread retirement.

Both mapped control routes currently refuse continuation. Stage A opens only
qualified dry waveform-hold actions with their existing durable acknowledgement.
Its copied source projection is reused in Stage B. Ordinary transport remains
gated until its feedback is lossless and revision-correct. No best-effort paused
event may replace the existing optimistic UI state and then silently disappear.

## Exact copied projection

Resume retains the saved next SourceReadCheckpoint at P and uses the actual
callback frame R. Require the same actual master epoch and R>=P. Translate the
copied mapped address by the checked cumulative pause duration:

`Qnew(n) = Qold(n - (R - P))`.

At R, the exact previous evaluation at P is reused, preserving binary64 source
position rather than subtracting/adding large quarter origins. No fitted phase,
PCM inspection, tolerance or catchup is permitted. Transport itself, its master
tempo/beat/bar state and scheduled starts remain unchanged. The voice's creative
source phase alone incorporates the pause duration.

Map-span search must begin at the earliest representable translated output
address. It cannot search through checked-subtraction underflow as if that were
a valid monotone interval. A resume projection is computed fully in a copied
SourcePlayback and admitted only when the existing same_position bit comparison
passes. Overflow, invalid source/domain or stale epoch rejects without mutation.

Later supported all-dry master changes need an explicit claim-frame projection.
For a previously paused-derived epoch, retain old Q(E) and candidate transport
M(E), then evaluate `Q(n)=Q(E)+(Mnew(n)-Mnew(E))`. At E the difference is exactly
zero. New projection history begins at E; earlier actual spans retain their old
epochs. Subsequent pause translations wrap this copied projection. Never-paused
legacy epochs preserve their existing evaluation exactly. This remains a
projection of the one authoritative transport, not another clock or scheduler.

## Callback cohort and ownership

Borrow the actual callback TransportTimeline when validating/rebasing; equality
of master-period scalars alone is insufficient. Borrow the original qualified
voice map owner, source admission, ContentKey, generation, loop/read coverage
and effective render tuple. No replacing them with current PAD wishes or a newly
selected map. Restrict mapped changes to actual Total0/Key-Lock-OFF dry voices
without active Native/FIFO/retained-input or unsupported source transitions.

Prepare every fallible changed-lane projection and validate required retirement
capacity before changing flags, source clocks, capture fences or hold registry.
Use bounded fixed-size copied values, not callback Arc clones or heap growth.
Only actually changed lanes call existing VoiceSlot.pause/resume invalidation.
Already paused/running lanes remain bit-identical. Scalar neighbors retain their
established semantics. One unavailable changed participant refuses the whole
effect without partial application.

HoldRelease resumes only its own still-valid changed_mask lanes after complete
cohort/generation/intent checks. Already-paused lanes confer no release authority.
Release-before-ACK, Move/Swap, supersession, stop/retrigger/seek/source replacement
and retirement pressure preserve existing own-effect behavior. Missing qualified
owners in artificial mapped fixtures remain negative cases.

During pause no source reads are published. Actual old source-history spans keep
their old epoch; pause/resume invalidate captures only at effective changes.
The new copied clock does not certify historical reads or Native preparation.

## Stage B feedback boundary and frozen control interface

Retain the existing ordered TransportForPlacement command queue, its originating
revision and actual content-effect claim. Require one feedback slot both before
popping the head command and in its direct handler. With no capacity, defer that
exact command before effect or Hold-registry mutation. Capture completion inside
the claim closure; a later admission cannot retag it. Keep the legacy generic
content-feedback resolver's exact-current revision contract unchanged.
Capture the semantic-source fence with the original admission before enqueue
and copy it through that same control message; constructing the returned DTO
must not recapture a source that the callback may already have replaced.

One focused Native module owns fixed ordinary observation metadata per stable
content cell. The callback is its sole snapshot writer. A monotone observation
sequence distinguishes actual completions and lifecycle changes, including those
with the same originating revision. A separate nonresurrecting semantic-source
fence changes on revocation, stream restart or real source replacement; compatible
resident-window refresh preserves it. Source/control writers never become a second
writer to the callback snapshot. Neither fence wraps or resets into an old usable
identity. No audio clock, scheduler, PCM owner or extra work queue is introduced.

One fixed pending-ordinary origin per stable cell preserves the existing exact
Hold effect while an ordered ordinary command awaits capacity. Publish it inside
the original content claim before the global transport revision; reset it on
actual lifetime replacement and finish only the exact original origin under its
full-key claim. A newer ordinary origin must survive an earlier completion.
An earlier queued release may become Superseded without retiring that retained
effect; it gains no release authority. Render/tail cleanup also retains the
validated cause until ordered disposition. A source-stale disposition may retain
its Refused cause only under the original command's source tag, never as new-source
completion. Actual source adoption then confirms surviving cause with a fresh
source tag and empty completion frontier.

AudioEngine.ORDINARY_TRANSPORT_API_VERSION=1 covers these nonconstructible DTOs:

- Pause/Resume return OrdinaryTransportAdmission with content_lifetime,
  transport_revision and requested_paused, minted from the original fields under
  the existing request/producer lock after successful enqueue.
- admission.current_control_slot() validates source/key membership; it does not
  expire merely because another ordinary command completed.
- admission.current_observation() durably reads the current immutable observation,
  through the same Native source/key/sequence/placement checks as event resolution.
- engine._resolve_ordinary_transport_feedback(message) returns that observation
  or None. Both query routes preserve the established Pending diagnostic.
- engine._current_ordinary_transport_observation(id) issues a new current DTO
  from the current Native key/lifetime/source and rechecks the requested slot.
  It can bootstrap observation without a preceding admission or raw message;
  it never makes an old source receipt current again.
- Observation contains current sample_id, lifetime, original transport_revision
  (zero when a lifecycle change has no ordinary origin), observation_sequence,
  status_kind, requested_effect/outcome or None for lifecycle, active and
  ordinary_paused. Status kinds are transport, started, stopped, source_adopted
  and terminated.
- matches_admission checks the direct exact effect origin. settles_admission
  checks the exact completed Pause/Resume frontier retained across Seek and
  same-content/source lifecycle. Both use full opaque ownership/source identity.
- Observation also exposes current_control_slot() and current_observation() with
  the same source/key/placement/terminal rules. A Seek takeover can therefore
  retain query custody without manufacturing a Pause/Resume admission.

The raw ordinary AudioMessage carries only the existing identity/revision style
plus observation sequence. Its stable owner/cell fields permit bounded coalescing;
the actual current state is read from the fixed Native snapshot after validation.
Actual Stop, claimed births (including bypass paths), semantic source adoption and
render termination publish current lifecycle projection. New content/source starts
with an empty completion frontier, without resetting the cell sequence. Explicit
terminal unavailability is reported rather than inventing a successful receipt.
Capacity for the next publication is separate from current-snapshot readability:
the last successfully published observation remains queryable until actual
invalidation. Lifecycle Stop/retirement must remain possible even when observation
space is exhausted, without allowing an old receipt to revive.

Hold alone never changes ordinary projection or completion frontier. Intentional
ordinary Pause can take pause authority from an existing Hold. Scalar Seek can
causally take over its validated consumed Hold cohort if it remains paused; early
refusal does not derive ordinary state from Hold-paused voice flags. Mapped Seek
remains refused. Ordinary Resume uses every changed mapped lane's own saved
SourcePlayback pivot; only Hold release requires the shared original ticket frame.
Mapped qualification also covers ordinary no-op lanes before an audible
ordinary effect is authorized. Validate an unchanged lane at its own next-read
coordinate and leave
its source trajectory, fences and read history untouched.
An accepted ordinary admission already supersedes an older Hold's release
authority under the existing transport-intent policy. If qualification refuses
the audible effect, consume that supersession inside the same claimed completion
and carry remaining pause custody only from the exact validated NativePauseEffect.
Report Refused without changing source trajectory or inventing an applied effect;
do not revive the old release or infer cause from arbitrary paused flags.

The ordinary pending origin is stored before the Release transport-intent store.
Render-tail cleanup is outside the content claim and MUST acquire intent freshness
before its subsequent pending-custody check. Reading old zero custody before the
new superseding revision is insufficient: admission can occur between those reads.
Only a custody check ordered after the observed supersession may authorize cleanup;
invalid content/cohort/lifetime retirement retains its existing precedence.

A nonreplacement same-material reload may keep its content key while replacing
the bank source and retaining the old voice. Source-adoption projection therefore
uses the actual surviving content cohort and its prior confirmed ordinary cause,
separate from the source-fenced receipt. The new source resets the completion
frontier and invalidates old admissions; Hold-only pause is never converted merely
because a retained voice is paused.

Python retains at most one admission or observation monitor per living existing content instance.
While pending, its exact requested state selects the next toggle; only a current
Native observation changes Session pause state. The same assignment, placement,
Pair, arrangement and bank fences apply before queries/admission/projection.
Loading and pending incoming assignment are part of this same shared fence;
locate the existing Python content instance before issuing a durable query.
Poll the bounded living content slots through the current-state query, including
those without a prior monitor. Verify the same captured Python content instance
at the returned slot before projecting. A temporarily absent snapshot does not
settle a request; independently validate its original admission membership.
Project confirmed state FIRST, then settle an exact completed request. Keep a
settled monitor for later durable lifecycle reads, including before a new click.
Never prune on later Native completion alone: Pause may already have applied while
Session still says Running. Source/lifetime removal uses existing assignment
cleanup, preserving content movement and unrelated occupants.
Every accepted current observation refreshes the durable monitor. A retained
request independently validates its own Native source membership, so replacing
the source under an unchanged content lifetime removes only the stale overlay
and preserves observation custody of the new source.

Ordinary pending events retain only the strictly newest sequence per stable
(owner, cell), within the existing 216-content bound. Lifetime/source remain payload
validation, not extra retention keys. This avoids the legacy 1,024-event deque's
best-effort limit. This observation metadata does not duplicate audio authority.
Bootstrap, launcher and stubs explicitly require the new Native capability.

## Meaningful verification and limits

Stage A uses a real qualified synthetic original map through the actual public
hold-ticket/callback route. Compare PCM before/through pause and resume, silence
while the shared master continues, the exact first resumed source point, later
knots/wrapping, repeated fractional pauses, caller partitions and subsequent
master changes. Cover other PAD/scalar isolation, release-before-ACK, pre-existing
pause, content movement/supersession, stale/source/overflow and retirement cases.
Reuse affected source-clock, global-rate, hold/content and scalar regressions.

Normal Release/import/preflight, strict specs and one independent final review
close each coherent delivery. Audio device/hearing is Human-owned. Stage A
closes neither ordinary transport/feedback, mapped seek, live policy adoption,
pitch/KeyLock, full map/B2, all37rates/Total-23..+24 nor full SYNC/final gates.
