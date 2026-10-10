# Design: equal material users and proved resident PCM

Status: R0 compatibility target, 2026-10-09, on published source
`dc5a7e5f89a1cb4147b2055746eb5a18c1fb8ce5`, tree
`a3cc602c6874da642658a67f0b1bcf09f625b38c`. R0 changes documentation/specs only;
implementation and performance acceptance remain OPEN. The runtime remains hybrid.

## Verified starting point

Current symbols, not old line labels, establish these constraints:

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
  materials/M<stable-id>/
    original/<Originalname.ext>
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
new complete content set stays inactive-only, even if the artifact writer is done.

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

Finite seek/intro/tail, old voices and wider refresh/retrigger contexts remain
guarded and are subsequent P4b work. The parent
P4b tasks remain open until their complete musical/resource/causality matrix is
proved. No new reader, scheduler, worker lane or callback I/O is introduced.

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
Its payload/permits prove complete source/cache/set identity, source ticket,
request/STOP/window/geometry/accepted timing, every relevant active voice and
shared DSP/history/FIFO/read-plan context, leases and reserved retirement/feedback.
Old-source pinned voices retain their own audio; they cannot inherit the new bank.
Proof must cover active current voices rather than reusing inactive readiness.

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

Ordinary FULL MIX/ALL STEMS toggles retain valid prepared windows warm within the
budget. FullMix selection can cancel obsolete pending demand; it does not delete
the disk set or forcibly evict live readers. Budget pressure retires only idle
unneeded views through exact ownership, exposing loss of ready state; active/
queued/history readers remain pinned. Explicit Delete/Unload intentionally revoke
their track-bound intent; passive validation/load failure does not erase saved
ALL STEMS. Preserve the completed ready-trigger reuse/pending coalescing/click-edge
fix for UI/MIDI/controller input paths and original input timestamps.

## Startup policy, all 216 slots and resource accounting

Use the existing Settings surface and `ProjectState` persistence, no machine or
credential configuration. `preload_stem_loops_on_startup` defaults false for old
and new projects. Turning it on deliberately requests all eligible existing stem
loop ranges at startup and after eligible generation; turning it off stops policy
demand, while explicit demand and pinned readers remain valid. It does not download
models or generate missing stems. With it off startup prepares FullMix only,
even if saved ALL STEMS intent or five WAVs exist. State this exception visibly.

Keep 2 workers, 32 queued/reserved jobs, 8 startup admissions per poll and 1-GiB
transient PCM per job. The existing 512-MiB `constant_timing::PcmBudget` bounds
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
