## Latest Human simplification — 2026-10-27

The effective hierarchy is MultiLoop -> BPM Lock -> optional SYNC. SYNC uses
automatic source beat maps and follows beat phase; the separate REALTIME BPM
control is removed. This supersedes the earlier separate Fixed/Mapped choice
below. Saved schema1 sync/realtime_bpm/revision and historical seals remain
unchanged; one shared runtime projection sets realtime_bpm=true, including
neutral revision0, without rewriting saved preferences. Legacy tracker actions
address SYNC. Native retains its old protocol modes for compatibility, but old
Fixed audio cannot confirm a newly requested Mapped state.

Reuse the original analysis carrier, source-only Native claim, map jobs, selector
pending owner and durable asset publication. Consume before tempo/key callbacks;
require source/content/intent currentness through durable publication. Exported
map-only jobs stay alive until strict terminal retirement, then release naturally.
Manual/Clear wins; a retry cannot consume twice or replay core metadata effects.
Fresh standalone NN acquisition requires requested SYNC and defers primary
analysis. Saved automatic maps restore with source verification without NN.
Explicit manual selections are never replaced automatically.

New map rows bind the existing source grid origin and Native authority revision.
Physical A/B markers use the loaded rate; original map timestamps and decoder
provenance retain source seconds. Operational bars contain four detected quarter
notes without falsely verifying meter/downbeats/counts. Gap/uncertainty, source,
ACK, final-claim, callback and unsupported-representation guards remain.
Global effective/pending/detail/phase uses one active-or-unknown participant set;
only fresh unfenced source-qualified inactivity excludes a stopped PAD. No
participants grants no effective or phase ACK. No new clock or owning pipeline.

## Earlier Human hierarchy and reusable authority — 2026-10-23

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
then requests the normal control path. Missing readiness stays pending.
Activation on an empty selected slot retains that slot until its first content
assignment. Before any attempt/admission/unobserved submission, the existing
application may bind that first identity once; subsequent loss or replacement
cannot inherit it. Selection changes never elect another reference. Pair mirrors
retain this still-unbound slot when filling it, while bound references continue
to follow their original content through the existing placement projection. All
mapped-to-Legacy and mapped-BPM guards remain: this block may retain old actual
audio pending a qualified representation conversion; it cannot claim immediate
suspension of that unsupported current representation.
The existing control owner must reconcile an unapplied BPM-Lock target when the
blocking mapped/global-rate work retires or a valid reference becomes available.
One refused command cannot leave the actual parent enabled indefinitely. Reuse
bounded existing triggers and retain pending feedback; no blind queue flooding,
second scheduler, selection-following anchor or removal of the safety guard.
An admitted deferred ON also retains its original qualified source/content/accepted
timing binding, following that same content through Pair moves. Replacement or a
changed qualification cannot inherit it; requalification produces a new receipt.
Actual OFF waits for actual Fixed voices to adopt Legacy through the original
policy owner (or retire), then passes prospective source-history guards. Pending
policy/history work retains preceding actual audio and its original owner.

Observe requested and actual BPM Lock separately in the existing bounded status
storage. An admitted receipt is not an actual effect. Reuse the original control
queue and scalar mixer owner; no additional owning work queue or clock is added.

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

Hierarchy publication also revokes the existing global timing fence, under its
original request serialization and complete content claim. Accepted master refresh
and global publication must exclude each other at the actual mutation boundary;
two comparisons before a write are insufficient. Reuse the existing global marker
as a bounded one-attempt CAS claim, together with the original source/content claim.
Prepare loop cuts and master/source-history changes before the infallible claimed
effect. Contention retains the scheduled owner pending; stale identity rejects.
All derived refreshes, including loop-only effects, retain a nonowning hierarchy
stamp copied from that same global revision on successful hierarchy publication.
Master effects additionally retain the full global fence; unrelated speed changes
do not invalidate loop-only work. Retain the original per-content map/request
cancellation revision as well: PAD-SYNC or selected-map changes must invalidate old
cuts without a broad runtime-revision fence that invalidates the refresh's own
binding rebase. Ordinary launch admission preserves this cancellation revision.
Python refresh signatures and Pair mirrors retain the corresponding immutable
PAD-SYNC/map request identity, with the existing anchor placement last.
Each admitted but unapplied BPM Lock target, including OFF, also captures the
existing hierarchy stamp. Validate it at service and under the final global claim.
A newer hierarchy prevents the old target from applying even if replacement
admission fails. Report pending/reference_changed without erasing actual audio.
Python requalifies the current target once successfully per stale receipt; failed
submission follows the same bounded fresh-observation retry rule. A reserved
cancellation followed by full publication may require this replacement despite an
unchanged Boolean/source fingerprint. Applied receipts remain historical facts,
and unrelated speed publication does not revoke a current hierarchy witness.
Initial ON derives the qualified anchor's actual Native source period divided by
current speed inside this same original target transaction. Prepare all endangered
histories and the existing one-time Transport bootstrap before applying flag,
master and bootstrap together. Publish the effective rate tuple before its BPM
applied receipt; Python then mirrors actual timing without another master command.
Historical applied BPM receipts are not revalidated on ordinary authority rebases.
Every fresh qualified ON first derives its current anchor period, even if the
actual flag is already ON. Equal flag and equal actual master may acknowledge
without another rate/bootstrap effect; a changed period retains all existing
mapped/rate/history safeguards and cannot acknowledge the preceding master.
Later master-bearing effects retain an optional Copy original anchor binding and
ContentKey through the existing SetRateIntent/PendingRate and global-rate owner.
The same nonowning hierarchy stamp remains part of this Copy qualification;
parent changes revoke even pending pre-capture master work without relying on a
broad runtime revision or invalidating it for unrelated speed publication.
Both fast and delayed history commits validate the original source and the exact
current-anchor-period/requested-speed tuple under global then content claims,
including without active voices. Preserve the qualification through speed-only
coalescing and discard a rejected master from the coordinator's requested state
so subsequent speed edits cannot resurrect it. Accepted-derived refresh keeps
its established qualified path; loop-only work remains independent of BPM ACK.
A new qualified master-only request with no pending rate work takes its implicit
speed from the actual tuple, preventing revival of a rejected unqualified speed
prefix. Existing pending coalescing, explicit speed intent and unqualified Legacy
semantics remain intact.
Application master setters use a qualified period directly for both static/manual
and accepted timing, avoiding a multiply/divide BPM round-trip in qualification.
For API compatibility, an unqualified Native BPM Lock ON may retain the old
flag-only effect with its previous master unchanged and no reference election or
bootstrap. This compatibility transition preserves the complete Transport clock
even at a nonzero activation frame. An explicit OFF is a no-op only when the
actual matching flag is false and its actual master is absent; an old public
master must otherwise clear through the existing guarded effect. Historical rate
request identity remains unchanged, and later speed edits use the cleared actual
tuple rather than resurrecting its former master. Its receipt certifies that flag
only. Application activation always
supplies the original qualified binding; it cannot use this compatibility path
as evidence of source-qualified master adoption or synchronization.
Pre-capture rate service distinguishes transient completed-drain/global/content
claim contention from a genuine foreign history lane or unavailable preparation
reserve. Only the former retains the same pending owner. The latter remains a
terminal capacity refusal before any participant epoch or worker request changes;
the existing captured execution boundary and all source/history guards remain.
For accepted-derived master refresh of the already master-ready original anchor,
keep the existing waiting entry until rate state is coherently terminal. Derive
its master from effective speed; requested Project speed can still differ after
rejection. Unknown, queued, preparing or locally unobserved state must not cause a
terminal loop-only fallback that loses the later master update. Pre-BPM-ACK and
nonanchor loop-only refresh remain independent. Retain the separate rate error,
requested controls and Native exact period/source/global/history claims.
Manual/TAP/Legacy master updates use the same terminal actual-speed rule through
the existing GlobalParameters owner; automatic accepted timing keeps its refresh
owner. Reuse the existing application fingerprint after initial application as
the last successfully admitted manual source/timing qualification. Suppress an
unchanged rejected master/context during periodic reconciliation while allowing
fresh explicit or source/timing qualification. Do not replay a rejected speed.
Receipt/content eligibility must not repeatedly resolve timing: one captured
timing determines both the speed/master target and its qualified binding. Fresh
Native metadata may refuse that snapshot. Resolved BPM ON without its matching
applied original owner or current anchor timing refuses speed before any setter;
cached actual OFF cannot authorize a speed-only fallback across initial ON.
No callback spin, blocking lock, allocation or fallible partial loop/master change.
Each Native admission refuses capacity/counter exhaustion before its own mutation;
staged cancellation and later BPM admission remain separate operations with truthful
pending/error if a later admission fails, not an all-or-nothing transition claim.
Exact prior-row staging also retains its original requested-pitch journal:
a terminal unreserved pitch request cannot be consumed before old rows are restored,
or its later rollback would be lost. Ordinary complete publication after the
reservation releases still reconciles that original terminal observation.
Claim-pending master refresh must also progress through real consecutive callbacks.
During a deferred retry with an empty control queue and no deferred control, retain
only the already completed Commands witness; its recorded global revision must
still match. Unread controls cannot borrow it. Resume normal invalidation before
ordinary queue processing and retain the original drain completion checks. Master
refresh purely rejects competing rate work, an incomplete drain basis or a wrong
actual speed/lock/period target before prospective history checks. It never cancels
the foreign rate owner to make its own effect eligible. Final CAS pressure still
retains the same scheduled payload pending; source/global/content claims remain
required before the complete effect. This avoids both self-invalidating drain
evidence and waiting for a rate owner whose service is blocked by that deferred
payload. Test actual two-callback progress, unread queue refusal and foreign-rate
receipt preservation, rather than direct scheduled-command retries alone.

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
Global and explicit PAD SYNC OFF resolve to the actual Legacy target in both
feedback and live-adoption matching. A settled Legacy OFF effect cannot remain
pending merely because a former Off target name is expected. Phase-only pending
belongs to SYNC; confirmed actual Realtime BPM retains its own settled projection.
Real policy/source/map/adoption/tempo mismatches still retain pending guidance.
Global SYNC aggregation and both timing/PAD labels use that distinction. Preserve
real refusal and tempo settling; do not erase prior actual mapped audio to make a
requested parent appear off. RT configuration availability comes from parents'
qualified requested state, never actual playing or phase confirmation.

Render Key Lock, Multi Loop, BPM Lock, SYNC, Realtime BPM in the right sidebar;
remove the old bottom-bar duplicate. Retain existing tags/Learn actions. Apply the
same guards to GUI, MIDI and PAD SYNC; retain child preferences while unavailable.

## Verification and remaining gates

The existing callback-owned rate record also carries actual master provenance in
the same sequence window: absent qualified BPM reference, unknown origin, or an
original-source correlation with an optional accepted revision. Qualified absence
of an accepted revision is Manual/TAP/Legacy evidence, unlike unknown origin.
Compute the fixed correlation off realtime from the original ContentKey, source
generation and full original-source projection; omit PAD placement, authority and
resident/window/context fields. This is correlation only. Existing complete source,
window, hierarchy, claim and retirement guards continue to authorize every effect.
Actual master commits replace provenance, even for an unchanged tuple; speed-only
and failed/pending observations preserve it. An explicit master-publication bit in
the existing coordinator distinguishes intent from a period carried in the base
tuple and survives only genuine pending-master coalescing.

Python observes this proof rather than creating another pending master. It compares
the fixed source correlation against its original reference before projecting the
actual accepted revision or qualified None. Unknown/foreign/unavailable evidence
stays unconfirmed while the actual Native rate tuple remains separately observable.
Keep the historical Boolean/source-content `_bpm_lock_master_ready` fence usable
for recovery; do not make accepted refresh depend on a proof it must repair. A
manual same-period no-op requires matching actual own Manual provenance as well
as period equality. Reflect the Native provenance API version without constructing
an Engine, and retain opaque nonempty accepted revision strings at the Python DTO
boundary instead of rewriting all existing controlled test labels.

Accepted-refresh completion uses this same fresh actual observation for the whole
Session master group. The existing ticket-current check retains its source,
hierarchy and global-owner guards, but a later callback may run after that check;
copying the earlier request values afterwards could overwrite newer actual timing.
Grid completion and callbacks keep their existing independent ticket authority.

After a terminal unqualified master refusal, the coordinator's next speed-only
base period comes from actual state while its stored speed preference and rejected
receipt remain intact. Preserve a newer request already coalesced with a genuinely
pending master when the older request becomes superseded; terminal cleanup must
not discard that newer explicit intent.

Production stream construction passes the existing ownership, acknowledgements
and preparation epochs directly into the common mixer initializer. Creating large
default owners and immediately replacing them needlessly nests their initialization
inside construction of the complete mixer. Standalone test construction retains its
defaults. Supplied owner identity and state remain intact; the existing admission
invalidation happens once after successful construction, and invalid construction
cannot invalidate the supplied policy. Retain the unchanged explicit two-MiB
production constructor/callback gate in Debug and Release; a larger fixture test
stack does not replace that bound.

Variable loop edits retain their original immutable map capture while reading the
saved physical start at that capture's sample rate. General requested-region
resolution would fetch a second map under active parents, so bar/end/max queries
reuse the captured start and resnap derives an automatic endpoint from that same
geometry. Maximum-bars queries keep their existing read-only snapshot without
capturing a mutable edit or remembering residency.

After a transient feedback read or original-source Hold, a normal qualified poll
may clear a stale presentation error only for its own matching Applied BPM receipt
whose master was already projected. This does not resubmit BPM or bootstrap. Clear
before ordinary manual-master reconciliation so a new failure remains visible;
pending/foreign receipts and missing or replacement references remain unconfirmed.

Cover parent/raw/actual matrices, no-op/exhaustion before side effects, selected
reference reactivation, Legacy BPM behavior, unavailable Fixed/Mapped, Native
omitted rows, reserved cohorts and ABA, grids and complete publication. This
also covers pending reserved launch cancellation and deferred actual-parent
reconciliation. Exercise accepted refresh before/after parent changes and Pair
move/swap with matching Python/Native fences and mirror signature projection.
Recovery checks include invalid policy with both parents true, failed/absent App hold,
valid False/False0, unrelated errors, original bytes and nonserialization. Mode
ACK must not confirm phase. Reuse existing layout/reader/custody/idle/Hold/rate tests.
Portable startup reflection also requires the qualified optional anchor submission
and callback BPM receipt API before application/device construction. Reuse the
existing global-rate API preflight and forbidden-call mocks; never invoke Native
as part of this interface check.

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
