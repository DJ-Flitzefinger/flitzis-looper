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
Result is nominal for lockOFF, no first-sample-note claim or green/Camelot/scale UX.

All source/trigger timing, M/B/S/phi, P/H, other pads and current routing remain
independent. Native ownership/cohort/hold integration is shared with arrangement;
no MIDI PauseHold is added. Final gates and exact row coverage are in the program.

## Current implementation boundary — 2026-10-15, acceptance still open

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
Physical cache observation ignores only unrelated RuntimeProjection advances, after
fresh original action guards and its genuine own WindowACK association; cache
observation itself grants no action permission. Source/incarnation, exact window,
timing ACK, endpoint/selection/rate and actual tuple remain exact. Background work
runs only behind accepted requests at job boundaries; an already running Native
preparation is non-preemptive and is not a deadline guarantee for a cache miss.
Ordinary capture allocates only the lightweight original work envelope and validates
its render format; a miss constructs Native on the existing worker after admission.
Pitch captures still allocate Native before admission: the64 action slots are not
a global bound on control-retained Native owners. Source/window ACKs do not imply
endpoint readiness. A ready hit can use original T as E; misses retain the existing
declared E/rejection. Acoustic causality and B5/B6/P6/Human obligations remain open.

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
