# Shared generation, disk selection and independently effective playback

## Status and delivery ownership

L1 is delivered; L2 separates offline custody and durable disk selection from
effective playback. L3/L4 and Bulk remain open. `X11-LIVE-STEMS` and `X11-BULK` are
additive delivery IDs in the maintained pre-Rust program. The old IDs/edges,
completed subtasks, failed results and final Human gates retain their meanings.
The central program defines topological order; these local stages are bounded
implementation units, not additional independently accepted program IDs.

## Published seams to reuse

- `controller/stem_job.py` holds immutable source/private-generation leases,
  independent `StemSubscriber` identities and final-interest cancellation.
  Matching uses verified canonical content and the captured effective fingerprint;
  independent source/intent authority remains with each subscriber.
- `controller/stem_workers.py` enforces two workers and 32 queued jobs. Add fair
  incremental batch feeding at this admission seam, not 216 submitted workers
  or a second executor. A bounded 216-entry inventory contains intent metadata,
  not retained complete PCM or a second 216-job executor queue.
- L2 permits offline generation and complete disk-only preparation while active.
  `mixer.rs::prepared_stem_rejection` still rejects new active component sets as
  `PadPlaying`; that adoption guard remains until the independent L3 proof exists.
- `ProjectAssetLifecycle` reserves retirement before intent mutation and
  acquires new assignment owners before releasing previous owners. Native
  shared readers, paired complete descriptors and off-thread retirement already
  cover queued/bank/voice/history readers. Extend these owners without copying
  historical source/timing/native ACKs between subscribers.
- P3 complete WAV/PCM pairs, P4b's accepted NormalLoop vertical/lifecycle and the
  current selected Some/Some transitions are foundations. P5a's bounded selected
  same-set None-to-Some/effective-mode/warm-return proof is delivered; whole P5a
  and its final musical/resource/Human gates remain separately open.

Source-backed regressions include `test_shared_stem_material.py`,
`test_asset_lifecycle.py`, `test_stem_workers.py`, `test_stems.py`,
`test_stem_separators.py` and the native `stem_publication_tests` families.
Existing tests that reject playing generation must change only alongside a
productive new-path regression. Mocked inference is ownership evidence, not
actual separator quality/memory or Human hearing acceptance.

## L1: captured offline job and durable fingerprint

One `effective-stem-v3` identity binds verified full original SHA-256/byte length,
full output rate/channel layout/frame extent, transform/alignment revision, separator/model,
only the selected separator's output-affecting quality options and actual full
sealed producer lineage. The lineage captures complete checkpoint/configuration
digests and the implementation/runtime inputs that determine output, rather than
requested labels or filename/mtime/size hints. Full file verification streams
off realtime; paths only locate inputs and do not contribute to equality.
Execution/device diagnostics remain separate unless they change defined output.
Unverifiable actual producer inputs cannot be attested by a synthetic fallback.

Admission retains the immutable verified original source lease and independent
subscriber authority. The first existing bounded nonRT worker phase resolves and
seals actual producer inputs, then emits captured identity to the control owner.
Control admission uses Native's existing loaded-original digest snapshot without
file I/O; pending load/adoption or absent current source ownership cannot provide
one. The first worker verifies full original bytes against that identity and
rechecks them after inference. Digest metadata never replaces a fresh own native
source ticket or later full pair verification/publication authority.
Control revalidates current Settings/material intent and decides one physical
job, joining equal subscribers or reporting `Stems already present` before any
inference. Workers do not own a second job/selection registry. Each admitted
inference uses its captured sealed inputs; mutable model paths cannot change its
producer midway. Different fingerprints serialize replacement per material.

The common descriptor, PCM binding and current selection carry the same optional
versioned provenance record. Present provenance participates in effective pair
compatibility, so equal WAV hashes with different known producer lineage are not
the same generation. Absent provenance preserves the exact legacy artifact
identity and model-free verified availability as UNKNOWN. Reopen does not load
or download a model, manufacture producer attestation or relabel UNKNOWN as the
currently selected model. The field is optional for legacy compatibility, not
permission to omit provenance from newly attested generations.

Verified equal original bytes under different contained roots may coalesce or
reuse compatible artifacts. Copy/rebind obtains each subscriber's own current
native source ticket/version; a sealed artifact marker attests that artifact
generation only. It does not become another subscriber's assignment, source,
timing/history or publication authority. Each interest retains its own content
instance, request, source ticket/version and Settings/material revisions.

Validated stem-generation Settings changes advance a monotone generation-intent
revision; changed material request intent advances a separate per-material revision.
Control checks both fences on every worker event and immediately before current
selection mutation. A-to-B-to-A retains the new revision even when fingerprints
compare equal, so old results never regain authority, consume newer jobs or
alter newer content. Model selection changes future intent without rewriting a
running job's captured producer. Old usable owners remain intact on rejection.

UI readiness uses validated metadata/current state snapshots from the same
authoritative decision as controller/direct and future batch admission. It does
not hash long originals/model files or load a model each render. A complete
current matching set yields the diagnostic `Stems already present`, with no
inference or output artifact writes. Changed selected fingerprint enables
generation; a valid completed current replacement disables it again. This keeps
one current discoverable model without a user-facing parallel-model library.

Gate L1 proves physical inference job/subscriber counts; full producer sealing
and fingerprint save/reopen; relevant versus irrelevant quality; same/different
actual model/config/producer with equal output bytes; cross-root copy/rebind own
authority; UNKNOWN model-free legacy restore; duplicate direct/UI/future-batch
admission; Settings and material A-to-B-to-A at events and immediately before
selection; stale/equal-byte content reload; and rejection with old owners intact.
These contracts alone report no measured result. L1 retains all playing/offline
generation and native active new-set guards: L2 proves playback-independent
generation and L3 independently proves new-generation adoption before opening
their respective guards. L1 is not active-generation or full LIVE acceptance.

## L2: offline generation and joint disk commit during playback

Offline generation and bounded complete preparation depend on source ownership,
not whether a pad is playing. No live source ticket is held as an irrevocable
timing authority across long inference. The job keeps its original sealed source
lease; after inference it verifies complete five WAVs/five PCM derivatives/common
descriptor and current material/model intent before atomic disk selection.
Recheck each subscriber's content/request at its own later native capture.
Unload, replacement or Cancel detach only that obsolete interest; surviving
subscribers can finish. No surviving subscriber means cooperative own-process
abort followed by actual read-end retirement, never deleting under inference.

`OfflineStemSourceLease` clones the own pad's existing `CommittedColdLease` under
loaded source/request ownership checks, including fully resident PCM. Its exact
own managed-original reference and Native source generation are checked from
held metadata; digest/PCM equality cannot select another root. The opaque lease
exposes immutable source/shape metadata and a worker-only full verification
operation. It carries no accepted timing, window or publication permission.
Physical job custody survives interest detachment until actual backend/process
read end. Subscriber completion captures a fresh ordinary `PreparedSourceTicket`
after checking current source/content/material intent; restoration remains its
existing model-free independent path.

The commit linearization point selects exactly one verified current material
set through the existing fenced config writer's actual atomic replacement,
after both areas and their common marker are durable and verified. Fresh own
source/request and current material/model intent are checked after the real
temporary-file fsync immediately before replacement. Independent new/predecessor
assignment pins and a bounded settlement record/reservation are already held.
Faults
or Cancel before this point leave the previous selected set usable. If Cancel
races after commit, report the already committed result and cancel only pending
batch interests; do not pretend the committed selection was rolled back.
Private/incomplete/retained predecessor generations are not selectable parallel
model libraries. Old files remain immutable and hidden from current-model
selection until actual last readers permit their removal.

After replacement, owner transfer, selection settlement and retirement retry
through the retained record; failure cannot discard the committed candidate or
pretend to roll back the config. If newer content or unload supersedes the pad in
the replacement/settlement gap, it wins in memory. The same persistence writer
must durably flush that newer snapshot before retiring the now-stale disk-selected
pair. A failed newer flush retains both exact pins and reserved settlement capacity.

The physical backend runs through the existing owned Windows job/tree and bounded
stdout/stderr readers, without consuming analysis's single execution slot or adding
an executor. Final-interest cancellation asks only the exact own separator module
to stop via a fresh private marker. Named decode/model/recursive inference/output/
hash checkpoints can exit cooperatively after cleanup; a blocking native model
call uses finite owned-tree escalation. Custody ends only after actual process
and output-reader end. Parent alignment/hash checkpoints use the same sticky event.
All newly executed process/cancellation helpers participate in the full sealed
producer inventory and fail-closed loaded-code identity, with a bounded 64-file cap.

Gate L2 injects faults at WAV/PCM/descriptor/config commit boundaries, races
Cancel/commit/unload/reload/model changes, verifies reopen and old-set usability,
and renders current FullMix and selected-stem KEYLOCK output during an actual
blocked worker. Source frame, timing, mask, Native/FIFO/filter state and unrelated
users remain continuous. X11-LONG proves conversion scratch separately; neither
a bounded pool nor disk chunks prove total Demucs inference memory bounded.

## L3: new-generation live preparation and own ACK

Disk selection is not effective playback. Each still-current pad prepares its
required FullMix/component ranges from the sealed new pair through existing
bounded lanes. Fresh own source/request/timing/window permits, actual live voice
identity and prospective DSP/history coverage govern enqueue and adoption.
Current bank B cannot authorize old voice A. Old-source voices keep their frozen
source/set/timing readers until their real retirement; never relabel them using
a new assignment. P5a same-set residency permissions are insufficient for this
new complete-set transaction.

Only a separately proved new-set transaction may accept during playback. It
must preserve physical loop endpoints, source fraction, rate/ramp, masks and
existing 128-source-frame transition policy, with actual wet Native/FIFO/filter
continuation. Reject unsupported source/history coverage safely and display
disk-ready/pending/error separately from effective stems. Old audio stays usable
through queue exhaustion, stale ACK, pause/stop, replacement and cancellation.
No stop/retrigger, position jump, callback preparation or wholesale guard removal
is an implementation shortcut. Each pad becomes effective only on its own ACK;
another subscriber's acceptance never grants authority.

Gate L3 runs productive controller -> preparation -> command drain -> own ACK ->
nontrivial rendered output for stopped/playing/paused NormalLoop, FullMix/selected
stems/empty and changing masks, KEYLOCK OFF/ON at nonunity rate/BPM, current and
old voice, irregular partitions and repeated replacements. Verify post-adoption
wet continuation through later real preparations, stale/queue/cancel rejection,
exact voice/source/cursor/loop continuity and independent owner retirement. Add
seek/Intro/Tail/full-track-domain cases as separately bounded coverage before
enabling them; unsupported domains remain visibly guarded. This stage cannot
declare whole P4b/P5a accepted.

### L3 candidate authority and exact reader seam

New-set admission is separate from existing same-set registration/control. A
candidate epoch and captured predecessor intent/selection fences keep V1 current
through worker preparation. Actual callback capture includes the installed
selection and set owner, not only control's accepted registration. Candidate
adoption rechecks those fences and the real voice/history/window, then publishes
V2's own selection. Candidate epochs must never masquerade as ordinary resident
control tokens: either use captured launch/intent fences without that pending
flag or allocate unique control ownership separately from effective intent.

`SourceReadPlan` stays copied scalar data. Set-aware selection binds a comparison
token to an actually held immutable complete-set owner. A borrowed fixed
`StemReadSets { current, outgoing }` resolves source and exact owner through one
read/tap/coverage implementation. The mixer holds outgoing V1 through 128 consumed
source frames; frozen views and Native requests hold the owners their actual
plans read. All replacements and cleanup use reserved retirement capacity.
V3 and interrupting controls cannot overwrite an occupied outgoing slot. Existing
KEYLOCK preparation deferral during a ramp remains; later preparation captures
the settled actual V2, without rewriting old history metadata.

Controller state is bounded per pad: exact disk candidate, retained effective/
control selection, one pending own attempt and latest desired intent. Exact disk
currentness governs candidate admission; separate source/content/Native authority
governs retained V1. Claimed attempts stay observable when newer disk intent
arrives. Component acceptance is not wet effectiveness; stopped waiting and
paused transitioning remain explicit. Final UI and every final-reader family
remain L4 proof obligations, rather than being inferred from this L3 mechanism.

### L3 Intro/Tail candidate adoption

The bounded extension covers first FullMix-to-V2 and selected-V1-to-V2 candidates
on one current running or paused Intro/Tail voice with KEY LOCK OFF or ON,
including callback-acknowledged accepted musical timing. The own explicit Seek
must already have completed. ON requires the actual KeyLockFiniteLoop physical
prefix-and-loop `[0,E)` or tail-and-loop `[S,N)` union; OFF requires physically
complete FullTrack `[0,N)`. Both exact predecessor/candidate reader generations,
the accepted projection and source/window/launch/history/capacity fences remain
authoritative at Capture and Adopt. Ordinary same-set mode requests, frozen
old-source voices, multiple same-pad voices and missing unions remain guarded.

Accepted musical timing preserves the physical one-shot prefix or suffix and
then returns naturally to the accepted musical loop period P, which can differ
from physical extent H. Productive tests must independently verify both physical
boundary taps, fractional return and later virtual P/H seams through the actual
128-consumed-source-frame handover, raw Native/FIFO and uninterrupted filter
chronology. An earlier receipt cannot confirm V2. ON needs V2's own later exact
4096-output-frame source preparation and productive Wet while still in its
explicit domain; OFF establishes distinct consumed-source dry effectiveness.

Paused Capture/preparation/Adopt retain source fraction, generation, Native/FIFO,
productive/filter history, unconsumed ramp and both immutable owners. Silent
callback time and controller polling do not advance the source clock or report
new effectiveness. Only own Resume and subsequent source consumption establish
V2 effectiveness; another pad keeps its independently consumed trajectory.
Save/reopen verifies durable V2 and musical/loop/KEY LOCK intent separately.

The earlier paused-Intro and accepted-musical Intro/Tail negative expectations
are superseded only within this candidate extension contract. Its positive
public/native software profiles, integrated checks and one independent review
are recorded in `docs/prepared-stem-publication.md`: twenty added productive
histories, sixteen stale-own-timing Capture/Adopt histories and preserved
ordinary same-set/withdrawal guards. This is software proof of the bounded
candidate contract, not general musical quality or Human hearing acceptance.
Whole task3.3 remains open: explicit in-loop/equal-phase Seek followed by new-set
adoption, actual KEYLOCK ON KeyLockFullTrack explicit Seek/ALL exceptions and
unproved complete-resident partial-loop contexts need their own bounded proofs.
Intro/Tail-specific final-reader/lifecycle coverage also remains separate from
the earlier finite NormalLoop L4 closure. Whole P4b/P5a/P5b, full L4, resources,
Human acceptance and the stop before full Rust-port/Slice8 remain unchanged.

### L3 own Seek and complete-resident partial loops: bounded software closure

The delivered grouped product block adds changed-position and equal-phase public
Normal Seek followed by fresh first/replacement adoption, proper partial
NormalLoop on actual complete OFF/ON residency, and ON complete-resident
Intro/Tail candidate exceptions. Its executed matrix is 32 Seek histories
(changed/equal, OFF/ON, physical/musical, running/paused, first/replacement),
24 complete-partial Normal histories (OFF/ON, physical/musical,
stopped/running/paused, first/replacement) and 16 ON complete-partial Intro/Tail
histories (Intro/Tail, physical/musical, running/paused, first/replacement).

An equal-phase case obtains two own acknowledged Seeks to the same integer T
without intervening source consumption, checking exact fraction `0.0` and the second
intentional Seek cut. A covered Seek can receive its actual preserved-window
ACK directly; no nonexistent Relocate is invented. Fresh V2 adoption preserves
the resulting source/voice/Native/FIFO/filter chronology without another cut.

Complete cases first obtain a real complete-source ACK, then declare a proper
partial physical loop `[S,E)` and use the actual mode-only ON ACK where needed.
Residency `[0,N)`, H=`E-S` and accepted musical P remain separate. Hand-assigned
context tags or a loop rewritten as `[0,N)` cannot establish this proof. The
existing full-source ALL trace remains separately scoped; these cases claim no
unexercised public Play or initial default-loop admission. All fresh permits,
exact readers, consumed-source effect and pause-frozen owners remain required.
Stopped cases prove complete Capture/Adopt and readiness before the existing
real public Play supplies markers, which may narrow ON residency to finite.
That Play does not prove complete residency persists; an obsolete readiness/
effect permit is not borrowed. A fresh own ALL receipt proves V2 output after Play.

Additional negatives prove 16 stale-pre-Seek Capture/Adopt histories
(Capture/Adopt, changed/equal, OFF/ON, physical/musical) and four complete-ON
ordinary same-set Intro/Tail histories (Intro/Tail, running/paused).

The three positive tests pass all 72 histories; Native negatives pass 16+4.
The integrated residency profile passes 106 tests, separate source coverage nine,
and fresh build-only Release/private preflight plus six Python files/183 tests
pass. Equivalent conditional cleanup only changes test style; successful audio
checks are reused and final fmt/Clippy test compilation passes with no changed-file
diagnostic. Exact profiles, failed fixture attempts and limits are recorded in
`docs/prepared-stem-publication.md`.

Together with delivered Intro/Tail and full-source-loop proofs, this closes
task3.3's bounded candidate domain topology, not a full Cartesian musical/resource
or reader-retirement matrix. Ordinary same-set Intro/Tail, frozen-source and
multiple-voice guards remain. Full-source public Play/default initial admission,
unchanged-set ALL, domain-specific final-reader/full L4, P4b/P5a/P5b, resources and
final/Human gates remain open; no full Rust-port/Slice8 work follows.

## L4: single-current-set retirement and duplicate UI

The shared material has one durable current selected set. After replacement,
new discovery and each requesting assignment resolve that set after its own transaction. Non-requesting independent copy C retains its separately selected V1 through origin deletion/reopen until explicit replacement/deletion or final release; runtime/history readers also retain predecessors until actual final use. One discoverable current model is not automatic reassignment or a parallel selectable model library. A failed native
adoption keeps the previous effective audio and rollback owners usable and
reports pending/error; it does not make partial new components effective. Durable
current selection and effective runtime selection are explicit distinct states.
Retirement waits for every all-bank assignment, job, subscriber, action/version,
queued publication, unload ACK, bank/voice, Native/FIFO/history and retained
complete reader. Existing bounded retirement admission fails before mutation;
sharing errors retry off-thread. Unknown, external and newer files survive.

Generate is disabled only for a valid current set matching selected effective
model/configuration/source/shape/transform. Selecting a different model enables
it; successful complete replacement disables it again. Pending work displays
progress and cannot start a duplicate. The controller remains authoritative for
direct and batch requests regardless of UI state.

Gate L4 holds each distinct actual reader across replacement and verifies no
leased overwrite/deletion, then releases readers one by one and proves removal
only after the true final reader. Save/reopen, failed-native retry, shutdown and
unknown-file tests retain source and current set. Human final tests verify UI,
audible continuity and selected-model behavior; software gates cannot close them.

The bounded domain-specific lifetime proof composes the existing public
candidate domain with an orthogonal replacement or queued-Unload lifecycle.
It uses eight ON replacement contexts (finite Intro/Tail, changed/equal finite
Normal Seek, complete-source proper-partial Normal/Intro/Tail and complete-source
Normal), plus two queued-Unload representatives in finite musical Tail and
complete-source musical Intro. Public Pause/Resume occurs during the occupied
replacement ramp. Real callback retirement, retained Native/history, later own
complete Native preparation, identity/component Weak expiration and physical
asset-worker cleanup establish final release; snapshot ownership counts alone
are insufficient. Final V2 public Unload follows the live domain/output checks.
Existing finite owner-family custody/preservation results can be reused only
where their inputs and ownership topology are unchanged. All ten histories and
the fresh108-case residency/13-case retirement profiles pass; fmt/Clippy pass
with unchanged warnings and no changed-file diagnostics. The unchanged production
1827227 Release/private import/preflight and183 Python results are reused.
Exact profiles and the repaired pre-Unload Legacy(None) reference omission are
documented in `docs/prepared-stem-publication.md`. This extends bounded task4.2/4.3
candidate-domain software-lifecycle coverage, without a full Cartesian, full-L4
or Human acceptance claim.

Retained frozen A after actual native bank-B restore has a separate
FrozenStemView current/outgoing owner route. Its productive final-reader cleanup
is outside this candidate-domain gate; the existing negative/admission proof
cannot replace that lifecycle proof. Native restore with `replace_assignment=false`
also does not establish public Loader assignment replacement. Stable OFF custody
reuse does not establish an active ON-to-OFF control/history transition.

## B1: all-loaded generation and cooperative Cancel

Confirmation captures bounded all-bank slot/content/material identities, selected
effective fingerprint and affected counts. Cancel before admission changes
nothing. Feed distinct required material jobs incrementally and fairly through
the two/32 pool; existing direct jobs and later interactive requests retain fair
admission. Skip unloaded or changed captured identities with a visible stale
result rather than targeting newly inserted content. Successful earlier items
remain successful if later items fail or Cancel is requested.

Batch interest identity must be distinct from pad/direct interest identity even
when both refer to the same pad/job. Cancel detaches only the batch's subscribers
and pending inventory. Final-interest Cancel asks only that job's owned process
to stop cooperatively, with bounded escalation/teardown; no foreign process or
remaining subscriber is cancelled. Existing `subprocess.run` plus a boolean
cannot prove prompt running-inference cancellation: use an owned cancellable
process handle and retain leases until actual exit/read end.

Gate B1 loads slot labels 1 and 216 plus all intermediate bank inventory, covers
216 duplicate and 216 distinct materials and an actual supported RAM-budget
case without claiming 216 simultaneous voices. It checks physical worker/queue
peaks (2/32), deterministic fair ordering/no starvation, pending/running/done/
skipped/failed/cancelled counts, bounded intent memory, mixed direct subscribers,
Cancel in queued/inference/commit/adoption phases and fresh partial results.

## B2: all-loaded deletion

Confirmation snapshots all loaded assignments and exact current/retained set
versions; shared material is counted once and affected pad labels are visible.
Accept revokes only still-matching targeted stem selection and pending batch
interests, then safely selects FullMix through the established native path.
Old active selections/transitions/readers may finish their bounded handover; no
file is removed until true final ownership release. Changed/new source or set
versions are skipped, not recursively removed. Original FullMix/source/PCM,
unknown content, external files and other owners survive. Repeated delete is a
truthful no-op. Cleanup/deferred errors remain visible without stopping playback.

Gate B2 proves no-op Cancel, all-bank shared/different materials, concurrent new
generation, stale dialog, active/paused old voices, Native/FIFO/history owners,
unknown/external/reparse/collision paths and forced sharing errors. Rendered
FullMix transition preserves transport and non-target users; delayed physical
cleanup completes only after actual final readers. Final Human all-loaded UI
and performance checks stay open.

## Dependencies and final integration

X11-LIVE-STEMS uses delivered source/job/paired integrity foundations, the
accepted finite NormalLoop/lifecycle proofs, X11-LONG streaming and X11-MATERIAL
identity/path transactions and genuine P5a selected-same-set first residency/retention/effective-mode/ordinary warm-return acceptance. P5a is the central DAG prerequisite; it is not proof of generation replacement. Its own L1-L4 gates additionally prove active new generation; no whole P4b/P5a acceptance may be inferred from the LIVE result.
X11-BULK depends on complete X11-LIVE-STEMS and material identity/retirement,
then B1-B2. P6/R1 and final V0/H-LIVE/H-FINAL/C-FINAL integrate these behaviors
in the central additive DAG; avoid an edge that makes LIVE depend on a whole
parent that already depends on LIVE. No full Rust port or Slice8 follows.

Further stages require the normal impact-based Rust/Python/static/strict
validation, independent nonauthor source/test review and genuine output/resource
receipts. X11-PLAN checks specification structure, mapping and dependency
coherence plus official strict validation only; it supplies no implementation
acceptance. Bounded L1/L2 delivery evidence is maintained in
`docs/prepared-stem-publication.md`; L3/L4/B1/B2 and final parent/Human gates remain open.
