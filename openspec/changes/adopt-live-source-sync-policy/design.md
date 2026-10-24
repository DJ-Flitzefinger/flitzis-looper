## Current Human bug impact: idle live-timing service — 2026-10-23

Human report: loading a PAD with default REALTIME BPM ON, SYNC/BPM Lock OFF
creates repeated missing_master attempts and Refused/Pending flicker. The current
scalar effect compares mixer matching period with Transport before inspecting
participants, despite Off and matching-disabled Legacy using speed only. Native
cache service retries every terminal refusal; Python correctly exposes its changing
attempt number. The fix belongs in existing Native eligibility/service, not a new
Python queue or a console throttle.

Retain both original masters, scalar arithmetic and final cohort capture/claim.
Require matching master only for dependent targets; preserve Fixed refusal. On
terminal service use existing complete actual_current acknowledgement regardless
of earlier terminal state. Suppress only a prior NoActiveVoices while every loaded
captured row has current qualified inactive callback evidence and no other observed
active slot exists. Unknown/stale evidence permits retry; re-evaluate every tick.
Keep Busy/Unsupported/Hold/renderer/capacity/source-ACK retries. Python projects a
qualified inactive NoActiveVoices result as stopped/off without inventing ACK.

Verify actual Native queue/scheduler/reader, repeated idle service, later launches,
unknown evidence, dependent-master refusal and old recovery/custody tests; verify
Python global/PAD status polls, real refusal visibility and scalar-settling
precedence. Root serializes all Native jobs after author freeze; one independent
final diff/evidence review, coherent commit and normal main push, clean fresh
remote equality. Running Human App/held Native must remain untouched. No complete
SYNC/REALTIME adoption, RT or Human acceptance follows from this repair.

Human practical performance target is normally 2–4 simultaneous PADs, rarely six.
An optional cap of five is conditional on demonstrated need and is not introduced.
Future affected measurements must distinguish 2/4/6 active PADs and the existing
32-reader stress case, with actual Reader/Native/DSP/transition scope. Internal
capacity and all original full-goal/final gates remain unchanged.
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

## Exact numerical support and finite master horizon

Before choosing the anchored geometry implementation, an independent exact
numerical adapter uses pinned `bnum =0.14.4`, with default features disabled.
Its current `Uint<800>` API stores 800 inline bytes (6400 bits), has no mandatory
dependency with those features and is Copy/no-drop. Explicit checked operations
are required: its checked left shift alone does not reject high-bit loss.
Decode finite f64 exactly as signed integer G*v, G=2^1074; this keeps the least
subnormal fraction without a rounded inverse. One checked division followed by
checked q*D/subtraction obtains a quotient/remainder without duplicate division.
Signed Euclidean division and nonnegative rational-to-source splitting are
separate operations. The latter keeps the integer quotient, rounds only r/D once
with ties-to-even, including subnormal and rounded-one carry, and uses u128 for
the potential 2^64 endpoint before caller domain normalization. Numeric zero
does not preserve raw signed zero; original pair identity still belongs to the
converter. All capacity failures return before mutation.

Cost refinement keeps the same capacity and algebra. A shared magnitude product
dispatches zero/unit and either power-of-two operand through the existing lossless
shift. For other operands A=a*2^s and B=b*2^t, exact trailing-zero counts produce
odd coefficients. If both coefficients and their checked product fit u128,
reconstruct a*b*2^(s+t) through that same lossless shift. Bitwidth/product failure
uses the original pinned checked multiplication, not a new refusal. The full
product fits exactly iff bit_width(a*b)+s+t<=6400; no intermediate overflow is
rescued by cancellation and no representation capacity changes. Coefficient
extraction is exact because only verified trailing zero bits are shifted out.
General fallback costs remain measured alongside actual copied-reader cases.
This does not reduce 800-byte argument/copy storage or qualify callback stack.
The helper also
serves exact qD reconstruction and raw-fraction identity. Decode finite binary64
before an exact u128 significand*u32 product (at most 85 bits), then widen and
shift: the result remains below 2^2130. Check nonfinite input before a zero factor;
preserve the separate unchanged legacy-rounded-product comparison in geometry.
A phase-only checked remainder avoids the unused quotient/qD reconstruction,
with the same positive-divisor and negative Euclidean adjustment. Source splitting
still uses shared quotient/remainder and one fraction RN. Compare unchanged minsub
and separate near-one, negative-zero, virtual-period, large-epoch and general-product
shapes before/after. No timing threshold, smaller capacity, new caller or
stack/callback permission follows from these descriptive measurements.

The following conditional original-knot algebra records capacity research only;
it does not choose exact seconds*Fs over legacy rounded knot geometry. Let
I(v)=G*v, Xi=Fs*I(seconds_i), Bi=I(beat_i), B0=I(loop_start_beat), Q0=I(loop_quarters),
a=q*G+I(f), mu=I(actual_M(n))-I(actual_M(E)), with anchor segment j and forward
segment i. Set dx=Xj1-Xj, db=Bj1-Bj, dx_i=Xi1-Xi and db_i=Bi1-Bi. Then
N=(Bj-B0)*dx+(a-Xj)*db+mu*dx, V=Q0*dx, R=N-floor(N/V)*V. The source ratio is
Y/Z, where Y=Xi*dx*db_i+(R-(Bi-B0)*dx)*dx_i and Z=G*dx*db_i.
For finite binary64 inputs, |I(v)|<2^2098 and nonzero u32 Fs gives |Xi|<2^2130;
differences have |dx|,|a-Xj|<2^2131 and |db|,|mu|,|Bi-B0|<2^2099.
The three N terms yield |N|<2^4232, V<2^4229; the two Y terms yield
|Y|<2^6363, while Z<2^5304. Extra exact source-period rational reduction is not
covered. Geometry/domain/reader qualification and actual implementation remain open.

The splitter independently enforces N<2^6363, D<2^5304 and exact N/D<=2^64.
Together the latter two imply accepted N<2^5368. For fractional RN, r<D has at
most 5304 bits; its at-most1074-bit shift requires at most 6378 bits, within6400.
No wide value enters a span/lane/checkpoint. Fixed storage and bounded library
loops prove neither stack budget nor throughput. Bnum is pure Rust, needs no
new runtime DLL/bootstrap route, and its MSRV1.87 fits the existing Rust1.95
locked Maturin build; complete Portable acceptance remains a separate final gate.

The same prerequisite adds a read-only endpoint certification helper on the
existing TransportBeatProjection. With valid positive slope and finite anchor,
the unchanged rounded evaluator is nondecreasing, so finite ordered endpoints
bound every intervening query. Preserve raw endpoint bits and old plateaus;
never substitute a new (n-E)/P clock. Existing valid period checks alone do not
exclude a later infinity. Certify the actual admitted horizon, through u64::MAX
when claiming full-u64 support. No production conversion/adoption is connected.
Original geometry/seam choice, layout, costs and full acceptance remain OPEN.

## Initial prepared exact geometry intersection

The next independent prerequisite resolves an actual geometry/reader gap before
an anchored trajectory is implemented. Legacy geometry construction remains
unchanged. An explicit off-realtime consuming preparation method qualifies the
same MappedLoopGeometry owner and records a private immutable Copy/no-drop
certificate. It adds no hidden exact scan to current launches, mutable cache,
second geometry pipeline, source permit or productive conversion caller.

For this first domain, every original segment with positive-length overlap with
[B,B+Q] must be trusted; both its original seconds*Fs products must equal the
existing rounded products. Preserve the existing interval-open coverage policy:
an adjacent segment with endpoint-only contact is outside this qualification.
Require exact I(B)+I(Q)=I(legacy B+Q), without changing original B or Q. On the
first/last original segment let X0=Fs*I(seconds0), dx=X1-X0, b0=I(beat0),
db=I(beat1)-b0. Exact source closure at t is proved by
X0*db+(I(t)-b0)*dx=expected_source_grid*db: expected_source_grid is
region.start*G at B and region.start*G+I(original P) at B+Q. No clipping,
synthetic knots or rational replacement of the retained musical extent occurs.
Only touching segments are scanned; unrelated outside geometry is not a veto.

Reuse the musical-pair domain guard for finite P in [.5,2^64], exact |P-L|<=1
and physical extent. The cheap later reader test requires the complete original
domain, including Some(P) bits, to equal the retained domain. The initial
qualification does not relabel physical None as Some(L). Exact source-slope
extrema use original dx/db cross products, not rounded extrema as a substitute.
For actual frozen FPB, compare dx*G against db*I(FPB) and its factors of two for
the existing [.5,2] rate bounds. Retain the existing nominal derivative/epoch
admission first; exact comparisons are additional guards and do not bypass its
intermediate-overflow or unsupported-rate refusals. Such wide qualification
stays off realtime.

This intersection is nonempty for variable geometry, including Fs48000 knots
(seconds,beat)=(0,0),(.5,1),(1,4), B0/Q4, region[0,48000), original P48000.
At FPB16000 its segment rates are exactly1.5 and.5. This is an arithmetic
eligibility example, not fabricated accepted timing, current custody or music.
Conversely binary64 .1 times Fs10 is exactly1+2^-54 but rounds to1; a valid
legacy [1,2)/B1/Q1 loop can therefore fail the exact starting correspondence.
The prepared mode refuses that case while keeping the legacy path unchanged.

General arbitrary-knot/manual geometry still needs its own explicit contract.
Actual raw-anchor inversion, generic signed-zero identity, finite-master spans,
reader/first-PCM proof, pause/history/master transitions and copied layout/cost
remain mandatory before an anchored mode or adoption can be enabled.

## Certified anchored coordinate mode

The next independent Stage A block stores a distinct mode in the existing copied
epoch, retaining marker, frozen actual projection, address translation and first
permitted output frame. Legacy phase/continuation fields move into their own enum
branch with identical arithmetic. The anchored branch stores only raw q/f,
original E0, cached actual M(E0) and the original inverse segment; no wide value or
owning reference enters a copied span. Two copied scalar span fields retain the
original physical start and Some(P) for separate endpoint normalization; legacy
evaluation ignores them. Expected epoch/span sizes are 128/232 bytes;
compiler checks, rather than this estimate, enforce 240 bytes and 16 KiB per 32
SourcePlayback values. The original qualified owning geometry stays external.

Require the prepared exact intersection and complete original Some(P) domain,
reuse the delivered musical-pair canonical input guard, and retain nominal plus
exact [.5,2] rate guards. E0 must be below u64::MAX. Certify the actual original
M(E0)..M(u64::MAX) horizon, preserving its expression, cast plateaus and endpoint
bits. No rounded inverse or synthetic source/beat knot is introduced.

Use the preceding checked 6400-bit algebra with G=2^1074, Xi=Fs*I(seconds_i),
Bi=I(beat_i), a=qG+I(raw_f). On original inverse segment j let dx=Xj1-Xj and
db=Bj1-Bj. Cache no wide terms: derive
N0=(Bj-I(B))*dx+(a-Xj)*db, V=I(Q)*dx and prove 0<=N0<V.
At addressed output n derive N=N0+(I(actual_M(n))-I(cached_M0))*dx and its exact
Euclidean remainder R modulo V. Select original forward segment i by comparing
R with (Bi-I(B))*dx, without rounding the beat. Evaluate
Y=Xi*dx*db_i+(R-(Bi-I(B))*dx)*dx_i, Z=G*dx*db_i.
The preceding conservative capacity proof applies unchanged. Split Y/Z once with
the existing exact quotient/remainder and fraction RN. A factored optional raw
fraction hook compares remainder*G with I(raw_f)*Z (both at most 6378 bits), reusing
that same division and retaining raw bits on EVERY exact fractional identity.
Do not shift the full Y, duplicate a divider or add an E-only identity override.
The existing numeric splitter retains its canonical +0 behavior.

Compare the separate rounded whole/fraction with the original absolute exclusive
endpoint, retaining a possible whole of 2^64 until normalization. Normalize only
exact equality to original loop start/+0; an out-of-domain result refuses. The
single fraction RN bounds circular source error by 2^-54 frame relative to this
exact anchored spline per frozen master epoch. This is not an ideal master-clock,
glide, cumulative process or acoustic accuracy claim.

New fallible original-axis segment lookup uses bounded comparison callbacks and
the original coverage, with right interior/left final-knot selection. Existing
legacy queries remain unchanged. For span bounds derive C=N(current)-R(current)
once and compare candidate unfolded N against C+(boundaryBeat-B)*dx; the wrap
threshold is C+V. This avoids a wide division per boundary predicate. Two at-most
64-step searches propagate failure, rather than treating it as boundary truth.
Refresh only derived segment/bounds/endpoint. No next boundary before the finite
u64::MAX horizon creates a terminal span ending there; querying/advancing to its
endpoint is allowed, but rendering/refresh at MAX refuses without a zero-length
refresh loop.

Pause/resume translates addresses and first-permitted frame with checked u64
addition, retaining raw anchor/inverse segment/M0. A changed master initially
supports only bitwise equality of old/new actual M at E-address_shift and a finite
new addressed horizon. Keep the original M0/raw anchor, change only projection and
minimum history frame. This preserves latent exact phase without rounded rebases
or a chain of stored epochs. Unsupported master changes, including some
paused-derived cases, refuse. Same projection remains idempotent; an API without
an actual execution frame cannot authorize a changed anchored master.
The existing replacement API must also compare the immutable original anchor,
mode, marker and address translation; an equal rounded current cursor alone
cannot authorize a fresh anchor or cross-mode replacement. Both legacy epochs
retain their preceding behavior.
The replacement API itself rechecks old/new actual addressed master bits at the
current intended frame. A changed projection requires its history floor to equal
actual E; an unchanged projection requires a nondecreasing preceding floor. The
API does not trust that the caller obtained its epoch through with_timeline_at:
equal current rounded M values do not certify intervening historical evaluations.
Endpoint comparison uses exact start*G+I(P), never a rounded binary64 start+P sum.

The new coordinates-only SourcePlayback constructor dispatches through this mode
and the existing mapped copy/refresh/project/resume machinery. Exact first pair
and unchanged-reader taps/PCM are bounded tests, not actual source acceptance or
live adoption. Reverse coordinates reuse the delivered musical scalar constructor
and existing glide. No runtime publication, 32-lane claim, source authority or RT
caller is connected. Actual stack/callback throughput remains OPEN even with zero
observed Rust allocation; a headless timing profile is descriptive only. All
existing ownership, reader-domain and first-PCM guards remain mandatory.

### Cost qualification before productive callers

The 128-query span profile omits work performed by the existing copied reader.
Full PCM rendering asks for one coordinate per frame; the checked reader first
validates every tap, then renders, asking twice. Mixer endpoint preflight and
advance add work per chunk; copied span refresh also repeats initial/endpoint
checks. A test-only fixed32 cohort therefore exercises the unchanged reader,
chunk/refresh/preflight/advance and shared original stereo PCM with preallocated
buffers. Separate raw minsub/near-one/negative-zero/large-epoch and aligned or
staggered knot/virtual-wrap shapes retain actual repeated refresh costs. Compare
full and checked outputs outside timing and observe only current-thread Rust
requests. Report entire512-frame blocks against512/48000 seconds, without a
flaky timing assertion or claiming mixer/DSP/device inclusion.

Read existing Windows Release EXE/PDB unwind records without loading the App.
Record exact binary identities and fixed per-function stack allocations plus
register pushes, excluding entry return addresses and transitive callees.
Unknown symbols, chained/dynamic frames and unresolved calls remain explicit;
individual frames and successful small-stack runs cannot certify complete peak
stack or callback headroom. No current productive caller or guard changes.

### Evaluated denominator decomposition: not adopted

An unpublished experiment used D=d*2^s and N=h*2^s+l to compute exact native
q=floor(h/d), r=N-((h-(h mod d))<<s), retaining low bits and the original fallback.
Its arithmetic and independent regressions passed, but unchanged copied-reader
profiles showed no material gain. The original profile geometry already bounds
source and RN highparts to at most 26 and 64 bits respectively; raw minsub tails widen
the low part, not h. A wider-h extension therefore does not address this cost.
Smaller individual frames also introduced new callees, so no peak-stack benefit
was proved. The experiment is not in product arithmetic. Preserve its patch,
positive/negative measurements and limits; retain independent low-tail/wide-
quotient/sign/guard regressions on the existing authority. Detailed evidence is
in results.md. Next attribute the unchanged repeated query/refresh/boundary work
before choosing another optimization. No new arithmetic or realtime permit.

### Test-only attribution of repeated anchored work

Cost attribution before further optimization is test-only. Use the existing
private anchor_terms, unfolded_at, phase_at and position_on_segment authorities
on a consuming512-frame span walk; preserve each actual span/frame and prepared
phase outside timing. Compare results outside timing with the unchanged complete
span query, including raw fraction bits and seek mode. No product instrumentation,
visibility, geometry, representation or authority changes are needed. Component
times include their own calling/black-box/prepared-input effects and are not an
additive decomposition of the pipeline. In particular the old isolated virtual
profile uses offset modulo the initial span width; when that width is one it
repeats anchor identity. Preserve it unchanged and identify this separate walk.

Replay the existing first_true searches at actual refresh addresses using actual
phase/segment/targets, counting left search, the explicit u64::MAX right probe and
optional right search separately. Match replay start/end to the original refreshed
span, retain at most64 predicate calls per binary search and propagate failure.
This is a matched replay, not a literal instrumented production hotcall trace.
In addition to the five component cases, replay the existing aligned-knot anchor
and a separate terminal request at u64::MAX-1; never start a512-frame walk there.
Actual copied-reader profiles remain the separate whole-path cost evidence.

### Qualified local boundary probes

After the original refresh gates and target-range checks, retain the explicit
unfolded(MAX) terminal test. The left search has known-true output: if output>firstE,
probe output-1; false selects output, true brackets the unchanged first_true below
that predecessor. The right search has known-true MAX: probe next, then next+1
only when strictly below the known upper endpoint; a true probe selects that
first address, otherwise run the unchanged first_true above the known-false probes.
Empty/single-point intervals retain their old behavior and no neighbor arithmetic
may wrap. General first_true remains untouched; these new helpers require the
concrete already-admitted total monotone predicate, not arbitrary fallible closures.
Unexpected probe None fails closed, never ignored or treated as a target.

Totality proof under those original gates: finite f64 grid magnitude<2^2098;
u32-rate grid<2^2130; source differences<2^2131; beat/master differences<2^2099.
Initial products<2^4230 and sum<2^4231; original0<=initial<period constrains it below
2^4229. Thus unfolded<2^4231 and targets<2^4233, all below6400bits. Successful phase
and shifted finite horizon guarantee every addressed n in[firstE,MAX] is finite
and valid; actual u64 casts/subtraction/positive division/addition remain monotone
with their original plateaus. Local probes therefore cannot skip a numerical
refusal hidden at an old search point. Other source/geometry/endpoint/RN/local-rate
vetoes remain after search. At most65left+1MAX+66right=132 predicate calls versus
the old129; bounded worstcase is explicit, not a RT claim.

Update matched replay to count the same admitted search helpers and label their
local-probe-inclusive bounds; preserve prior baseline receipts. Verify against
the original binary search with pure total predicates over small/full-u64 ranges,
plateaus, invalid intervals, failures, history and actual knot/wrap/terminal pairs.
General fallible semantics need their own unchanged failure witness. Compare
unchanged isolated/component/reader profiles and final stack facts before adopting
this numerical cost change; no new layout, owner, clock or productive converter.

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

## Condition-based automatic retry and current map guidance

Ordinary Python publication/cache hits call the existing adoption method with
`service=True`; its keyword-only default `False` preserves deliberate callers.
Only automatic Unsupported suppression changes. Preserve all original pending,
cleanup, current-target and qualified-idle gates and other error recovery. Exact
reflection rejects an older Native signature before App/device construction.

Capture a fixed semantic global/32-voice observation, including older active voices
of the same PAD and all actual Unsupported blockers. Exclude cursor, output frame,
glide/transition progress and continuously changing telemetry. Prepare previous
shape storage on the heap off realtime and compare lanes without a large callback
temporary. Each mixer owns a private observation slot; the existing input-runtime
ownership selects its current slot through an off-realtime registry. Register only
after full successful boxed construction or completed explicit owner rebinding.
An old callback writes only its old slot. Checked nonreused birth/condition tokens,
unreadability, overlap and exhaustion must never create false suppression or an
audio failure. No callback registry lock, owner clone/swap or final destruction.

The actual Unsupported guard consumes once-read Copy inputs and retains those
exact inputs as refusal evidence, including later loop/domain/transition/feed
checks. A refusal token is usable only when both qualified outer observations
match that actual decision and original source/global fences remain current.
Before/after equality alone misses a false/true-at-guard/false race. Unknown or
changed evidence leaves automatic retry open. Keep original effect guards and
cleanup reservation; observation is not another publication permit or authority.

Mapped feedback captures existing selector geometry/readiness/error once after
source qualification. Explain unavailable/preparing/failed maps before generic
Unsupported, retaining technical details and preceding actual audio/status. Do not
relax cold capture or infer phase/adoption from map readiness. This bounded step
does not implement living representation conversion or shared musical grid phase.

## Compact borrowed anchored query prerequisite

The explicit compact-only path retains the general 6400-bit reference unchanged.
Optional grid statistics are collected during the existing exact geometry scan;
failure only disables this additional class. Retain constant-size exponents and
headroom rather than a table for a possible 250000-anchor map. Actual-E source and
master refinements must keep each coefficient strictly below 2^126 on a grid in
[-126,0]. The master grid comes from the unchanged evaluator: integral cast/frame
difference, actual FPB exponent, original transport anchor and cached original M0.
Finite ordered horizon endpoints bound magnitude, not interior grid resolution.

For source coefficients X, beat coefficients b, loop B/Q and original segment j,
use dxj=X1j-X0j, dbj=b1j-b0j and Cj=X0j*dbj-(b0j-B)*dxj. For original inverse
segment i, A=dxi, I=Xanchor*dbi-Ci, P=Q*A. Evaluate U=I+(actualM(n)-originalM0)*A
and R=U rem_euclid P, then N=A*Cj+R*dxj and D=A*dbj*2^(-sourceGridExponent).
Prepare I/P and forward constants once per temporary borrowed reader view. Phase
uses 256 magnitude bits; source split/rounding/normalization uses 512. Every operation
is checked, and the class proves the unchanged original intermediate/split bounds.
No cancellation rescues an original overflow. Preserve raw whole-phase and generic
fractional identity, ties-to-even/carry, and original start/Some(P) normalization.
Output fractional bits can be finer than the raw source grid; do not newly refuse
those exact rounded outputs merely because the anchor grid was coarser.

The existing off-RT epoch supplies the original inverse segment; no new inverse
constructor is added here. Borrowed views check geometry/domain/rate/B/Q, original
segment membership and the cached global endpoint. Value association is no source
ownership proof. A future retained Native reader must borrow its own original
retained geometry, not the current logical voice's replacement map. This block
does not wire that productive caller. Existing copied spans/playbacks keep their
layout and no owner. Explicit playback/reader methods use the same interpolation
and tap loops, avoiding hidden reference calls in endpoint/advance/refresh work.

Use exact outside-loop beat ordering in original bounded map searches; preserve
all fallible boundary predicates/local probes/MAX and nominal/exact rate guards.
Keep no-hidden-fallback refusal, atomic copied-state failure and original reader
preflight semantics. Compare complete consuming workloads, not only arithmetic
microbenchmarks. Require independent exact fixtures, existing reference equality,
actual PCM/layout/allocation and explicit 2-MiB tests plus descriptive 2/4/6 and
stress 20/performance-related 10. Productive conversion, callback throughput and
shared musical phase remain separate open gates.
