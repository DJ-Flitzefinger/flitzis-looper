# Content lifetime and native mutation linearization

Status: durable independent Copy (R1) is implemented. R2 placement/authority and
live-image groundwork is in progress; no public live Move/Swap or caused Hold
remap is enabled. Pause/resume still requires the caused-effect implementation
below. Reuse asset_lifecycle/project_assets/cold_store/stem_cache and existing
command/poll/ACK/journal paths; no framework or new scheduler.

PadSlotId is fixed0..215 controller layout; slot epoch changes with successful
occupant mutation. ContentInstanceId/lifetime owns mutable musical intent and
living native playback; lifetime never reused across restore/reassign/ABA.
Persistence preserves durable ContentInstance lineage and musical intent, then
reopen allocates a fresh runtime lifetime. Saved lifetime/action/feedback/hold
tokens are historical data and cannot regain authority over reopened content. Shared
immutable Material/Source/Analysis/StemSetVersion identifies data, not a pad or
PauseEffect. SourceTicket/current_binding/NativeHistoryPermit/source/timing/window
authority is current per-content proof, not copied historical evidence. R2 must
prove native remap including sample routing/SourcePlayback/DSP/history/FIFO/filter/
warm processors/GainEQ ramps/loop/STOP guards/pending results/telemetry; changing
Python arrays or voice.sample_id alone is insufficient.

Native placement uses a preallocated complete permutation of all216 physical
slots and authority cells, including empty cells. ContentKey names the engine
owner, stable cell and nonzero lifetime; each permit separately retains the
actual engine ownership Arc. SlotSnapshot separately captures
the physical slot and its monotone binding epoch. A preparation snapshot cannot
be reused as a new destination admission after remap. Existing accepted source,
timing, history and selection guards retain the same actual owners and resolve
their stable cell without scanning, allocation or locks. Actual source/timing/
window/intent changes still invalidate their original complete guards.

Pair publication and source assignment share a bounded Native mutation claim.
All fallible resource/queue/overflow checks precede claim; a failed claim changes
no image, while an interrupted irreversible apply remains visibly Pending.
Actual assignment advances the affected physical binding epoch; same-content
maintenance preserves it. Source payloads and rollback retain their committed
incarnation and Native ownership through the existing retirement lane.

Explicit intent, legacy BPM and legacy origin control writers use that same
same-content SlotWrite claim. All locks, capacity and overflow checks precede
entry; ACK revocation, cell authority, declared intent and prepared epoch are
fixed stores inside it. Queued clears carry the returned committed snapshot.
Source cleanup retains its original source/slot identity independently of later
timing intent, so a legitimate post-unload default edit cannot strand its unload.

KeyLock control admission captures a real slot snapshot even for empty saved
intent; Pending is not an empty occupant. Original request and intent counters,
restore guards, geometry, locks, resource/ACK/proposal preparation and the complete
queue payload precede one same-content claim. Direct captured cells publish the
intent/request and prepared resident owner/command together. Loaded actions retain
their content key; empty actions retain their empty snapshot. Worker final arming
uses the same boundary. External resident failure/finish uses a captured-key outer
claim and retains Pending settlement for polling retry before publishing terminal
state. Existing Cold/resident callback tails already own the marker and retain
their direct-cell completion, without attempting a nested claim.

A callback that loses the marker after dequeue retains exactly one complete
control payload ahead of the remaining queue. Retirement/feedback capacity is
rechecked before each retry. Loader delivery likewise retains an already popped
event during Pending rather than dropping or reordering it. At stream closure,
the unique preallocated callback-state box transfers over a separate one-slot
close lane to the existing retirement worker. This box owns deferred/queued/
scheduled payloads, the mixer and retirement backlog. The worker remains alive
through the close handoff and drains remaining normal retirement owners before
joining on the control thread. Worker creation is fallible before CPAL receives
the callback; no backend destructor thread releases the final state owners.

Launch admission and cancellation capture original stable-cell flags/revisions and
hold owners. Single and bulk operations use one same-content marker; bulk validates
the entire prepared participant image before any store. Payloads and cancellation
result arrays are prepared first. Pure cancellation preserves admitted flags for
an ordered stop. Terminal counters never wrap, and fresh lifetimes cannot inherit
old exhausted launch permission. Admitted callback actions resolve their retained
content keys at both dequeue and scheduled execution, while new fixed-slot inputs
still require current physical bindings.

Committed-stem worker arming uses that boundary with complete original source/ACK
and control guards. Displaced owners leave the marker before failure settlement or
final release. External failure keeps a captured retry owner through Pending and
publishes terminal rejection only after settlement. Callback committed work uses
its own outer claim and the existing single deferred lane. Public preparation
currentness follows the valid proposal until its own control-published flag is
stored after the captured control epochs; proposal arming alone cannot switch
readers to an epoch that has not been published yet. Cancelled or superseded
unpublished proposals still reject.
The existing off-realtime failure-settlement gate also covers final control
arming, acquired before the Native marker and released after it closes. External
cancellation cannot settle a status as unpublished while that transaction can
still publish its captured control. Callback paths use the existing private CAS
and direct captured-cell finish without taking this gate.

Cold/migration tickets
retain their private adoption state plus the actual eventual SourcePublication;
pre-arm cancellation prevents publication, and published cancellation settles that
captured incarnation together with its original adoption state under the marker.

The existing complete MIDI runtime replacement captures every slot snapshot and
original runtime revision before publication. One same-content mutation claim
validates the complete 216-cell image and all loaded source permissions; it then
stores the prepared revisions directly in those captured cells and exchanges the
runtime state. The previous state is destroyed after leaving the marker. A
transient marker, late overflow or stale participant leaves both views unchanged;
unchanged entries retain their established revisions and permissions.

Current-content preparation epoch arrays belong to stable cells. Control
lookups resolve the current slot once under existing request ownership, and
queued work keeps its captured Arc rather than looking up a later occupant.
Retained voices already have independent stretch preparation epoch owners;
placement preserves those exact owners and actual voice generations. A reused
cell cannot turn an old current permission into authority over its new lifetime.

Finished QM retains its original immutable ticket when placement is temporarily
Pending. Request ownership checks the actual control custody plus original
content lifetime, request incarnation, source geometry/digest/rate, preparation
epoch and Automatic intent. This grants no new publication authority. Accepted
publication prepares allocation, adoption and queue resources before claiming
the same-content marker; guard consumption and record/epoch/queue publication
commit together, and displaced evidence retires after the marker closes.
Only a valid Pending publication raises the typed BlockingIOError. The existing
controller keeps that worker outcome, ticket and immutable assessment for one
retry per poll through its existing worker. Native Pending observations retain
publication and derived-refresh observers through unreadable readers; own
record ACK and own derived ACK still precede saved grid/master projection.
The poll brackets its Native reads with a stable observation generation, separate
from layout revision or source authority. Both marker entry sites increment that
generation under their owned odd marker before claim; rejection restores layout
but retains the generation crossing. The increment uses checked bounded scalar
work. MAX is reserved as unobservable, so exhaustion cannot wrap or certify a
failed reader interval. A token mismatch retains the observer even if Boolean
Pending is already false; the next poll rechecks actual ownership.

CopySnapshot captures source, valid analysis, selected available StemSet, loop/
excerpt/grid/timing/manualTAP, source correction/numericbase/extra/KeyLock/other
playback/GainEQ/current musical mask/custommask/preset/mutes/retrigger checkbox.
Internal occupied Copy supplies the exact original target's opaque Native binding,
not an overwrite flag. The existing Copy claim replaces that lifetime with a fresh
stopped successor and retires only its old Hold/cohort/readers. Target observers
stay fenced through own-ACK/control/config/asset settlement; controller retirement
is delayed until own Native acceptance and shared jobs lose only that subscriber.
The occupied asset publication retains its exact prepared table and retirement
cursor across interrupted return and unrelated newer intent. Its distinct strict
`pad_copy_overwrite` journal retains immutable A/B, fixed C, independent source/stem
pins and actual Config receipts through terminal settlement. Startup validates
complete history and lineage before ordinary restore. Original-input replay needs
the captured A/B; recognized C acquires fresh target-only Native acceptance and
its actual saved selection's own registration/component/preload acknowledgements.
Portable relocation dispatches each complete Legacy/Pair/Copy history through its
own strict schema without granting runtime authority. Public gestures and complete
selection/Hold acceptance remain separate open R2 obligations.
Mutable settings become independent; immutable disk/input backing stays shared.
New copy is stopped with fresh current guards/native ACK, no cursor/voice/DSP
history/meters/progress/pressed/HoldAction/tempjobhandle/runtime-token clone.
Same suitable resident ranges share input; varied ranges have independent views;
each content retains independent DSP. Save/reopen after origin bank deletion uses
canonical material and fresh authority, no valid-data reanalysis/decode/separation.

Prepare one guarded immutable transaction for pair Move/Swap/Copy-overwrite or
all36 BankCopy/BankClear, including empty source target removals. Capture exact
source/target lifetime/slot/config revision, desired selection, all refs/job interests,
command/action/feedback/retirement/DSP capacity and journal recovery state. Acquire
all new refs before releasing any old. Pre-claim cancel/failure is complete no-op.
Native execution linearizes action and layout, acknowledges complete projection;
Python/project/config/journal reconcile that identity all-or-none. Irreversible
claim without observed ACK keeps old/new/action pins, fences conflicts and records
uncertainty; do not claim rollback, success or early cleanup. Crash recovery uses
verified current revisions/material and fresh native ACK, never persisted tokens.
No36 independent unload loop. Callback applies only prepared bounded fixed handles;
capacity and drain budgets are future measured implementation proofs.

Move/Swap retains living identity/cohort/playhead/DSP/history and selected content/
editor follows. Copy leaves source/holds intact and creates fresh stopped content;
overwrite/clear stops/fences only actually removed targets. Global stop-restore,
active/paused/editor/telemetry projections must remap or retire with native identity.
Fixed MIDI pad trigger/stop/selection mappings stay at slots for future input.
Already admitted selected pitch events follow live lifetime or fence removed one.
Visual pressed state is no authority. Global middle-hold output mute remains global
and restores latest intended volume independently; no content-scoped mute abstraction.

HoldAction binds ContentInstance/lifetime/AcceptedActionId/native caused PauseEffect/
control revision/playback-cohort handles+generations, input origin and matching
physical waveform right press, original timing and diagnostic slot/epoch. Already
stopped/paused has no owned effect. Enqueue is admission, not effect ACK. Release
uses native immutable guards, never selection or a Python-resolved unguarded slot.
Matching button-up/cancel delivered by existing input/control tick outside hover/
selection/editor render/focus. Release-before-ACK retains same release-requested
record; ordered native pause/release settles effective/noeffect/retired. Saturation
retains exactly one bounded request/pins with visible pending/error and polling
retry. Duplicate releases consume once. Later STOP/retrigger/resume/intentional
pause/cohort replacement revokes old claim; continuity-proven same cohort may carry.
Move/Swap carries it; Delete/reassign/overwrite/all36 removed lifetimes fence it;
Copy no token. Release versus commit must prove both legal serial orders, delayed/
stale ACK, superseding intent, queue pressure and repeated mutations/ABA. Shutdown
settles stop/retirement before final pins; physical holds never persist on reopen.

All-bank assignments, immutable versions, old/current voices, reader/history/FIFO,
job subscribers, accepted actions and native unload ACK participate in final use.
Remaining interests prevent job cancellation; cancellation does not end actual read
leases. Removed action pins protect queued handles but cannot keep removed content
playable. Checked identity cleanup preserves external/private/unknown files.

Gesture owner branches before mouse trigger/stop even at drag start. Left empty
Move/full Swap; right Copy/full overwrite without single-pad dialog; clear target/
operation preview. Cancel/self/invalid/empty source no-op. Mode warning/help preferred
bottom-left, OFF after restart, MIDI alive. Other-bank pulse; left exact other bank
copies all36 including empties, occupied target OVERWRITE/CANCEL with bank name,
source remains selected/no navigation/selfcopy. Right any clicked bank CLEAR BANK/
CANCEL including current. Hover/dragend consumed before bank actions. Full requirements
and HC-01..HC-26 below remain future proof, with R2/R4 separate native closures and
P6/V0 repeats; hardware-free evidence cannot close real H-LIVE/H-FINAL.
