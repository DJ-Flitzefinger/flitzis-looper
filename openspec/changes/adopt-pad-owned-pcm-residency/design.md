# Design: pad-owned assets and proved resident PCM

Status: P0 target design only, based on `b4b264be30a67379c6e3cc81b39c1c52d68551d9`
(2026-10-09). No feature implementation or performance acceptance is claimed.
Implementation remains the existing Python/UI + Rust audio hybrid.

## Verified starting point

Current symbols, not old line labels, establish these constraints:

| Area | Current implementation | Required change |
| --- | --- | --- |
| Originals/PCM | `cold_store.rs`, `sample_loader/cold.rs` and `warm.rs`: copy-first sealed originals; complete versioned decoder/playback PCM under `samples/.pcm-cache/v1`; warm initial range read already exists | Move physical ownership to each pad; keep stable capture and full integrity rules |
| Stem disk ownership | `controller/stem_cache.py`: `STEM_CACHE_ROOT = samples/stems`, `.ready-<uuid>` and five-file marker | Pad-owned `stems`; short visible container, immutable generations |
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

For zero-based native/UI slot `id`, the visible owner is `samples/#(id+1)`:

```text
samples/
  flitzis_looper.config.json
  #1/
    Original track.wav
    .pcm-cache/v1/.ready-<uuid>/
      decoder.f32le
      playback.f32le
      manifest.json
    .pcm-cache/stems/v1/.ready-<uuid>/
      vocals.f32le  melody.f32le  bass.f32le  drums.f32le  instrumental.f32le
      manifest.json
    stems/.ready-<uuid>/
      vocals.wav  melody.wav  bass.wav  drums.wav  instrumental.wav
      .complete.json
  ... #216/
```

The tree illustrates namespace and ownership, not a claim that implementation
already uses these paths. Original filename, extension and bytes preserve actual
encoding. Do not rename every source to MP3. Native slot mapping is exactly 0..215;
reject #0, #217, traversal, absolute out-of-root references, symlinks/reparse points
and changed checked ancestors. Project metadata remains at its current root path.

Use one authoritative pad-path resolver shared by import, restore, generation,
migration and cleanup. Reuse the existing stable capture, atomic generation and
asset lifecycle mechanisms. Internal manifests retain content/source/decoder/
transform/rate/schema/generation identities; `.ready-<uuid>` prevents live leased
files being overwritten. Hashes need not be visible long directory names. A same
basename replacement cannot overwrite a leased original: use an owned short
collision suffix and retain original-name metadata, or defer the canonical name
until the prior owner retires. Never silently replace its encoding/content.

Physical pad ownership includes duplicate content in different pad folders.
Verified in-process immutable PCM backing may still be shared by compatible
content/transform/range. Each assignment, original path and cleanup lease remains
independent. No final global cache, cosmetic wrapper, hardlink, symlink or reparse
alias substitutes for this layout. Measure physical disk duplication separately
from shared resident allocations. Unknown files and other project owners survive.

## Transactional migration and rollback

Migration is control/background work and never a new automatic analysis request.
P1 installs safe old/new readers and new-write destinations. P2 migrates existing
assignments using a versioned per-project journal whose entries name actual paths,
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
   must be no permanent global-cache dependency at final acceptance.

Test multiple pads/projects sharing a source, same/different basenames, duplicate
content, corrupt/partial/missing files, cancelled/stale jobs, unload/same-pad reuse,
live sealed readers, settings edits and restart at each journal phase. Restoration
and rollback use validated bytes plus fresh native ownership, not historical ACKs.
Migration must expose unexpected current data loss; autosave/GC are observations,
not inferred causes. Private models/audio/evidence are never migration targets.

## Persistent aligned stem PCM

Keep five WAV outputs and complete-set integrity. Produce five complete aligned
f32le playback artifacts off-thread, using the existing shared alignment algorithm
and source zero. Bind every WAV digest/length, complete source and playback identity,
loaded rate/layout/full frames, resampler/channel policy, single signed alignment
offset, alignment algorithm/version, PCM hashes/dimensions and immutable StemSet
identity in the descriptor. No independently shifted component or guessed alignment.

All additional stem PCM lives in the pad's `.pcm-cache`, not its WAV `stems` area.
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

[Tasks](tasks.md) owns the serial P0/P1/P2/P3/P4a/P4b/P5a/P5b/P6 program. Each
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
