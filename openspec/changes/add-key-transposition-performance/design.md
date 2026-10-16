# Independent metadata, pitch intent and admitted events

Status: the Key/Transposition GUI, semantic MIDI actions and captured Native pitch
path are implemented as an engineering delivery. Full B5/B6 acoustic, history,
deadline, resource and Human acceptance remains open.

Use existing ProjectState/PadController/LooperAction/input routing and Rust
source-reader/processor/preparation/scheduler ownership. Current manual_key is
metadata. GUI explicit content and Native selected-content capture share the
original CapturedPitchAction; selection ordering is retained before pitch input.
Keys use MIDI kind/channel/number without device identity. Retain existing
multi-port/channel collision rules and prove the two-device workflow rather than
invent current per-device binding support.

Persist recognized SourceKeyVersion, separate optional correction+epoch, numeric
base_shift, numeric extra_shift and retrigger flag. Labels derive from corrected
source root+base and root+base+extra. Correction/removal preserves both numbers;
successful deliberate analysis admission advances epoch and removes only the
preexisting correction. A correction made after admission wins against late results.
Failed admission preserves state; restore is not analysis. Accepted true source
replacement creates fresh lifetime and neutral source-bound values; copy/move/restore
preserve them. Legacy manual_key becomes metadata with base=extra=0, retriggerOFF.

For same-mode absolute target, d=(target-source) mod12, d>6 => d-=12, fixed +6 tie.
Thus base=-5..+6, extra all37 integers -18..+18, total k=-23..+24, h=2^(k/12).
For equal-rate existing varispeed route lockON p=h/r, lockOFF p=h with audible r*h.
At r=.5..2 this desires p~.132433..8; actual future r(n) extrema are part of B5.
The new pitch-intent path validates actual p without the legacy .5..2 compensation
clamp and uses Native for nonunity p even with lockOFF. Positive-finite API checks
alone do not establish support. B5 establishes complete finite context/quality/RT/readiness/latency/
unity transitions. A concrete demonstrated limit may narrow one common validated
UI/MIDI/storage/audio domain with visible rejection, never silent clamping or tempo
changes. Old diagnostic corners remain regression cases, not replacement coverage.
All selected stems share one trajectory/k; input backing is immutable, per-content
DSP/voices separate. No added complete pitch PCM or serial second shifter.

An authoritative admission captures unique action/attack sequence, original input
timestamp, explicit GUI content or native selected ContentInstance+lifetime, full
base/extra/keylock/render tuple, source/timing/native/history/prepared permit and
existing trigger/startpoint/Quantize/SYNC intent. Pure polling/preparation is not
accepted action. Direct native/fallback/pending paths use the same envelope and
target; Move/Swap follows living identity, removal fences it. Capacity/unready/
unsupported failures are atomic and visible. Prepared work may share/coalesce;
each admitted attack remains its own bounded existing scheduler record and permit,
including equal-value repeated mouse/MIDI attacks. Later requests do not globally
invalidate an earlier admitted tuple. SET changes no cursor/start; SET_RETRIGGER
applies tuple and normal trigger at the same dueframe, never retunes early or toggles.

Checkbox selects GUI/new Learn variant; learned semantic variant stays immutable.
Absolute target actions encode pitch class/mode, relative ones signed integer,
not menu positions. Both variants for each target including0 coexist. NoteOff/vel0
does not attack/reset/highlight-clear or release a mouse waveform hold.

Both header-toggle menus stay open independently after selection/repeats/MIDI.
Render from common selected-content state, including closed summaries; keyboard
root is base-key root, middle0/signed steps, unique enharmonics, distinct -12/0/+12.
White/black are keyboard colors only; a contrasting selection survives NoteOff.

Selection reveal keeps an already visible key at its current screen position.
Only a selected key outside the child draw clip queues horizontal reveal; this
includes partial clipping. Unconditional recentering previously applied on the
next frame and could move a neighboring key under a rapid second mouse press.
Actual headless widget/input tests retain that repeat-click regression, draw
colors, extreme-value reveal, closed summaries and independent menu state.
Result is nominal for lockOFF, no first-sample-note claim or green/Camelot/scale UX.

All source/trigger timing, M/B/S/phi, P/H, other pads and current routing remain
independent. Native ownership/cohort/hold integration is shared with arrangement;
no MIDI PauseHold is added. Final gates and exact row coverage are in the program.

## Current implementation boundary — 2026-10-15, acceptance still open

Moving SET owns a preallocated 1024-span/262144-active-frame source-read journal
per voice and a separate immutable snapshot per accepted action. Record actual
post-chunk/pre-advance reads, including DRY, and fence read gaps and lifecycle
changes. Callback storage contains scalar clocks, plans and comparison markers;
the worker owns all Native resets, allocations and replay. Copied complete-stem
PCM identities and original publication epoch owner/value prevent old-address
aliasing. Historical plans describe data only; replay needs current held readers'
whole physical coverage and fresh strong endpoint authority.

Preserve the existing constant endpoint path only after its required actual
interval proves the same rate/read/selection contract, with deliberate constant
start priming before O retained. Otherwise replay genuine spans plus frozen F-to-E
from canonical A on the voice's O/512 lattice. Query H after configuring each
encountered pitch, expand the suffix without clipping retention, initialize with
the first completed real block's pitch profile, then replay without resets.
Restore the exact E clock/plan and adapter phase. Gaps, insufficient or evicted
history, missing old contexts and stale voice/source-trajectory fences refuse locally.
The journal epoch fences real source/rate/domain/selection/Pause/Resume edits
without discarding genuine past. Stretch retains its separate DSP preparation
epoch, so source-neutral prospective Key Lock work does not cancel an earlier
SET. A source edit restored before E still invalidates the frozen future; a later
mode tuple retains its original captured-pitch check after the earlier SET applies.

Independent Intro/Tail A-reset PCM and refusal/retry on the same public worker are
bounded engineering evidence. They do not certify an arbitrarily continuous
Native state: the separate aligned-suffix experiment records actual large PCM
differences despite equal FIFO counts. Full continuous/acoustic, broader prospective
mode, deadline, resource/216 and Human gates remain open.

Prospective nonzero Key Lock now uses the shared bounded replay through the
existing source lane. Each lane preallocates one reusable history-work Box.
Callback capture reserves storage/queue capacity before DSP hold, copies genuine
history and retains original F-to-E data. The existing snapshot projector builds
the endpoint request at original E once. Ordinary automatic requests keep their
existing catch-up path; history-bearing mode requests use the shared replay.

The worker clears and returns that same Box on success, unavailable/stale work
and failed-worker retirement, including cleanup when no Native spare is available.
A small SOURCE/voice guard travels with the fresh Native permit through strict
adoption; recycling the large snapshot cannot refresh or revoke that guard.
The extra guard does not participate in productive-history or endpoint-data
observations. All-voice claim and the resident transaction's same own final ACK
remain authoritative. The held-selection extension supports one real committed
StemSet across historical current/outgoing roles, with a settled original E.
Actual F reader roles are frozen with the projection. Worker replay resolves
each sliced plan's required roles against that pin and copied publication
generation before the same coverage and checked-read paths. Only roles the
sliced plan no longer reads may disappear; an empty mask still requires its set.
The physical pin survives even when FullMix E has no endpoint stem reader, so
endpoint comparisons use required readers separately from retained history
ownership. No new lane/queue or mutable reader lookup is introduced. Active
transitions at E, other/unheld sets and mismatched generations still refuse.
The admitted set's copied publication owner/expected value is provenance, not
live operation permission. Actual committed residency uses the same publication
for a mutable selection receipt; ordinary Play can advance its intent counter
while reusing the admitted PCM. Historical data matching therefore compares the
original stamp and real PCM/backings, without imposing old receipt freshness.
Initial publication remains fully guarded, and fresh/effective Native permits
plus source/voice guards remain the authority for preparation/adoption/continuity.
When a new independent selection operation reuses those exact admitted readers,
its preparation clones their original data-publication stamp. Only newly created
physical sets take a new data stamp; the new selection receipt still owns its own
strict operation checks and final acknowledgement separately.
Shared-cache reconstruction alone is a physical reader source, not a prior
Mixer admission. Its unbound publication placeholder is replaced by the new
operation's actual admitted stamp; only a real admitted pin preserves provenance.

For a paused participant, global E remains F+4096 while the frozen future contains
zero additional active source frames. Preparation replays genuine past only to
the retained endpoint. Paused polling services bounded recyclers without removing
the held candidate, and the same all-participant claim checks the captured paused
state at E. Resume before E cancels through existing lane and SOURCE fences;
Resume after E keeps the adopted Native/FIFO and retained source/filter history.
The strong source permit uses the existing current cell's prepared-source epoch,
or a freshly admitted retained voice's birth lifetime, independently of the lane's
request epoch. No authority checks are removed. Adoption clears wet-success flags;
paused ON remains armed until that voice actually renders after Resume.

Same-held-set active-E endpoints use the genuine-history request entry point;
ordinary/history-free requests keep their existing active-transition refusal.
The single retained StemSet can resolve both logical roles, but every required
role must independently match its actual immutable identity and strict backing at
adoption. Finite coverage captures incoming/outgoing logical owners separately,
so FullMix and empty masks cannot hide an outgoing dependency. A second distinct
or unheld set remains a local refusal.

Only after genuine replay reaches an active E does the prepared Native box receive
an optional scalar expected-transition tracker. The original request/plan stays
frozen. Productive source processing checks exact tracker phase before the read,
advances it only after successful wet source processing by the actual output
frame count and applied ratio, and keeps Some(inactive) after natural completion.
A mismatch latches loss
of the old active request's continuation proof. This distinguishes a completed
outgoing role from premature clear or a later identical restart without a new
queue, scheduler, allocation or callback Native call. The same Native/FIFO and
filter state remains through the remaining ramp and after its proved completion.
Transition progression retains a source-progress anchor, exact applied rate and
integer output-frame count per actual rate epoch. Per-sample gains use the same
anchored expression as boundary advancement. Live callbacks, copied span slices
and worker projection therefore agree bit for bit across different partitions;
only an actual rate change reanchors, and zero-frame calls leave state intact.
Exact matching includes that complete canonical state without a tolerance.
The independent A-reset PCM proofs do not establish continuous-O, acoustic,
performer-deadline, full resources or Human acceptance.

Eight public Intro/Tail cases at +24/-23, ON/OFF, compare every 4,097 adopted PCM
samples against an independent scalar/direct-C-ABI A-reset oracle, including
off-grid source-rate changes. Ownership/recycle and independent permit regressions
are separate evidence. With copied selection/backing context, additional mode
span storage is 32 * 1,105,920 = 35,389,440 bytes before work owners/queues. The
current Release host profile measured 1,415.5 microseconds median for 32 full snapshots
plus fixed future assignment; 32 journals + 64 admitted SET + 32 mode span payloads
derive 141,557,760 bytes. The canonical transition epoch adds 16 bytes per span,
or 2,097,152 bytes across these 128 span allocations. The profile actually allocated
32 journal and 32 mode span blocks; SET storage is derived from its separate
64-owner custody bound. Request inline storage is 2,384 bytes and its preparation
enum is 2,400 bytes, counted separately from spans. Whole-process
resources and performer deadlines
remain open. This bounded engineering does not certify continuous-O equivalence,
acoustic quality, the full B5/B6/P6 matrix or Human acceptance.

Paused nonstarting SET uses its existing action-owned genuine SourceFeed snapshot
and voice/source fences. It keeps the same original global E, but its frozen
future contributes zero active source frames and replay ends at retained F.
The existing paused endpoint projector, worker replay and strong action permit
then adopt the exact candidate without a separate lane or MODE permit. Public
Resume/Pause and source controls still fence stale captures even when restored;
global silence supplies no missing history. After adoption Resume keeps the
prepared owner, source smoothing and filter history.

The first automatic source-specific endpoint reserve is limited to one currently
selected Normal start, independently of whether that content is stopped or playing.
It captures a fresh physical loop-start trajectory, never a live SET/voice history.
Existing Native selection publishes owner-scoped
atomic interest; the callback reads it without the control mutex and captures its
actual settled mixer endpoint after command/parameter drains. The existing worker
owns construction and preparation on a separate single-item lower-priority lane,
leaving all64 accepted-action slots/request cells available. No automatic216-pad
Native pool or unseen-tuple/instant-burst guarantee is introduced.

A hit transfers one complete, consumable Native/FIFO bundle. Exact endpoint data
must match a fresh original action request, whose own epoch/permit replaces the
cache request without changing DSP state; the displaced request retires off the
callback. The existing strict adoption and complete effect claim remain intact.
Ordinary starts and same-applied-tuple pitch SET_RETRIGGER use the same Normal
snapshot and whole-candidate transfer. Eligibility still resolves the original
action against the currently selected endpoint descriptor, including the actual
source incarnation, window/ACK, stem owners, rate and applied tuple. A pitch hit
first captures its own exact request through the unchanged rt_capture guard.
Only that request travels with the candidate; the cache request retains no action
authority. The shared transfer clears reserve readiness at the ownership move,
including when cancellation subsequently prevents the action becoming PREPARED.
SET and changed tuples do not consume this one physical endpoint. Misses retain
the original action and existing preparation policy; this adds no unseen-tuple
cache or new timing/worker path.
Physical cache observation ignores only unrelated RuntimeProjection advances, after
fresh original action guards and its genuine own WindowACK association; cache
observation itself grants no action permission. Source/incarnation, exact window,
timing ACK, endpoint/selection/rate and actual tuple remain exact. Background work
runs only behind accepted requests at job boundaries; an already running Native
preparation is non-preemptive and is not a deadline guarantee for a cache miss.
Ordinary capture allocates only the lightweight original work envelope and validates
its render format; a miss constructs Native on the existing worker after admission.
Pitch publication now checks the existing command producer and reserves one of
64 engine-local non-cloneable work claims before allocating its original envelope
and fixed callback history snapshot. A duplicate publication is rechecked under
the same producer mutex. The claim follows queued, inflight, ready, scheduled and
retiring work; receipt cancellation/effect and runtime-slot release cannot return
it. It is the last work field, so storage and displaced owners are destroyed before
capacity becomes reusable, including stream/engine teardown. Retained lightweight
receipts do not retain that capacity.

Pitch candidates start absent. The existing worker constructs Native/adapter/FIFO
only after original request, source and permit validation; stopped SET, actual
scalar/no-op and pre-preparation refusal construct none. Format errors still fail
on control, while Native construction failures become local asynchronous
PreparationFailed receipts. Callback capture still owns preallocated source-feed
storage and never constructs Native. Adoption swaps the prepared box into the
existing voice and the displaced voice box back into work for retirement.
Thus 64 work owners bound work-held snapshots/adapters, while the existing
MAX_VOICES productive adapters are counted separately. Ordinary, mode, endpoint
and other resource owners remain separate; neither limit is a process-RAM or
216-content/deadline certificate. Source/window ACKs do not imply
endpoint readiness. A ready hit can use original T as E; misses retain the existing
declared E/rejection. Acoustic causality and B5/B6/P6/Human obligations remain open.

Custody validation exercises three real 64-owner cancelled-publication cycles,
overflow without further work/Native allocation, failed-worker and ready-cancel
retirement, retained receipts and fresh GUI/MIDI recovery with original T/E and
tone PCM. Per-engine test counters avoid unrelated process-global Native counts.
The current Debug pitch suite passes99 with1 explicitly ignored test; shared
ordinary paths pass42, and private Release/API plus84 affected Python tests pass.
These counts overlap the4 new custody tests and are not whole-program acceptance.

The numeric tuple uses one source-feed/DSP path, per-action Native owners and the
existing scheduler. Requested control rows, effective Native state and original
input T are separate. Prepared or stopped actions retain the existing target;
an explicitly unready action may expose a declared E/failure while preserving T
and the previous effective audio. An added preparation lead is not an immediate
or quantized performer-deadline certificate. The complete K/B5/B6/B7 acoustic,
finite, variable-rate, resource and Human gates remain required.

Cancellation competes with the final complete Native claim. Once that claim wins,
the effect performs only fixed stores and owner transfers; it does not revalidate
after changing part of the tuple or destroy Native/source owners on the callback.
Equal-T pitch and ordinary transport records use one total ordering domain.

Ordinary GUI and MIDI attacks allocate their original lightweight preparation-work
envelopes off the callback; misses construct Native only on the existing worker.
A scalar capture on the existing command lane asks that callback
to record the ordinary `next_grid_frame` target before any own window publication.
The later start uses that T and a separately recorded E. It freezes the actually
applied mixer tuple and adopts its own Normal endpoint history/FIFO without writing
a bank pitch tuple. Global starts reserve all participants before one scheduler
event and one complete mutation claim. Their original group container and displaced
Native owners retire on the existing retirement worker.

Transferring work into a control publication is staging. Only its successful final
source/placement claim marks it published. A failed control claim returns that exact
work to the original owner for retry; rollback detaches its origin Arc before storing
it in the origin, avoiding a reference cycle. A queued command remains distinct from
actual scheduler admission and from the audible effect.

The Python-deferred global route now retains a native group before its residency
wait, including immutable participants, loop intent and genuine native resident
dependencies. The focused seven Native regressions cover this bounded integration.
A later field-matching ticket or a scalar T token cannot replace an original
dependency. Pending edits without a Native ticket are explicitly unavailable
before group acceptance; this does not preserve an accepted gesture through
queue pressure. Native readiness deadlines and resource/acoustic gates remain
separate acceptance obligations.

Restored numeric pitch and actual Key Lock belong to the existing source-bank
publication claim before its ACK. A later Loader SET cannot protect an earlier
MIDI start. Fresh reassignment initializes neutral source-bound pitch; same-content
material migration preserves actual effective Native pitch rather than borrowing
a newer requested control shadow. Retained old voices keep their own tuple.
Finite Native read coverage is independent of actual Key Lock: lockOFF with
nonzero pitch still requires the appropriate Native context.

The first focused run exposed four fixture failures and Native internal ring-buffer
errors at extreme requested p despite a coarse frequency pass. The bounded priming
repair and subsequent 64-profile numeric baseline resolved those ring errors.
This engineering evidence does not certify acoustic quality, onset/latency, the
full deadline/resource budget or Human hearing acceptance for the requested range.
# Deferred GLOBAL ownership

GLOBAL START captures a native `CapturedGlobalStart` before any resident readiness wait. It owns the original `CapturedPadStart` members and retained `ResidentStartDependency` values from actual existing tickets. One target-capture message uses the existing callback quantization path to freeze T for every member. The subsequent publication consumes those same preallocated members and enters the existing atomic ordinary group and single scheduler. It does not read fresh requested pitch, timing, source identity or loop geometry.

A dependency retains the actual publication/state and original source binding. Only its genuine accepted adopted binding and launch permit can satisfy readiness. It follows the original content key through placement and cannot borrow a replacement ticket's ACK. Cancelling a global gesture cancels its attack owners, not the independently requested resident edit. A resident pending row without a native ticket is explicitly unavailable before global capture; this bounded rejection is not accepted queue-pressure preservation.

## Original pitch preparation across placement

Immutable pitch preparation consumes the original request's strong source/history
owners, source-feed snapshot and frozen format. It does not resolve or publish
control custody rows. Its worker guard therefore validates the captured ContentKey
against its stable cell with the same lifetime, source, authority, timing, window,
launch and action-epoch fences before and after Native preparation. The result
grants no physical slot. A historical slot number cannot authorize a new occupant.

Control admission and operations consuming control rows retain the coherent
control-placement guard. Actual mixer adoption retains coherent Native placement,
the exact original prepared permit, frozen E and complete existing mutation claim.
This distinction preserves immutable preparation through both the short Native
publication marker and a delayed control mirror without adding a retry queue,
another clock or scheduler, or reconstructing a Native candidate.

## Shared ordinary readiness

Native WindowState retains a fixed source/timing/loop/context seal and bounded
weak member cancellation flags. Equal original attacks may register distinct
tickets against that one actual physical preparation, before superseding a
proposal or allocating another range. Each attack keeps its own Work, sequence,
input timestamp, target T and scheduled effect. The record contains no attack or
adapter owner, and its cache reference is weak. Callback permits check only fixed
request/member atomics; membership registration and retirement stay off realtime.

Independent controller edits are joined only through the exact ticket retained
once by the ordinary gesture. Their actual mode request ID and WindowACK survive;
member cancellation never cancels that independent request. A later matching edit
does not supply permission. A request owned only by captured starts may cancel
its unclaimed preparation after its last live member is explicitly cancelled.
Ticket destruction does not cancel queued work.
