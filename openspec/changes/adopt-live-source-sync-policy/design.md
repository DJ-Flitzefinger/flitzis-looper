## Current authority and implementation boundary

TimingPolicyController and SourceSyncController publish through the existing
InputMappingController runtime refresh. Native set_runtime_state_with_policy owns
complete row publication, source delivery and checked raw revisions. A successful
publication currently changes future starts and cancellation fences, not living
voice policy. The Python equal-signature return is before Native publication.

TransportTimeline owns master time. SourcePlayback and its actual read journals
own source addressing. The existing mixer/global_rate.rs fixed 32-lane kernel
captures actual content/placement/lifetime/reader/pitch/continuity, prepares all
replacements, rechecks at actual E inside commit_slot_write_fixed, then mutates.
RateTuple equality intentionally remains a rate no-op; it is no adoption ACK.

Stage 0 copies only the actual master epoch's sample rate, present period, anchor
frame and anchor beat position into an immutable value extracted by
TransportTimeline. These are exactly the inputs of its current clock comparison
and beat evaluation. Keep the original subtraction/division/addition order;
output progress, downbeat/grid display and bootstrap bookkeeping are not beat
projection inputs. Missing master refuses construction. This is frozen data from
the existing clock, not a clock with an independently mutable lifetime. It frees
fixed cursor space for conversion without opening any adoption envelope.

Stage A of this change must close the mathematical conversion contract before
Stage B opens live admission. Existing mapped starts, master changes and qualified
Hold/ordinary Pause/Resume keep their legacy canonical projections and guards.
New anchored conversion must not silently change their established arithmetic.

## Exact fractional conversion

An integer source frame plus a binary64 fraction is the original cursor. Adding
the two before inversion can lose fraction bits even at ordinary long-track
positions; B followed by S is not an exact inverse in binary64. Store the exact
pair as an immutable source anchor at actual E. Future positions must derive
from that same anchor and the frozen actual master projection, independently of
callback/read/preparation partitions. Do not integrate or reanchor at callbacks.

Two mathematical issues remain unresolved before Stage A can be enabled. A
two-word inverse needs an actual proved arithmetic bound, not an arbitrary safety
multiplier. Interior knots allow continuous error bounds; loop source coordinates
are discontinuous, so a circular phase bound alone does not prove ordinary absolute
source-coordinate agreement or actual seam PCM. Define and prove the chosen
wrap-side/seam contract first. Also, the existing scalar noninteger musical-period
path recombines integer offset and fraction even at zero elapsed time: adding a
fractional constructor alone does not prove the reverse exact cursor. Neither
issue is resolved or bypassed by the Stage-0 compact projection.

The new representation must retain original segment anchors, knots and loop
geometry, derive local rate from that same path and support a genuine fractional
reverse scalar constructor. Its admitted domain, numerical error bound and
boundary classification must be explicit before implementation. A single-frame
cursor override followed by a different trajectory, fitted offset, snapping,
nextafter search or relaxed same_position comparison is not an implementation.
The fixed mapped span remains at most 240 B and a 32-lane SourcePlayback copy at
most 16 KiB. Geometry/reference data alone grants no source or adoption permit.

On mapped-to-fixed/OFF conversion, capture the current exact source pair and
applied local source rate. Start the existing .05-per-512-active-output-frame
scalar glide toward the newly resolved unclamped scalar target. Do not change
master, loop intent, launch target, generation or other voices. The new policy is
not settled matching until the actual scalar target has settled. No pre-conversion
history may be fabricated from the endpoint; existing read receipts remain frozen.

## Initial dry live envelope

Initially admit stable manual physical loop regions only, with an already valid
actual master for requested synchronization, zero pitch, Key Lock off, no active
Native or retained Native input/history requirement, no stem transition and no
affected paused/Hold lane. Validate the complete relevant cohort, including an
unchanged-looking requested value. Refuse a changed physical region or a source
cursor outside the new qualified domain rather than clamping/wrapping it.
Both genuine global start-mode settings use this same live conversion; the
settings themselves remain fully-stopped-only and never acquire a live override.
AutoBars/window changes and unsupported transitions retain their explicit gates.

## Immutable custody and original lifetime

Before enabling any conversion, an independent behavior-preserving prerequisite
aggregates the current original CapturedPadStart and its effective immutable map
reference as PadStartCustody. Existing successful mapped births obtain the same
original map, so all current effective geometry and witnesses remain unchanged.
Scalar births remain without mapped custody. Do not add a replacement/adoption
setter in this prerequisite. Pending start Work and MappedNativeInput still use
their own original captured geometry, not a living voice's effective reference.

The existing PadStartTarget retirement payload carries the whole aggregate in one
entry. Reservation accounting remains one slot per retained mapped voice; raw
captured-target cleanup wraps only its original owner with no effective map clone.
No owner moves before the original successful claim and retirement preflight.
Stop/retrigger transfers both references together; failure preserves both.
Move/Swap follows the same living content/voice without replacing this custody.
The last effective map reference may outlive original-owner destruction, so both
references must be destroyed on the established worker/close teardown path.

MappedStartSource already holds QualifiedSourceMap, original delivery/artifact
pins and owning geometry. Its Arc may be shared as immutable effective map custody.
VoiceSlot must retain its exact original CapturedPadStart independently, including
Work/start-policy lifetime and its established off-realtime retirement. Adopting a
map cannot synthesize a start, consume a launch lease or reuse an old ACK.
Every render/snapshot/continuation/beat-position/rate/feedback geometry access must
use the effective custody. Retaining a launch owner after mapped-to-scalar must
not force the old map domain into scalar playback. Newly adopted, displaced and
rejected custody can itself be the last reader; reserve and use existing off-RT
retirement capacity before admitting an effect.

## Publication, retry and atomic execution

Adoption must capture the original current Native snapshot, not Python's idea of
the cohort. Bind all raw global choices and revision, PAD revision, exact map
delivery/revision, content key/lifetime, source/request incarnation/generation,
placement, loop window, pitch, timing/drain fences and actual master epoch.
Away/back changes remain distinct even when booleans match. Map-ready
None-to-delivery is a new adoption candidate without requiring another toggle.

A successful runtime publication followed by temporary admission or retirement
pressure must remain retryable. Either service Native-owned pending work after
publication or invoke an idempotent original-snapshot admission on cache-hit
refresh too. A hook only under Python's changed-signature branch is insufficient.
Do not add a Python cohort queue, a second audio clock or a second scheduler.

At actual E, prepare all fixed conversion replacements, then repeat complete
participant/freshness/reader/phase checks under the original claim before mutation.
Commit all affected policies, effective custody and playback values atomically;
leave master unchanged. Capacity, stale, unsupported or missing-master failures
leave all preceding effective audio and pause/Hold custody intact. A global ON
choice must never bootstrap or rewrite master tempo. Reserve owning work off RT.

## Feedback and validation

Extend the existing bounded SourceSyncStatus/diagnostics. Actual voice source,
window, mode, raw revisions/choices, PAD and map revision and settled state must
agree before acknowledgement. Distinguish an admission refusal from awaiting an
unimplemented start/retrigger transition; enqueue/mapready/equal values are no ACK.

Use real public runtime publication through the callback and actual source reads.
Cover exact fractional E/PCM, original knots/wraps and unequal partitions; all
policy directions; map-ready and cache-hit retry; full revision away/back; one bad
participant among 32; pause/Hold/source/window/master/Move/Swap/retirement pressure;
original launch and displaced map last-reader lifetimes; and feedback pressure.
Existing scalar/global-rate, launch, Hold, ordinary transport and history neighbors
remain required affected regressions. These are bounded software proofs only.
