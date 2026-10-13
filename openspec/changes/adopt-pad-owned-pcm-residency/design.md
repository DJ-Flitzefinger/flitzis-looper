# Design: equal material users and proved resident PCM

Status: R0 compatibility target, 2026-10-09, on published source
`dc5a7e5f89a1cb4147b2055746eb5a18c1fb8ce5`, tree
`a3cc602c6874da642658a67f0b1bcf09f625b38c`. R0 changes documentation/specs only;
implementation and performance acceptance remain OPEN. The runtime remains hybrid.

## Verified starting point

The published 2026-10-09 R0 symbols established these historical starting constraints;
later bounded implementations and their remaining gates are recorded below:

| Area | Current implementation | Required change |
| --- | --- | --- |
| Originals/PCM | `cold_store.rs`, `sample_loader/cold.rs` and `warm.rs`: copy-first sealed originals; complete versioned decoder/playback PCM under `samples/.pcm-cache/v1`; warm initial range read already exists | Canonical immutable material with equal assignment users; keep stable capture and full integrity rules |
| Stem disk ownership | `controller/stem_cache.py`: `STEM_CACHE_ROOT = samples/stems`, `.ready-<uuid>` and five-file marker | Material-version `stems`; short visible container, immutable generations |
| Stem playback | `stem_cache.rs::prepare_stem_buffers_from_cache`: complete PCM16 WAV decode/conversion/shared alignment, then finite views | Persist complete aligned f32 playback artifacts once |
| Window changes | `resident_relocation.rs::WindowWork::prepare`: complete FullMix read and complete WAV preparation before cropping | Direct descriptor range reads without repeated complete conversion/alignment |
| First live activation | `mixer.rs::can_accept_prepared_stems` rejects active pads; resident adoption matches only None/None or identical Some/Some | Separate guarded first residency of an already selected valid disk set from generation/content replacement |
| Mode completion | `mod.rs::set_stem_mix_mode` queues a scalar; command drain discards the mixer's result | Current-bound effective-mode feedback; queue acceptance alone is not mode ACK |
| Restore/UI | `app.py` restored-load callback eagerly publishes; `stems.py` invalidation resets saved mode; `render/sidebar_left.py::_render_stem_mix_mode` disables both buttons without resident readiness | Lazy restore, retained intent, explicit disk-backed request and truthful pending/effective status |
| DSP | `cold_residency.rs`, `resident_relocation.rs`, `source_reader.rs`: proved dry tap interval; labelled full-track KEYLOCK fallback | Actual finite continuation proof and implementation before complete loop-residency acceptance |

Existing WAVs are PCM16 outputs; the callback already reads f32 RAM. This design
does not claim WAV streaming in the callback. The gaps are repeated full conversion/
alignment, missing durable stem f32 lineage and unnecessary residency.

## Ownership and visible layout

One existing cold_store/project_assets/stem_cache lifecycle owns these target paths:

```text
samples/
  flitzis_looper.config.json
  <Originalfilename>/
    <Originalfilename>
    .pcm-cache/v1/.ready-<generation>/decoder.f32le playback.f32le manifest.json
    .pcm-cache/stems/v1/.ready-<generation>/<five aligned f32 files> manifest.json
    stems/.ready-<generation>/<five WAV files> .complete.json
  #1/<optional small membership descriptor>
  ... #216/<optional small membership descriptor>
```

Material/source/analysis/StemSet versions are immutable. A MaterialId is a stable
locator; content/decoder/transform/rate/schema and full integrity determine reuse.
Preserve original filename, extension, actual encoding and byte-exact content;
different bytes with the same basename obtain a different material version or
collision-safe internal generation. Never overwrite leased originals/manifests.
No hardlink, symlink/reparse alias, phantom origin pad or second cache implements
sharing. Copy adds references and fresh stopped content, not files or decode work.
Independent imports may reuse compatible bytes only after complete existing
content/transform validation, never by filename/mtime. New analysis/stem versions
cannot overwrite data used by other assignments, readers or voices.

PadSlotId 0..215 is fixed controller layout; #1..#216 are membership/cleanup UX.
ContentInstanceId plus nonreused lifetime generation owns mutable musical intent;
Copy creates a fresh identity, Move/Swap preserve it, accepted replacement retires it
even for identical source bytes. Current source/timing/window/NativeHistoryPermit,
SourceTicket and ACK authority remain explicit, never copied from an origin.
Existing slot-bound permits/projections need proven native remap in R2; changing
Python arrays or unloading/reloading is not continuity-preserving Move. Historical
evidence stays immutable. Stable slots do not confer action/release authority.
Save preserves durable content lineage and musical intent; reopening allocates a
fresh nonreused runtime lifetime. Saved action/feedback/hold tokens never regain
authority, even when durable content and material IDs are unchanged.

Use one authoritative contained material/slot resolver for import/restore/generation/
migration/cleanup. Reject #0/#217, native ids outside0..215, traversal, out-of-root
references, symlinks/reparse points and changed checked ancestors. Native/project/
config/journal layout commits are guarded all-or-none; reserve new refs/feedback/
retirement/action/native capacity before releasing old refs. Pre-claim failure is
no-op. Irreversible claim without ACK fences conflicts and retains old/new pins;
never invent rollback or publish a guessed layout.

Assignments across all banks, readers, jobs and remaining subscribers, queued
actions/HoldActions, old voices/history/FIFO/filter state, immutable version users
and native unload ACK are genuine owners. Remove slot membership first; empty slot
directories may retire while material survives other users. Delete Stems revokes
only that content's selection/demand. Jobs cancel only after last interest and their
physical leases retire only after actual read end. Final material cleanup is off-thread
by verified owned identity after every owner has ended. Unknown, external and private
files survive. Copies work after origin slot/bank deletion and new-process restore
without new analysis, separation or complete decoding when prepared data are valid.

The complete musical snapshot includes source, valid analysis/available selected set,
loops/excerpts/grid/manualTAP/timing, correction/base/extra/KeyLock/playback settings,
Gain/EQ, current session-derived stem mask/custom mask/preset/mutes and retrigger flag.
These musical choices become durable independent intent. Voices/cursors/meters/
progress/pressed holds/temp job handles and native tokens are excluded. Suitable
resident inputs share backing; varied ranges use separate views; each content has
independent DSP/voices/settings. No full-source pitch PCM is generated.

## Transactional migration and rollback

Migration is control/background work and never a new automatic analysis request.
P1a installs safe typed legacy/new readers and canonical new writes; P1b extends
equal all-bank owners/subscribers after J0. P2a/P2b migrate existing
assignments once per distinct verified immutable material using a versioned per-project journal whose entries name actual paths,
digests, generations and phase, with bounded scanning and exclusive contained writes.

1. Capture the **current** config/assignment revision and leased original/PCM/WAV
   bytes. Inventory every referencing pad/project and current jobs/readers. A broken
   entry reports failure while retaining its previous bytes/references. No restore
   of an older session config to obtain a historical matching hash.
2. Copy into exclusively owned target staging; verify actual original/complete PCM/
   five-WAV identities, dimensions and EOF. Rebuild path-bound manifests in a new
   generation with explicit verified old/new lineage; do not edit a live manifest.
   Missing regenerable artifacts may be rebuilt off-thread from preserved source,
   with visible costs; missing original content cannot be invented.
3. Flush and publish each immutable generation on the same filesystem, reopen its
   sealed readers and verify it again. Existing Windows atomic visibility is not
   a power-loss guarantee; inject failures at every copy/flush/rename/reopen boundary
   and restart from recognized owned staging/journal entries. Preserve unknown or
   live/unqueryable process staging. Retry matches verified content, never basename.
4. Normalized original path is part of Python `SourceVersion`. A migrated assignment
   acquires genuinely fresh source request/Native SourceTicket/ownership and actual
   ACK. Record a content-verified alias/migration relation to historical analysis and
   accepted evidence; preserve manual/TAP/markers/key/mix and original raw records.
   Complete evidence verification and fresh timing adoption re-establish CURRENT;
   path replacement alone cannot fabricate native freshness or accepted timing.
5. Stage the whole related project reference update against the captured revision.
   Commit config atomically only after matching native adoption, with crash recovery
   capable of replaying the verified config after process loss (a new process still
   needs fresh ACK). Autosave serializes with migration. A newer session edit/source
   wins; rebase/retry its exact current intent rather than clobbering it. Before an
   irreversible native claim, failure leaves previous effective audio/config intact.
   After claim without ACK retain unconfirmed files/ownership and fence dependent
   starts; do not assert safe rollback of a possibly adopted bank.
6. Keep the old config/reference mapping and old originals/artifacts as rollback
   material until all referencing assignments/projects/jobs/queues/history/voices
   retire and crash/retry verification is complete. Cleanup is by owned file identity
   off-thread, never an unconditional recursive move/delete. Reference-safe staged
   retirement removes obsolete global storage only when proven unreferenced; there
   must be no obsolete legacy-container dependency at final acceptance; the new
   canonical shared material store is intentional.

Test multiple pads/projects sharing a source, same/different basenames, duplicate
content, corrupt/partial/missing files, cancelled/stale jobs, unload/same-pad reuse,
live sealed readers, settings edits and restart at each journal phase. Restoration
and rollback use validated bytes plus fresh native ownership, not historical ACKs.
Migration must expose unexpected current data loss; autosave/GC are observations,
not inferred causes. Private models/audio/evidence are never migration targets.

The P2a application path schedules one distinct legacy material after its ordinary
startup restores have settled; canonical originals do not schedule migration.
The operation captures every related current assignment across all banks, uses
one verified material preparation and gives each subscriber a separate native
request/assignment and callback ACK. It preserves content UUID and the entire
neutral key-intent DTO, including maximum epochs. Missing unavailable stem sets
preserve ALL STEMS desire; corrupt advertised-ready sets fail visibly. No new
analysis or separation is requested.

ProjectPersistence is the sole config writer: a strict transaction ID owns the
writer fence, the current revision is captured before timing verification, and
changes arriving during an atomic write remain dirty. The coordinator reserves
new saved owners before writing, transfers them after the coherent commit and
retains old global files for the later full reference inventory. Journal phase
files are exclusive immutable records in a typed guarded metadata directory.
Incomplete/unknown records and interrupted transactions remain visible on reopen
with newly acquired writer/start fences; saved records create no native ACK.
A later current configuration carrying the exact committed alias/revision is not
replaced by an older journal snapshot. The exhaustive crash, replay, retry and
all-project final-owner cleanup matrix remains P2b.

## Persistent aligned stem PCM

Keep five WAV outputs and complete-set integrity. Produce five complete aligned
f32le playback artifacts off-thread, using the existing shared alignment algorithm
and source zero. Bind every WAV digest/length, complete source and playback identity,
loaded rate/layout/full frames, resampler/channel policy, single signed alignment
offset, alignment algorithm/version, PCM hashes/dimensions and immutable StemSet
identity in the descriptor. No independently shifted component or guessed alignment.

All additional stem PCM lives in the material version's `.pcm-cache`, not its WAV `stems` area.
The five-derivative PCM directory commits immutably as a complete set and its
manifest references the exact committed five-WAV generation in `stems`. A
versioned joint commit descriptor binds both generation identities/source/rate/
transform and every WAV/PCM digest. Write its eligible marker last, after both
directories have been flushed, committed, reopened and verified; select the pair
atomically through current guarded metadata/native adoption. Two directory renames
are not one filesystem transaction: a crash between them leaves unselected owned
generations, recoverable by the migration/commit journal and never half-ready.
No consumer may treat a WAV-only marker as the joint PCM set's completion.
Retirement retains the pair while any WAV/PCM descriptor, job, assignment or live
reader needs it, and cleans only final-owner files in each checked owner area.
Fault-injection/restart tests cover each cross-area boundary and retry/rollback.
Partial PCM is not reusable. A fresh process verifies full source/WAV/PCM integrity on the
actual sealed leases. A retained already verified descriptor may serve multiple
bounded window reads without rehashing or decoding full tracks. Disk integrity I/O
is distinct from resident-range I/O and is included in measured warm costs.

ALL STEMS renders four components; `I` remains Drums + Melody + Bass, never a fifth
instrumental layer. Instrumental derivative PCM is durable disk data, nonresident
by default and loaded only for an explicit offline consumer. Current
`PreparedStemSet`/`STEM_BUFFER_COUNT`/full availability checks assume five buffers;
P3/P4 must introduce an explicit complete descriptor plus four-component resident
readiness representation. Do not weaken five-artifact integrity by omitting one
buffer from today's check. The callback remains bounded over known four live bits.

FULL MIX generation commits disk WAV/PCM but does not retain component windows
unless explicit ALL STEMS demand or the enabled preload policy applies. Release
temporary decode/alignment/validation buffers after completion; keep live readers
and old sets until their actual final owners retire. Selection/replacement of a
new complete content set requires the separate E11-05/19 active-generation transition proof and own ACK. Offline immutable generation may proceed while playing; actual runtime replacement guards remain closed until that proof.

## Source-domain resident read plans

Reuse `CompleteSourceIdentity` and introduce/extend retained complete stem PCM
descriptors, not a second path-based loader. A read plan carries absolute source
intervals, complete identity, requested/effective revision and explicit consumer
context. P4a reads these intervals directly into final f32 views with checked seek/
length arithmetic, cancellation, byte reservation and retained immutable readers.
Unchanged views share handles; a loop edit prepares only the affected pad/read set.
Other occupied pad views remain usable and byte/identity unchanged. Initial loop/
source configuration reconciles required FullMix windows for all occupied slots.

The paired owner retains an immutable shared `VerifiedStemPair` after the fresh
complete verifier. Equivalent sealed handle copies remain in the existing asset
registry; current owners, jobs and live logical history retain the range reader.
Window preparation uses positioned reads on those PCM handles, not another
complete pair open. FullMix uses an independently opened sealed playback handle
bound to the held complete lease. Both paths construct the final f32 Arc in
bounded chunks without a whole encoded buffer or a second Vec-to-Arc PCM copy.
Admission counts distinct old backing, required new ranges and bounded scratch;
the exact existing interval shares its Arc. Revision/context changes and every
publication still use the existing request/source/timing and callback ACK gates.

Dry loop coverage is already proved for both interpolation taps and the fractional
musical seam. Accepted binary64 P, physical integer H, rate ownership and source
zero remain distinct. Seek/ALL/editor/analysis retain explicitly admitted complete
or non-loop ranges; they cannot relabel full duration from a finite window.

P4b is a separate finite DSP feasibility/proof/implementation slice. Audit actual
shared source-reader accesses, active voice position, ratio smoothing, Rubber Band
feed/lookahead, native history/FIFOs, filters and old/new stem-selection sides.
Derive coverage from executed dependencies, not a guessed halo or the 4096-output-
frame horizon. Prove preparation and continuation against a complete-buffer oracle
for current supported rates, loops/seams, seeks, pause/resume, source replacement
and native continuation at production k=0. Existing isolated nonzero-k diagnostic
fixtures may check future compatibility, but P4b does not implement production
KEY/pitch or close the original B5/K1 nonzero-k matrix. Store a
bounded coverage descriptor (possibly several finite intervals) and native/history
permit. All required samples must be causally available before they are consumed.

Preserve the 1598-frame/33.292-ms, 48-kHz/rate-2 unavailable-at-T fixture and all
original acoustic/latency gates. A map cannot manufacture future source input or
remove physical/native latency. If the supported context cannot yet be proved,
retain the labelled admitted full-track KEYLOCK fallback or report unavailable
without changing old audio. That is an intermediate limitation, **not** completion
of requested finite loop provisioning. P4b remains open until the necessary
bounded DSP implementation and regression evidence establish actual finite
coverage; do not claim universal loop-only supply from P4a's dry proof.

The finite Seek/domain vertical reuses effective-voice CaptureResidentSeek,
WindowWork and its existing control/window ACK. The whole-context range closure
is the physical loop for Normal, source prefix through loop end for Intro, and
loop start through actual complete source end for Tail. It is derived from the
actual SourceReadPlan fractional taps and SourcePlayback context; all later
Native feed/lookahead and canonical smoothing remain inside those admitted
physical unions. No guessed output-horizon halo grants readiness. Complete-backed
and non-KEY nonresident Seek plus explicit ALL retain their admitted complete
exceptions. Initial finite KEYLOCK-load/default-loop admission remains separate
required P4b/P5b integration.

Every explicit seek advances voice generation, cuts Native/FIFO/filter history
and preserves canonical rate target/smoothing intent, including equal-phase
seeks. Physical edits compare actual next left/right tap addresses and weight;
coordinate equality alone is insufficient at an edited interpolation seam.
Continuous actual next reads may retain chronology; a clamp/jump requires
clean Native/recycle admission before any old usable state resets. The callback
uses existing warmed-handle exchange and fixed adapter reset, with Native reset
and large reader retirement off-thread. Already adopted storage-only full-to-
finite continuation retains original reader pins; pending work stays exact-
window/source/timing/selection/history/checkpoint bound. Retained old source A
uses its actual original lease and fresh admitted-voice permits independently
of current bank B. No pool, registry, scheduler, fifth component, callback IO,
large allocation or total disk-duration limit is introduced. This bounded
vertical does not close whole P4b/P5a/P5b/B5 or resource/music/Human gates.

The first productive P4b vertical is stopped/current same-source NormalLoop at
production k=0 and non-unity rate. Its immutable coverage binds actual complete
source and resident window, copied read plan/playback, timing and selected stem
owners. NormalLoop's shared tap mapping closes over the physical loop interval
for every fractional phase and canonical rate chunk; the 4096 output-frame
prepare horizon does not size a source halo. Existing retained range readers,
WindowWork admission/transaction and the Key Lock worker perform finite read,
own window ACK, actual native/FIFO preparation and continued callback rendering.
Pending history stays exact-window-bound; already adopted same-source history
may survive storage-only ACK without resetting native/FIFO/filter chronology.
The second bounded context is current-source NormalLoop selection transitions
with the identical already ACKed resident selected committed StemSet. Verified
pair publication while inactive may attach Some without changing FullMix; playback
followed by the existing scalar SetStemMixMode/SetStemEnabledMask commands is the
actual Some/Some product seam. Prospective admission checks every relevant current
voice and the real target/outgoing selection before mutation. Productive coverage
checks both selection and active transition.from against the four real component
ranges and timing, including empty masks. SourceReadPlan/fractional_taps and copied
SourcePlayback remain the sole address/domain/rate authority; strict complete
source/component-owner equality is unchanged. The existing 128-source-frame ramp
and interruption policy advance by actual fractional source distance.

Source-specific requests remain deferred during active ramps. Real wet native,
input/output FIFO, cursor/loop/timing and chronological filter continuation must
be proved for the entire ramp, followed by actual settled-selection preparation
and exact adoption through the existing worker/permits. Storage-only ACK changes
no chronological DSP state. Existing SetStemPairFullMix retirement must wait for
the outgoing selection's final use; native-history/job reader pins continue to
their own final use through existing registry/recycle retirement. No new reader,
lifecycle, worker pool or scheduler is needed. This slice does not implement
initial active None->Some, retention/effective-mode ACK or ordinary StemController
warm return: the controller FullMix command retires component ownership and those
remain P5a, not a permanent-retained-fixture claim.

The bounded lifecycle implementation uses genuine finite NormalLoop at
production k=0/non-unity, with physical
loop geometry fixed. Pause/resume before and after preparation capture, settled
and smoothing rates, and admitted storage-only ACKs must retain actual position,
adopted native handle/FIFOs and chronological filter. Pending work remains bound
to its exact captured window/projection/selection/checkpoint; stale work cannot
mutate current audio. Only already adopted history may use the proved
storage-only effective-source permit while its original real readers remain.
Resume proof extends past its first callback through further real requests,
worker catch-up and adoption decisions, with independent complete PCM and
algebraic raw-native/chronological-filter references.

Same-source accepted-period refresh/clear admits only the existing chronological
projection alias with unchanged physical region and equivalent wrapped fractional
phase, including virtual P+/-H seams. Source, marker, seek and position cuts do
not acquire that alias. Current source/timing acknowledgement remains required;
old active and paused source voices reject a new-bank refresh before effects.
SourceReadPlan/fractional_taps and SourcePlayback retain actual domain/address/
rate authority rather than a second geometry or rate model.

Replacement bank B never labels an active or paused voice A with B's source,
physical region, accepted timing, FrozenStemView/transition or history. A's PCM
pin is not native authorization. A current-bank candidate captured before B
replacement stays invalid; retaining an old-voice pin cannot retrospectively
authorize it. If A needs another preparation, capture the actual retained
Voice/Source/Timing/Selection/History afresh and require its own matching permits
through existing admitted mechanisms. Test A's continued wet output after resume
and through subsequent real preparation/adoption, not merely frozen fields.

Future-bank B ranges use actual existing WindowWork preparation/admission and B's
own callback ACK. That ACK grants no native-history authority: fresh public
capture and independent Source/Timing/Stem/History permits govern preparation
and exact checkpoint adoption. Explicit retrigger remains a real DSP/history cut
into acknowledged B. Reserve retirement/admission before effects; failed
reservation leaves A's playback, parameters and owners unchanged.

Existing registry/retirement/recycle paths retain each voice, source, stems, job,
native history, reader pin and outgoing selection until its distinct actual final
use. Checked peak admission includes real old/new held backing and history,
replacement overlap, late/cancelled work and saturation. There is no hidden
complete backing, fifth live instrumental component or new loader/reader/queue.
Peak capture reuses same-pad weak PCM history and original assignment membership,
deduplicating actual backing identity. The existing resident-stem ownership gate
serializes WindowWork peak/read/component preparation/registration off realtime;
partial cancelled allocations unwind before the next window admission. The
history mutex spans source-range read/registration and ends before asset locking.
No unchanged two-worker window-I/O concurrency or total native/FIFO process-RAM
measurement is claimed.
Actual complete PCM shared between pads may resolve more than one assignment;
the existing weak history conservatively charges all associated live component
backings once per allocation. This can reject earlier near the 1 GiB operation
limit; exact pad-exclusive attribution is not claimed. Equal paths or content
without actual shared backing do not confer membership or native permission.
Irregular callbacks cross pause/resume, rate ramps, refresh/clear and ACK/adoption;
observe taps/feed/lookahead, handle identity, FIFO occupancy, cursor and nontrivial
output against independent full PCM/raw-native/filter references. Existing accepted
Some/Some evidence may be reused by actual unchanged source impact and supplies
no lifecycle acceptance by itself.

Finite seek/intro/tail, physical-domain cuts and every unproved context remain
guarded. P5a active None->Some, retention/effective-mode ACK and ordinary controller
warm return stay separate. The bounded lifecycle subtask is independently
accepted; P4b parents remain open and retain the complete
musical/resource/history/causality matrix, binary64 P/integer H, 119.999..123.45
BPM and <=1-loaded-source-frame 75/1000-cycle tolerance. The 48-kHz/rate-2/H16384/
C3678/U2080 gate remains unavailable at T for 1598 frames/33.292 ms. Nonzero KEY/
pitch remains B5. No new scheduler/worker lane or callback I/O, GIL, blocking,
logging, heavy allocation or unbounded work is introduced; Human alone operates
the App, devices, recording and listening. This step does not plan a full Rust port.

### Initial saved finite ON admission

Saved restore already captures absolute physical bounds and resolved global/pad
KEYLOCK intent before native loading. Its selected live extent must be derived
from the incoming verified complete source, never from the predecessor's loop
or timing. One `ResidentLoadHint` selection resolves the existing physical
boundary policy and returns the actual range/context. Cold selection, fresh
verified warm range reads, streaming preparation and prepared-material
subscriber reads use that same policy: proper partial ON is KeyLockFiniteLoop,
complete ON is KeyLockFullTrack, and absent trustworthy geometry retains
explicit complete admission. Full disk verification and metadata are unchanged.

Initial callback source and Load checks must share the same supported incoming
NormalLoop coverage rule. Their own native ACK must not accept a source that
the actual Load would reject. Both finite contexts initialize physical markers
from the incoming view; captured loop/mode cancellation and fresh source
generation checks remain in force. Reapplying ON to a genuine admitted finite
ON view is valid, while an OFF FiniteLoop view still needs the existing proved
window/mode transaction. Existing Play, timing, Native/history and source-context
guards remain the launch authority.

Real saved-ON cold/warm/prepared-source admission is separate from first wet
Native output and later source-bound 4096-history preparation/adoption. The
productive controller proof must start with captured ON before Load, exercise
own restore/source ACK and public Play at a nonneutral rate, and continue with
independent complete-PCM/raw-Native/FIFO/filter/source-clock oracles. OFF preload
followed by ON is not an initial ON-load proof. Genuine complete requests,
malformed bounds, stale intent, capacity failure and prior readers remain
meaningful negative controls. This bounded delivery does not solve ordinary
external Loader's later incoming-timing/default-eight-bars preparation or close
whole P4b/P5b, resource, musical or Human gates.

Paired `WindowWork` preparation uses the existing `retain_stem_pair` registration
as its authoritative component/file/source custody. It already retains the same
four PCM allocations, reference PCM, complete-set identity, source lease and sealed
WAV/PCM/common areas. A second generic registration adds no ownership and consumes
bounded reader capacity, so only legacy unpaired windows use `retain_stems`.
The 1024-record limit, real exhaustion rejection and all216 readiness/edit/output
obligations remain unchanged; no live owner is evicted to admit another window.

The current saved physical NormalLoop implementation and test plan are recorded in
[the bounded report](../../../docs/pcm-cache-residency.md#initial-saved-finite-keylock-on--2026-10-12).
Its four public histories combine inherited or explicit ON with public SPEED or
manual source BPM plus display target deriving nonunity SPEED. Neither variant
asserts BPM-lock admission or accepted-musical timing. Cold/fresh-warm/prepared
selection and incoming source/Load coverage checks are distinct from productive
public restore/Play, later own Native history, Pause/Resume and independent215
continuation. The bounded runtime/integration/Release/private-Python gates pass at
the report's stated scopes; ordinary external/default-loop and whole-parent
obligations remain open.

## First lazy live activation and effective-mode acknowledgement

Separate five states per source-bound pad: durable desired mode, selected verified
disk-set eligibility, requested/pending/error residency, acknowledged resident
component readiness, and native effective source selection. Generation progress
is separate. Saved ALL STEMS is restored as intent without automatic residency
when preload is off; FullMix remains effective until an explicit request succeeds.
An ALL STEMS click cannot return early merely because saved intent already equals it.

Disk-set selection must be established for the current source without active
generation/content replacement. First residency can attach the **same selected
committed set** while FullMix is already playing; it cannot select a different
set, adopt a late generation, or evade inactive replacement. P5a adds a typed
residency-only transaction for None->Some, separate from general publication.
Its selected-disk authority is pad/source-bound and proves complete content/cache/set
identity and a current source ticket. Fresh payload/permits capture the actual current
voice/source/request/STOP epoch, window/revision/geometry/accepted timing, selection/mask,
Native history/checkpoint and real SourcePlayback/SourceReadPlan. Before attachment,
recheck both fractional taps, smoothed-rate feed and actual FullMix/outgoing and stem
target sides against held ranges, with leases and retirement/feedback already reserved.
Disk eligibility, saved intent, another pad's set or a historical ACK cannot replace
that authority. Old-source pinned active/paused voices retain their own audio, frozen
selection and readers; any operation on them needs fresh own voice/source permits and
cannot borrow the new bank's ACK. Proof covers active current voices rather than
reusing inactive readiness. New-generation publication remains separately guarded L3.

Prepare cache windows off-thread while FullMix continues. At a bounded callback
boundary, recheck native guards and atomically attach the matching handles/owned
projection. Reject stale/history-incompatible/over-budget work with old audio intact;
retry the latest context finitely, without stop/restart. After actual residency ACK,
apply the existing 128-source-frame mode crossfade using the same playhead/loop/
timeline. Preserve native history/FIFO/filter/seam correctness for both sides.
Mode adoption has a source-bound revision/feedback record: pending command enqueue
does not mean effective ALL STEMS. Report transition/adopted selection precisely,
including queue failure, rejection, superseding FULL MIX and STOP; reconcile control
without advancing or correcting the audio timeline from UI polling.
The residency ACK and own effective-mode receipt remain distinct and both bind the
matching source/request. Pause freezes actual source/ramp progress and Native/FIFO/
filter chronology; resume continues that same voice. Stopped/paused readiness does
not certify active continuing output. Cancellation, stale history/checkpoints and
capacity failure preserve prior audio and owners before mutation. Existing lanes,
retained-reader registry and off-thread retirement remain authoritative; no callback
allocation/deallocation, I/O, blocking, GIL, logging or unbounded warm-up is allowed.

Ordinary FULL MIX/ALL STEMS toggles preserve selected committed disk eligibility
independently of live residency. FullMix can cancel obsolete pending demand; the
ordinary controller retires resident components only after actual final outgoing
ramp use. A subsequent ALL request must reacquire absent ranges through the existing
worker/controller path and new own residency/effective-mode receipts, even when saved
intent already says ALL. Do not prove warm return with permanently retained fixture
components or general inactive-set publication. Other Native history/job/reader pins
retire only at their distinct actual final uses, including retained old-source voices.
Eligible idle views may stay warm within existing ownership/budget policy; loss of
resident readiness remains visible without deleting the disk set or evicting live
readers. Explicit Delete/Unload revoke track-bound intent; passive validation/load
failure does not erase saved ALL. Preserve ready-trigger reuse, pending coalescing,
click-edge behavior and original input timestamps across UI/MIDI/controller routes.
This bounded production k=0 NormalLoop P5a implementation requires real public-route
and Native continuation, paused/retained-voice, stale/failure/capacity and true-reader
retirement proof. Whole P4b/P5b, new-generation L3, nonzero KEY/B5, aggregate resource/
music and Human/device/hearing gates remain open.

## Startup policy, all 216 slots and resource accounting

Use the existing Settings surface and `ProjectState` persistence, no machine or
credential configuration. `preload_stem_loops_on_startup` defaults false for old
and new projects. Turning it on deliberately requests all eligible existing stem
loop ranges at startup and after eligible generation; turning it off stops policy
demand, while explicit demand and pinned readers remain valid. It does not download
models or generate missing stems. With it off startup prepares FullMix only,
even if saved ALL STEMS intent or five WAVs exist. State this exception visibly.

Keep 2 workers, 32 queued/reserved jobs, 8 startup admissions per poll and 1-GiB
maximum simultaneous transient scratch PCM per job, not a total on-disk extent bound. E11-03 streams complete source/stem conversion and fresh warm validation. The existing 512-MiB `constant_timing::PcmBudget` bounds
complete timing preparation (explicit maximum 1 GiB); the analysis lane has its
own 512-MiB bound. Neither is an aggregate resident-pad limit. Keep these separate
and retain existing registry/pool bounds until a measured typed-capacity change.
216 requested is not 216 ready and not 216 voices (current maximum 32). The bounded
216-entry scheduling table drains without starving any occupied slot; the public
snapshot reports requested/queued/preparing/ACKed/error counts and exact bytes.
An over-budget project shows incomplete readiness and a recoverable error/warning;
such a run cannot satisfy complete-216 acceptance.

P5b must size actual descriptor/assignment/readiness capacity for all 216
unique FullMix + four-component views **and** bounded jobs, queued adoption,
retiring old windows/voices/history. Count payloads/slots separately: current
1024 reader records count FullMix and complete StemSet readers (roughly 432 base
records for216), not each component; the separate4096 PathOwner pins,1024 retirement
requests/history-book bounds and128-entry reader/history limits also remain bounded.
The current 96 native preparation handles
are three per each of32 voice lanes, not a216-pad PCM pool. Increase only
demonstrably insufficient descriptor/registry capacities with checked reservation,
overflow/backpressure tests and the same realtime retirement guarantees. Preserve
old live-owner bounds; do not drop readers to make the target fit.

Estimate unique required allocations as sum(interval frames * channels * 4), with
content/range sharing accounted separately, plus measured simultaneous old/new
overlap, job buffers and native/DSP state. Show both a conservative estimate and
exact admitted resident/preparation costs, full-track exceptions, disk bytes and
unmeasured RSS overhead. Arc PCM bytes are not process RSS; job checkpoints are
not aggregate peaks. Derivative instrumental does not enter ordinary RAM estimate.

P5b adds an explicit **new aggregate residency** budget, separate from those
existing preparation bounds: persisted `resident_pcm_budget_mib`, integer
128..16384 MiB, default512 for old/new projects. The finite16-GiB upper choice is
a supported admission ceiling, not a promise of available host RAM. Reserve unique
resident backing plus old/new live overlap and pending resident allocations before
mutation; charge temporary job overlap to its existing separate bound and report
both. Avoid double counting shared backing while retaining each owner's reservation.
Budget reduction cannot destroy pinned readers: report temporarily above-budget,
retire eligible idle windows and reject new allocations until reconciled.

The conservative512-MiB default and deliberate adjustable finite ceiling avoid
unbounded eager loading. For illustration only,216 unique0.5-second48-kHz stereo
FullMix+four-component windows cost207,360,000B; four-second windows cost
1,658,880,000B before context/overlap/native state. These are arithmetic estimates,
not measured acceptance. Increased budget is an explicit Settings choice with
warning and visible admission failures. The user may use fewer pads or deliberately
allow more RAM. Completion requires an actual supported budget configuration with
all216 ACKed, for FullMix and preload/explicit four-component cases, plus honest
rejection beyond configured resources. No settings or partial deferral substitutes
for that successful required case. Any later ceiling/default change needs a
justified delta and measurements; this new control does not raise timing/analysis/
cold-job limits or the32-voice/96-handle pool.

## Acceptance and serial boundaries

[Tasks](tasks.md) and [extended program](../../../docs/pad-owned-pcm-program.md)
own the unchanged38-ID/48-edge C1 order including R0-CLOSURE/J0/split P1-P2,
K-META/R1-R5/V0 and genuine H-LIVE/H-FINAL. R2/R4 prove HC-01..HC-26 layout
release integration; P6/V0 repeat its resource/pitch workload. All future cases stay OPEN. Each
implementation slice freezes current source/runtime/test identities, completes
hardware-free native/control/worker/drain/render tests, affected strict OpenSpec,
maintained docs, independent nonauthor semantic and complete raw/index/blob/tree
review before coherent commit/normal publication and coordinator acceptance.
There is no parallel production, inferred next feature or full Rust-app port.

P6 measures actual baseline versus implemented cold/new-process warm/in-process
warm startup, first ALL STEMS during playback, subsequent ready triggers, single
loop edits, 216 unique and duplicate-content slots, preload off/on, disk duplication,
save verification, stale/cancel/unload/STOP/bank-switch/shutdown and final retirement.
Record file/read bytes, full WAV decode/alignment call counts, range bytes, tickets/
ACKs/mode feedback, window identity, timings, process working-set/commit/peaks and
separate PCM ownership. Repeat matched conditions; publish individual runs and
spread, not only a favorable mean. Claims require causal evidence: zero repeated
full conversion/alignment/cold jobs on valid warm paths, identical untouched pad
handles, complete216 actual ACKs, and measured RAM/latency deltas. Preserve integrity
costs and negative results; folder reorganization alone proves no speed benefit.

All original B2 T01-T05 independent references, six balanced paired T03-T05 human
sessions, absolute/20%-zero-baseline musical/default/remediation gates remain OPEN.
Beat This 1.1.0/final0/minimal becomes sole NEW analyzer only after B2 acceptance;
no selector/hidden QM fallback/autodownload. Separator selection is independent.
Already accepted OLS/corrected-legacy engineering and clickfix are not redone as
new acceptance. Permitted B3 exact-fixture offline B/S preparation follows this
feature without claiming B2 closure. B3-B8/K1/Slice7, nonzero-k/rate/unity/latency,
Rubber Band, 1/2/4/6-pad and 30-minute device/live gates stay OPEN. The concrete B6
terminal-before-pending-owner-clear race stays OPEN until its bounded fix and
meaningful ordering test; passing retries do not repair/disprove it. Human alone
operates GUI/audio devices and supplies visual/hearing acceptance and corrections
in the final pre-port phase. STOP BEFORE full Rust application port/Slice8.

Original power-loss durability and non-Windows immutable-capture acceptance also
remain OPEN; injected crash/retry tests do not substitute for those proofs.
Original model-unavailable, independent-component/atomic/freshness/E2E obligations
remain required alongside these new contracts.

## X11 bounded extension to this design

E11-01/02 use the published global boolean default plus nullable track-bound pad
override. Global changes target inheriting loaded pads only; explicit ON/OFF survives
later global actions and clearing the override restores the current default. Requested
origin colors remain distinct from actual current-window ACK/confirmed Native output
and pending/error feedback. There is no separate mutable inheritance flag. The accepted
NormalLoop/lifecycle checkpoint is the narrow prerequisite; seek/intro/tail/new pitch,
full P4b and P5a guards do not open wholesale.

The public multi-target continuation composes the existing Python Transport,
per-pad WindowWork and real callback/retirement worker without another global
audio switch. Several inherited targets and explicit ON/OFF peers retain
independent consumed-source clocks, Native/FIFO state and filter history even
when their immutable PCM is shared. Test references allocate independent complete
PCM, raw Native chronology and EQ chains per voice; each pre-EQ contribution,
post-EQ pad peak and audible mixed output is checked separately. One paused
target and one actual failed preparation target exercise different ownership
boundaries. Own pad/source/window/request feedback remains required; equal
numeric request IDs or shared PCM do not merge voice authority. Finite test
representatives do not establish complete/ALL, resource216 or Human acceptance.

Complete sharing is composed through one public Restore of fresh distinct pads.
The existing digest/device preparation gate registers complete PCM Weak before
releasing preparation, while the first producer retains its sample/lease through
own ACK waiting. Complete followers share that allocation; finite ranges and
pinned same-pad replacements retain their existing distinct-allocation policy.
The bounded test observes stopped setup source transactions through unchanged
one-message production-drain partitions, then uses normal drains for public
playback/GLOBAL/unload. It proves own mode-view retirement with audible shared
peers and final PCM readers; it neither implements public Copy nor changes OFF
saved-hint classification. Whole-domain/resource/Human gates stay separate.

E11-03 replaces complete arrays in stem_cache.rs::prepare_complete_stems_at_project_root and the full-extent warm gate in stem_pair.rs with retained-reader streaming passes. Full reference/mono scans first determine the exact global peak and then the first threshold crossing (including all-silent behavior). Retained positioned reads extract the existing pre-onset 0.5-second and four-second bounded region for the exact +/-0.25-second alignment oracle. A subsequent streaming output pass applies one signed loaded-frame offset, exact PCM16 normalization and edge zero padding while writing five immutable f32 derivatives and full digests. The old complete-array conversion is the independent small-fixture oracle. Warm verification streams all leaves through EOF before reuse. Use existing sealed/source/range readers and job lanes; no callback file access or new pool. Memory peaks include scratch, worker overlap, resident old/new users and process RSS separately; actual Demucs inference remains a separate measured limitation. No total-duration cap, cap raise or complete multi-GB allocation substitutes for the streaming implementation. User loop/ALL demand and aggregate budget remain authoritative.

The same bounded preparation also covers upstream FullMix import: sealed original
packets write decoder PCM, then one continuous Rubato state reads those complete
disk frames and writes playback PCM with the unchanged delay/extent/tail policy.
Warm verification separates full disk geometry from 64-KiB integrity scratch.
Material preparation holds complete identity plus a bounded metadata seed;
subscriber/reference reads retain their actual captured integer frame ranges.
Automatic-analysis import retains the selected live view and attempts analysis
under its separate 512-MiB conservative bound. Matching source success carries
either real analysis or its independent visible error after native ACK. Stale
source/timing results cannot override newer manual state; genuine ALL remains
subject to the live allocation and overlap limits.

The ordinary Python UI import still supplies no resident hint, so its initial
live allocation requests the complete source. Default eight-bar initialization
occurs only after source success and cannot rescue that initial admission.
Supplying the actual default/user loop before allocation remains open within
P4b/P5b; bounded storage and finite Native/restore proofs do not close all of
E11-03/X11-LONG. The 2026-10-11 storage proofs use KEYLOCK off. Current known saved
physical ON demand uses the shared incoming selection above: proper partial
KeyLockFiniteLoop or complete KeyLockFullTrack, with unknown/no-hint demand still
complete. Its bounded runtime gates pass. Initial BPM-lock/accepted-musical admission
and ordinary external Loader incoming-timing/default-loop preparation remain required
P4b/P5b work. Automatic analysis is still requested and is not silently disabled.

Complete PCM16 storage uses RIFF for small files and checked RF64/ds64 u64 extents
for large files through Python project/model writers, native WAV parsing and the
registered Symphonia probe with explicit RF64 provenance. Full EOF/content checks
are mandatory on every consumer and publication boundary. Demucs inference still
owns its original model/tensor memory; its output adapter bounds host conversion,
not model activation memory. Stream scratch reservations, actual allocator peaks
and process working-set/commit/peaks are separate evidence. New active-generation
adoption, aggregate P6 resources, Human device/hearing and full Rust-port work are
outside this delivery.

E11-08..10 separate hidden full-content identity from `samples/<Originalfilename>/` visible roots. First canonical verified root and original leaf win for equal bytes under different names; unsafe/case-colliding directories use a readable prefix plus sufficiently extended content SHA suffix. Exact supported original leaf names/bytes stay intact. A guarded full-SHA claim precedes the existing derivative gate. Hidden `.material.json` is a locator, while held incoming/canonical readers and complete SHA/length/EOF verification decide reuse. Compatible hits create no temporary full-source snapshot, additional original leaf, complete decode or inference. Decoder/rate/channel and actual executed model/config/producer transform compatibility remain separate; missing historical producer provenance stays unknown. Native material_paths/cold_store/project_assets plus Python material migration/persistence share one resolver and existing journal. Copy-to-new-root and atomic current-reference/config commit precede eventual old-root retirement; no rename beneath readers, no unknown deletion, no saved ACK replay or data reset. Source/window/timing/pair ACKs remain separate, current-bound rights. Full X11-MATERIAL integration still depends on open LONG/P4b/P5b and applicable original/final gates.

The supported producer sets contain 37 Demucs or 33 BS exact files within Native's
64-file guard, including the bounded execution-policy source and empty host package
initializer. Held-source compilation checks the relevant actual loaded host
functions/classes/properties/defaults/constants/import aliases and supported
decorator wrappers before known compatibility is stamped. Required unsupported
bindings remain UNKNOWN. Exact held package roots enter the immutable request,
precede inherited child paths under `PYTHONSAFEPATH=1`, and each child owns a fresh
empty bytecode namespace with writes disabled. Actual tools still require their
matching image/FileID witnesses and exclusive child-returned receipt. Normal
Python imports, dependency versions and narrow OS runtime policy remain explicit
limits; this is no global or hostile-interpreter attestation. Composed fixtures
substitute declared neural output and, for BS, an owned source-verified asset
verifier; there is no production-global exemption or neural/resource/music PASS.
Current evidence and pending final gates are maintained in
`docs/readable-material-validation.md`.

Concrete implementation slices, dependencies, source seams and full acceptance gates are in [the extension](../../../docs/pre-rust-extension-20261011.md). This is engineering planning only; original accepted/negative evidence remains scoped and immutable.

### Ordinary incoming Loader preparation

This bounded delta is implemented with a passing software gate and independent
final review. Split the existing cold/warm
verified disk-artifact preparation from selected live PCM reading. Determine
incoming activity with the active fixed any-channel tolerance rule and incoming
BPM with the existing analysis and separate budgets; release temporary complete
analysis PCM before reading the final live interval. Cold and warm use one
verified selection policy. No predecessor range, BPM or Native receipt supplies
incoming authority. Unknown BPM retains Auto8bars without a musical endpoint;
absent newer trustworthy physical geometry, complete demand remains genuinely
allocated and bounded. Saved explicit geometry/material subscribers retain their
separately verified paths.

Use the existing ProjectState global-default/nullable-override resolver for
request-bound incoming demand, with a revision covering relevant mode, loop and
timing edits. Reselect off-thread or reject stale work before adoption. At matching
source claim, an off-thread intent-update CAS and callback claim have one atomic
order. An edit after the claim returns already-owned status without cancelling
the actual candidate; its later domain revision settles after the matching old
source receipt through the existing source-bound controls. Rejection retires the
single actual incoming message atomically, including a raced intent-update gate.
Source timing validates finite positive period, frame extent and representable
BPM before mutation and again before callback effects. At matching
replacement completion only the predecessor override ends; an explicitly newer
incoming override survives. An action bound only to the current old source cannot
silently transfer to the new assignment. Do not add another inheritance flag or
mode authority.

Generic live gestures capture a validated actual current source generation before
reading or changing project intent. The same generation travels through native
admission, queued scalar controls, resident retries and delayed start execution.
A pending ordinary source without an actual current binding rejects before
project/session mutation. Project-derived generic controls and starts also reject
until the matching Loader receipt when Native has already adopted the new source;
MIDI readiness and global starts use that same boundary. Pending global defaults
update the existing addressed incoming resolver rather than predecessor project
overrides. Callback checks use the actual source generation;
intent admission may already have revoked the prior timing authority. Stale BPM
rows are rejected before coalescing and again before application. Saved and
maintenance callers retain the existing optional-guard default.

The existing bounded source publication carries validated incoming geometry,
scalar period and grid origin so actual adoption installs them with the source
before own ACK. Python then projects the adopted result without recomputing stale
defaults or clearing newer incoming intent. Keep every source/request/epoch,
timing, capacity, cancellation and retirement guard. First public Play must prove
nonunity SPEED and genuinely enabled initial BPM lock using incoming timing and
the current master target. Own source ACK, actual wet output and later exact
source-specific Native history adoption remain distinct. Independent complete
PCM/raw-Native/EQ/FIFO/source-clock and unrelated215 oracles remain required;
this block does not close whole P4b/P5b, aggregate resource/music or Human gates.

### Public complete-source Play and unchanged ALL return

The existing Loader and Transport controllers now prove matching assignment
projection, public waveform ALL, resident/control ACK, guarded ordinary Play and
actual START feedback on complete OFF/ON sources. A shared off-realtime physical
ON classifier corrects the reproduced complete-context error without changing
partial ON, OFF hints, geometry, admission or finite Seek captured-pin lowering.
Independent complete PCM/raw Native/FIFO/EQ/phase/wrap/later4096 and Pad215 oracles
retain exact audio. Tracked ON has its own armed receipt before launch and actual
waiting/dry-to-wet readiness after birth; no harness creates the tested Pad0 voice.
The actual selected committed set retains its disk/registration identity through
ALL STEMS, FULL MIX and fresh ALL, distinct current receipts, paused consumed-frame
ramp/Resume and actual final-reader retirement. Public finite excursion checks
only own geometry/context/window ACK; separate finite rendering suites remain
required. Actual quantized rendering rejects captured stale source/window/launch
authority before ordinary START or exclusive unrelated STOP, with valid due starts
as counterexamples. See the bounded public report for final validation disposition.
No new loader, worker pool, lifecycle framework, callback work or parent closure.

### Public active ON-to-OFF proof

The existing public complete-source birth has an optional test-only follow-up;
the original two histories retain their full chronology. Four actual public pad
OFF representatives cover running/paused FullMix and ALL at nonneutral rate.
Own requested intent remains separate from pre-ACK wet truth. The actual own
mode/window ACK preserves voice/source/loop/EQ and clears current FIFO/history/
wet readiness; historical adopted request and reusable handle are not wet truth.
Independent complete PCM/dry/EQ/source chronology and unrelated215 remain exact.
The old ON view retires through the collecting callback sink/reconcile and real
Native preparation worker, while shared current PCM/components stay owned.
Subsequent own public FULL MIX and actual final readers release ALL components
before STOP. This is not production retirement-queue pressure or every reader/
domain/music/device proof. No production code/build input or lifecycle changes.
The bounded report records integration/review disposition and unchanged evidence
reuse; full E11-01/P4b/P5a/P5b and Human gates remain required.

### KEYLOCK feedback/preparation phases

An existing warmed unique Native handle can render initial wet audio from the actual current feed after a supported mode/window change; a later source-specific preparation candidate adopts only at its exact output-frame/SourcePlayback/read-plan match (existing4096 output-frame horizon). Mode/window callback ACK, actual wet processing and source-candidate adoption are distinct. Do not add an unconditional candidate wait or treat WindowACK as source Native adoption. Pause/resume follows actual SourcePlayback, not elapsed global clock. Existing Residency requested/previous/owner, effective/error/unconfirmed, eight-attempt/30-second retry and claimed-tail semantics remain the authoritative status pipeline. Global default changes collect bounded per-pad outcomes only for targeted inherited requests and expose partial/mixed failures separately from the default and explicit overrides; unchanged requested values are not proof of convergence. Current controller/UI source seams must be reread against the implemented result.

Pure KEYLOCK intent has no implicit physical loop or seek edit. If an existing
pending transaction contains a real geometry/seek change, a superseding mode
request retains that coupled intent. Repeated identical pending intent retains
the same work and deadline. Each callback mode application carries its own
monotonic request identity, current source fence and window revision. A bounded
scalar snapshot reports the applied mode and whether stopped/paused playback is
armed, playing output is dry/wet, or preparation is waiting/failed. Control reads
recheck current source identity; accepted window publication alone cannot settle
an active ON request without usable wet processing. No source-candidate adoption
is implied by this snapshot.

The same Residency authority supplies separate confirmed Native/pending/error display
selectors. The main global color reflects the default; PAD green/red reflects explicit
requested ON/OFF and is neutral for inheritance. Adjacent GLOBAL is default green/red
only when inherited, otherwise neutral; its action clears only that override. Native
confirmation or failure cannot rewrite origin. Global changes visit affected inherited
loaded targets across banks even when one fails; a local action changes only that
target and later globals preserve it. Nullable `pad_key_lock` with strict integer
`key_lock_intent_version: 2` represents origin without another inheritance flag,
playback queue or source owner. Same-default actions preserve explicit overrides;
per-target pending reuse and the existing BPM-lock no-op/anchor guards remain required.
