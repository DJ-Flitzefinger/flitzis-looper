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

## Stage B feedback boundary

Reuse ContentFeedback owner/cell/lifetime/transport_revision, Native resolution,
placement fences and the existing App poll. Current callback delivery is best
effort; ordinary completion needs bounded retained delivery or pre-effect capacity
with valid deferral. Exact originating revisions must survive ordered Pause/Seek
and Seek/Resume even when later gestures are already admitted. Never relabel an
old effect with a newer gesture merely to pass the resolver. A later valid
transport observation must carry forward the actual ordinary paused outcome.
Hold's own ticket remains separate from ordinary pause intent and UI state.

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
