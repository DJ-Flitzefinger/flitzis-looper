## Latest Human contract and reusable authority — 2026-10-23

The effective hierarchy is MultiLoop -> BPM Lock -> SYNC -> Realtime BPM. Retain
separate raw project/content preferences; suspend child availability while a
parent is off and requalify on reactivation. This supersedes earlier usable
independent controls and the SYNC-OFF bypass of existing BPM matching. The selected
PAD at BPM-Lock activation remains its reference; no automatic selection-follow,
new master election, Transport reset or source seek is introduced.

ProjectState, existing controllers, the complete Native runtime publication and
actual callback claim remain the only paths. Reuse strict optional schema1 with
required nullable sync/realtime_bpm and u64 revision; no wire ABI change or new
persisted effective fields. Preserve historical seals, raw PAD intents and valid
other settings. Historical absence preserves wishes, now masked by the hierarchy.

## Shared resolution and parent request identity

ProjectState resolves BPM Lock from MultiLoop, its stored preference and the
specific timing-recovery gate. PAD SYNC adds its existing global/content request;
PAD variable timing adds raw RT preference. Keep raw preference helpers for
remembered UI state. Every grid, auto extent, editor, restore, accepted-timing
publication, speed/master coupling and capture consumer uses this resolution.

An accepted parent edit increments the existing checked global revision without
changing child choices. Its role is now hierarchy request identity. Check capacity
before Native or project mutation; equal setters remain no-ops. A revision proves
a new request, never actual audio. Reuse complete JSON/reserved-cohort, row/cache,
future-launch and full raw callback fences for OFF/ON ABA. Preserve ordinary
MultiLoop launch cancellation and request-only Hold behavior in their own domains.
In particular, the Python revision preflight must allow the existing MultiLoop
cancellation/supersession of a pending Pair/Bank launch; a reserved-row global
policy rejection must not prevent the parent action. Preserve complete reserved
publication until its original cancellation/ACK boundary, without partial rows.

Accepted-timing refresh uses the same hierarchy/recovery identity in its control
signature and existing Native global-revision fence. Update the Pair mirror's
signature projection together with publication; stale queued/in-flight refreshes
cannot republish an anchor after OFF/ON, recovery or a content move/swap.

Separate BPM-Lock wish from its existing actual application. Parent OFF requests
the resolved effect off while keeping raw wishes. Parent ON with remembered BPM
Lock freshly validates the existing selected reference and current source timing,
then requests the normal control path. Missing readiness stays pending. Existing
mapped-to-Legacy and mapped-BPM guards remain: this block may retain old actual
audio pending a qualified representation conversion; it cannot claim immediate
suspension of that unsupported current representation.
The existing control owner must reconcile an unapplied BPM-Lock target when the
blocking mapped/global-rate work retires or a valid reference becomes available.
One refused command cannot leave the actual parent enabled indefinitely. Reuse
bounded existing triggers and retain pending feedback; no blind queue flooding,
second scheduler, selection-following anchor or removal of the safety guard.

## Native policy and actual parent guards

Unresolved/inactive SYNC selects Legacy, preserving original constant detected,
manual or TAP timing and BPM matching only under actual BPM Lock. A remembered
SYNC=true must not override a suspended actual parent. Effective SYNC chooses
Fixed or Mapped from RT preference; unavailable variable geometry cannot silently
fall back. Actual Fixed/Mapped consumers require actual BPM Lock, not a retained
period proxy. Keep all current source, epoch, pitch, reader, rate, drain and final
claim checks and the no-active/idle recovery repair.

Keep Native raw SourceMapProjection intent and derived captured MultiLoop and
requested BPM-Lock availability rather than destructively erasing omitted-row
wishes. Resolve an inactive requested parent to Legacy before target selection;
the separate actual-BPM-Lock guard cannot substitute for that resolution. Reuse
the existing qualified runtime pitch/control capture for the BPM parent where
possible, including pre-ACK requests. Include the parent identity in the
original projection identity and retain it through selection-free copies. All
runtime/adoption/GUI/MIDI/PadStart callers obtain the same request. No new owning
queue, mutable source state or audio/rate authority is introduced.

## Recovery remains effect-safe

Malformed global metadata is detected at its existing precise validation-error
location. Set one nonserialized ProjectState recovery latch before returning the
recovered model; existing PrivateAttr is appropriate. Valid BPM Lock, MultiLoop,
speed and content preferences remain stored, but resolved timing effects and
master-coupled publication remain suspended. Existing original-byte/save fences
and material-migration launch holds remain independent defenses, including when
hold acquisition fails or restore is used without AppController.

Do not distinguish corruption using False/False/revision0 or a magic exhausted
revision: that is a valid ordinary preference. Parent toggles, equal-looking
requests and in-place mirrors cannot clear the latch. Historical omissions and
unrelated metadata errors do not set it. A fresh valid project load has normal
availability; no new repair/clear workflow or serialized recovery field is added.

## Mode facts and phase qualification

The current Native effective/target_current/settled/map fields prove configured
mode or period, not shared-grid phase. Keep these facts and their original source,
map, request and parent fences. Add a separate nullable presentation qualification
for grid phase, initially unknown until a later qualified Native witness exists.
A matching active mode is applied-mode/phase-unconfirmed, not full SYNC success.
Global SYNC aggregation and both timing/PAD labels use that distinction. Preserve
real refusal and tempo settling; do not erase prior actual mapped audio to make a
requested parent appear off. RT configuration availability comes from parents'
qualified requested state, never actual playing or phase confirmation.

Render Key Lock, Multi Loop, BPM Lock, SYNC, Realtime BPM in the right sidebar;
remove the old bottom-bar duplicate. Retain existing tags/Learn actions. Apply the
same guards to GUI, MIDI and PAD SYNC; retain child preferences while unavailable.

## Verification and remaining gates

Cover parent/raw/actual matrices, no-op/exhaustion before side effects, selected
reference reactivation, Legacy BPM behavior, unavailable Fixed/Mapped, Native
omitted rows, reserved cohorts and ABA, grids and complete publication. This
also covers pending reserved launch cancellation and deferred actual-parent
reconciliation. Exercise accepted refresh before/after parent changes and Pair
move/swap with matching Python/Native fences and mirror signature projection.
Recovery checks include invalid policy with both parents true, failed/absent App hold,
valid False/False0, unrelated errors, original bytes and nonserialization. Mode
ACK must not confirm phase. Reuse existing layout/reader/custody/idle/Hold/rate tests.

Static phase convergence, variable shared-grid convergence, actual8/64bar and
one-shot EOF remain open product work. Static grids cannot identify true tempo
variation absent from their reference. Later phase control must preserve exact
activation cursor/first PCM, both real start modes and Key-Lock-OFF tempo/pitch
coupling without stop/restart or seek. Use realistic2/4/6 and stress20, then10 when
performance-related stress failure warrants it. Old32 timings are diagnostic;
internal32 layout/correctness/ownership bounds remain.

Root serializes checks/builds after every compiled writer freezes, performs one
independent final diff/evidence review, coherent commit and normal existing-main
push without per-step approval, and verifies clean HEAD/tracking/fresh remote.
Protect the running App/held Native, original/private/denied data and all full-goal
final gates. Realtime has no I/O/GIL/locks/logging/inference/heavy allocation or
final owner destruction. Capacity failures use interruptible20/40/max60s same-step
retry without a chosen total/retry limit, model/scope change or voluntary Goal stop.
