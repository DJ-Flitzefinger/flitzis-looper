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

Stage A must close the mathematical conversion contract before any representation
conversion or Stage B general mapped admission. Independent Stage S below adopts
only scalar requests/targets on an unchanged scalar projection and reader; it
requires no map inversion and does not discharge Stage A. This explicitly replaces
the earlier broader sequencing rule that blocked all live admission on Stage A.
Existing mapped starts, master changes and qualified
Hold/ordinary Pause/Resume keep their legacy canonical projections and guards.
New anchored conversion must not silently change their established arithmetic.

## Stage S: independent scalar-to-scalar live policy adoption

Use the complete current original Native runtime row and its exact binding,
source/content/lifetime, physical window, pitch projection, launch/runtime fences
and raw policy/PAD revisions. Capture owning work off RT under existing correctly
ordered locks; callback admission repeats bounded freshness inside the original
placement claim. The map-withheld fallback snapshot is not proof of scalar row
custody. Raw timing-policy revision and numeric RateTuple revision remain distinct.

Every active lane in the fixed 32-lane cohort must have current scalar playback
and a newly requested scalar policy, unchanged actual source/window/domain and
qualified readers, zero pitch, Key Lock off, no Native/retained input or stem
transition, no affected pause/Hold or conflicting pending work. One unsupported,
stale, missing-master/period, unsupported-rate or capacity failure refuses the
whole effect before mutation, including unchanged-looking participants. No mixed
partial adoption, scalar fallback for mapped intent, new clock/scheduler or
Python cohort queue. Master tempo/epoch, placement, generation and source stay.

Resolve every prospective scalar target from its NEW captured request. Preserve
raw actual SourcePlayback; capture_voice_snapshot's old-policy normalization and
chunk must not consume a preceding glide step. Equal target adoption changes only
the request; the full pending scalar trajectory remains exact. Changed targets
use the existing set_target/.05/512 step boundary exactly once, preserving exact
E cursor/reader/PCM. Prepare/check the resulting bounded scalar state before the
complete actual-E claim. Existing read receipts are retained, not reconstructed.

Use the existing global-rate capture/preflight/claim machinery with explicit
policy effect identity; preserve rate-edit supersession/drain fences and serialize
incompatible effects. Runtime publication plus cache-hit refresh or Native-owned
pending service must retry temporary refusal. Admission is idempotent after actual
success, including while its glide settles, and repeated refresh must not flood
owning work or restart/reanchor the voice. Never acknowledge enqueue/publication.

Existing bounded source-sync status/diagnostics report actual adopted raw request,
pending/refusal identity and real scalar settlement. A target change is unsettled
until its actual glide reaches target. Real GUI/MIDI/runtime-to-callback evidence
must cover both global choices, away/back revisions, equal target/pending glide,
retarget PCM/partitions, unchanged master, retry/idempotence, all-lane refusal,
source/Move/Swap/pause/Hold/conflicting work, retirement and feedback pressure.
Stage S does not authorize any scalar/map conversion or musical reader change.

Runtime publication is desired intent, not a new transport command. A raw
SYNC/PAD/global-policy-only change must still cancel previously captured future
starts through the original launch/runtime revisions without superseding a caused
Hold's existing transport identity. Use the original prepared Cancel write and
retain only its original transport revision when every other row projection is
unchanged: loaded/source/content/placement/binding, resident window, accepted
timing, physical loop, multi_loop, pitch/latest request/anchor and ACK Arc. Selected
maps must both be absent or have the same original delivery owner, source key/
generation/rate, exact extent and map revision independently of raw request fields.
This qualification applies to all requested modes: a refused scalar effect must
not strand an existing mapped Hold. Its retained effective map/launch custody and
original continuation guards still own the eventual release/resume.

Keep the original reservations, publication-current, complete marker, runtime/
launch/current-transport checks and cancellation of pending starts. Source/window/
timing/loop/pitch/multi_loop or selected-map addition/removal/replacement retains
the existing transport-superseding cancellation. No Hold/currentness/continuation
guard is relaxed. The original Cancel counter preparation may conservatively
refuse at transport exhaustion before publication; preserve that atomic refusal
without wrapping or resetting counters.

Stage S implementation uses one immutable RuntimePolicySnapshot cached in the
existing Native RuntimeState, with exact original rows and ownership/ACK/map pins.
Snapshot capture follows existing pad-request-ids then runtime-mutex lock order.
One separate runtime-policy publication stamp includes full publication, actual
pitch-shadow reconcile/capture/apply and pair-mirror writes; commit it after real
projection changes, never from prospective local rows. Exhaustion closes only
this admission without wrapping or changing legacy controls. Actual-E stamp
freshness inside the original claim is the desired-intent linearization point.
Source generation/request/ACK fences can also change independently of an immutable
runtime row. On request, compare the cached captured fences to their current Native
values in place under the existing request/runtime locks before allocating. An
observed transition of an otherwise current cached snapshot commits one new stamp
and recaptures the original rows/fences; unchanged fences keep the same Arc.
Availability still has to pass actual-E qualification. A pending-to-ready ACK must
not leave a permanently stale cached source fence or require another toggle.

The existing scheduler carries a by-value RuntimePolicyWork containing a cached
snapshot Arc and attempt identity at callback start without eager due drain.
Normal due dispatch follows both control/parameter drains. Late/deferred dispatch
captures fresh actual E; it never extrapolates a previously captured voice.
Reserve one existing retirement entry before each owning control or due transfer;
retain the original work while scheduling so Full/order exhaustion drops only a
nonfinal clone. The snapshot cache retains no work owner. A bounded dispatch-owner
count prevents retry until every previous work owner is destroyed off RT. Final
work destruction makes a matching still-pending attempt retryable even when the
snapshot remains cached across close/restart. All terminal/refusal/cancel/close
paths use the existing worker/close owner. No owning pending effect enters
GlobalRateCoordinator and no per-retry owning allocation is needed.

An additional callback target_current observation over all actual voices of a
PAD distinguishes already-adopted target from unsettled glide. Cache-hit dedup
uses qualified actual raw requests and this observation, not cached Applied state
or settlement alone; a changed living cohort can retry the same immutable snapshot.
All scalar modes, including Legacy, report settlement only when the requested
target is current and the actual applied rate has reached it. Their projection and
glide arithmetic remain unchanged; a newly adopted Legacy request cannot borrow
the earlier unconditional Legacy settlement while its new target is still gliding.
One in-flight attempt and cached Arc reuse bound repeated requests before new
owning capture/allocation. Nested attempt feedback retains target raw revisions
and choices, current snapshot/attempt and actual execution E; stale source, row or
stamp yields no attempt projection. All previous actual policy fields keep their
meaning, and actual matching/settled callback acknowledgement retains precedence.

## Exact fractional conversion

### Independent physical scalar coordinate prerequisite

An explicit pair-preserving scalar projection mode may be constructed from an
already canonical Normal integer/fraction pair, a genuine physical loop domain
(musical_period is None), and finite initial/target rates in [.5, 2]. It is
coordinates only: no source/adoption/history permit. Reject malformed, outside,
explicit Intro/Tail or musical domains before mutation; never change an accepted
musical period to physical length. Existing constructors and legacy arithmetic
remain unchanged. No live conversion is enabled by this prerequisite.

For each constant-rate epoch, decode the binary64 rate into its 53-bit integer
significand and denominator 2^s, s in [51, 53]. The product of u64 elapsed frames
and that significand fits u128. Split the exact product into integer displacement
and a <=53-bit fractional numerator. Reduce the integer modulo physical length
before combining it with the relative integer anchor. Add only the two fractions,
normalize the rounded carry, and add the absolute source start using integers.
An exactly zero fractional displacement is the generic additive identity, including
signed zero; every whole-loop revisit retains the anchor fraction bits. There is
no actual-E-only override and no conversion of elapsed frames to one binary64.
The only rounding is addition of fractions below one, with circular source error
at most 2^-53 frame relative to exact dyadic motion, for the entire u64 epoch.

Keep the existing .05-per-512-active-output-frame glide and its epoch rebases.
Callback partitions do not rebase. Exact trajectory/rate-target comparisons include
projection mode. Copied/sliced cursors preserve that same mode and epoch. Earlier
history is unavailable; in-epoch constant rewind is allowed, and unproved shifted
history refuses. An unsupported domain change refuses before mutation. Explicit
seek starts a deliberate legacy trajectory. Checked u64 exhaustion refuses before
advance and position_at returns unavailable on offset overflow. The copied
32-lane/16-KiB bound remains. An already-Normal explicit-seek clear is a no-op
in this mode. Its inactive step timer is canonical zero once the glide reaches
target, so in-epoch constant rewind/replay restores the exact full copied state;
legacy timer behavior remains unchanged. Noninteger musical projection and reader seam remain
separate required work before general reverse conversion can rely on this mode.

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

Before musical reverse conversion, an independent coordinates-only PairMusical
mode retains an already canonical Normal q/f and the exact original Some(P) domain.
It does not change SourceReadPlan or its existing musical phase recombination.
Manual markers alone do not imply a physical domain; identical pair bits do not
prove PCM equality if scalar and mapped musical periods differ. That conversion
contract remains open after this coordinate prerequisite.

A bounded source/reader integration must distinguish observed qualification from
an arithmetic-only seam example. Use an actual cold source, original PCM binding,
accepted timing publication/ACK and independently qualified map delivery. Existing
explicitly synthetic test-only BeatThis evidence may exercise production fit and
publication, with no model, inference or acoustic claim. Derive both domains from
their original qualified paths; never fabricate an accepted projection, ACK or
source generation. A differing-period pair/tap/PCM counterexample documents why
the exact reader-domain and first-PCM guards remain mandatory. It does not enable
conversion or permit either original domain or interpolation to be rewritten.

Admit only finite P in [.5,2^64], rates [.5,2], a valid physical region and exact
|P-L|<=1. Decode period/rate dyadics on the fixed exact grid H=2^53. C=P*H<=2^117
and D=n*rate*H<2^118 fit u128 for the full u64 epoch. Reduce U=D mod C before
adding the integer anchor q*H. Compare its remaining period gap with raw f, never
combine a large q and tiny f to decide canonicality or wrap. At most one exact
period carry is needed. Euclidean splitting of the signed residual, an error-free
fraction sum and one final RN of its canonical fractional remainder give circular
error<=2^-54 source frame per constant epoch. Generic zero fractional displacement
copies f bits, including -0. Exact whole/partial carries and rounded-period equality
normalize canonically; malformed anchors are refused before this arithmetic.

Keep the mode explicit in comparisons and reuse established checked elapsed,
rebases, .05/512 glide, copy/checkpoint and no-invented-history behavior. Same raw
pair plus the unchanged plan/readers gives exact executed E taps/gains/PCM without
new pair-aware interpolation. Valid P>L may have a virtual frame at physical end;
validation therefore compares q/f with P rather than reusing PairPhysical's bound.
No production conversion/adoption caller or owning work is added here. Bounded
tests check copied layout and no-drop ownership; the implementation has fixed
integer operations and no data-dependent traversal. Primitive throughput and
callback-budget qualification remain open before productive conversion uses it.
No global ideal-glide/resource or musical/Human acceptance is inferred from the
per-epoch bound.

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
The existing dispatch counter is zero when idle and includes one cleanup
reservation throughout an active attempt. Begin publishes two for its original
Work plus that reservation; cloning adds one. Each Drop subtracts one. A previous
value of two uniquely identifies the final actual Work and leaves the reservation
at one during matching pending-attempt cleanup, then publishes zero with Release.
Unchanged service uses its existing Acquire nonzero gate, so an old destructor
cannot overwrite a newer attempt. Release is constant work without locks or CAS
loops. The scheduler route has at most three actual Work owners (original guard,
returned insertion event and stored insertion event), so the counter peaks at four.

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
Expose the already coherent callback target-current bit as optional feedback.
For active scalar playback, a complete current raw request plus that actual target
bit and an unsettled rate identifies an ordinary settling trajectory before
independent attempt diagnostics. This does not grant effective or settled ACK,
change adoption eligibility, or replace mapped geometry qualification. Missing or
malformed target evidence cannot establish this precedence.

Use real public runtime publication through the callback and actual source reads.
Cover exact fractional E/PCM, original knots/wraps and unequal partitions; all
policy directions; map-ready and cache-hit retry; full revision away/back; one bad
participant among 32; pause/Hold/source/window/master/Move/Swap/retirement pressure;
original launch and displaced map last-reader lifetimes; and feedback pressure.
Existing scalar/global-rate, launch, Hold, ordinary transport and history neighbors
remain required affected regressions. These are bounded software proofs only.
