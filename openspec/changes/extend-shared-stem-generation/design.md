# Shared generation, disk selection and independently effective playback

## Status and delivery ownership

This is PLAN_ONLY for E11-05/06/07/19/20. `X11-LIVE-STEMS` and `X11-BULK` are
additive delivery IDs in the maintained pre-Rust program. The old IDs/edges,
completed subtasks, failed results and final Human gates retain their meanings.
The central program defines topological order; these local stages are bounded
implementation units, not additional independently accepted program IDs.

## Published seams to reuse

- `controller/stem_job.py` holds immutable source/private-generation leases,
  independent `StemSubscriber` identities and final-interest cancellation.
  `matches()` currently compares path, version, shape and separator options;
  extend this to verified canonical content plus durable effective fingerprint.
- `controller/stem_workers.py` enforces two workers and 32 queued jobs. Add fair
  incremental batch feeding at this admission seam, not 216 submitted workers
  or a second executor. A bounded 216-entry inventory contains intent metadata,
  not retained complete PCM or a second 216-job executor queue.
- `controller/stems.py` rejects active pads at admission and completion;
  `mixer.rs::prepared_stem_rejection` rejects `PadPlaying`. Those delivered
  guards remain until the new independent admission/adoption proof exists.
- `ProjectAssetLifecycle` reserves retirement before intent mutation and
  acquires new assignment owners before releasing previous owners. Native
  shared readers, paired complete descriptors and off-thread retirement already
  cover queued/bank/voice/history readers. Extend these owners without copying
  historical source/timing/native ACKs between subscribers.
- P3 complete WAV/PCM pairs, P4b's accepted NormalLoop vertical/lifecycle and the
  current selected Some/Some transitions are foundations. P5a's selected same
  set None-to-Some/effective-mode/warm-return work remains separately open.

Source-backed regressions include `test_shared_stem_material.py`,
`test_asset_lifecycle.py`, `test_stem_workers.py`, `test_stems.py`,
`test_stem_separators.py` and the native `stem_publication_tests` families.
Existing tests that reject playing generation must change only alongside a
productive new-path regression. Mocked inference is ownership evidence, not
actual separator quality/memory or Human hearing acceptance.

## L1: captured offline job and durable fingerprint

An admitted job retains a verified immutable source lease, content digest,
output rate/layout/full extent, transform/alignment revision, separator ID,
verified model checkpoint/config digests and all output-affecting quality
options. Persist the versioned effective fingerprint in the common pair
descriptor and current material selection. Execution/device diagnostics are
recorded separately; device choices that change defined output belong in the
effective fingerprint. No path/mtime/filename/size alone proves equal material.

UI readiness uses validated current metadata and state snapshots; it does not
hash long originals or load model weights each render. Controller/direct/batch
admission verifies authoritative identity outside realtime processing. Legacy
unknown fingerprint is not silently stamped as the selected model: retain its
usable verified artifacts and offer a bounded verified upgrade/regeneration.
Model selection changes future intent, never a running job's captured model.

Job deduplication uses the verified material/fingerprint/shape/transform tuple.
Concurrent identical requests join one physical job with independent interests;
a complete current valid selected fingerprint produces an explicit already
present no-op. Different fingerprints serialize replacement for that material;
newer selection wins before commit rather than allowing old late completions
to restore retired intent. This preserves existing global model choices without
creating a user-facing multi-model set library.

Gate L1 measures physical job count, subscriber count, complete fingerprint
save/reopen, same/different model and quality cases, stale/equal-byte reload,
duplicate direct/UI/batch requests and failed admission with old owners intact.

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

The commit linearization point selects exactly one verified current material
set after both areas and their common marker are durable and verified. Faults
or Cancel before this point leave the previous selected set usable. If Cancel
races after commit, report the already committed result and cancel only pending
batch interests; do not pretend the committed selection was rolled back.
Private/incomplete/retained predecessor generations are not selectable parallel
model libraries. Old files remain immutable and hidden from current-model
selection until actual last readers permit their removal.

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

Future code changes require the normal impact-based Rust/Python/static/strict
validation, independent nonauthor source/test review and genuine output/resource
receipts. X11-PLAN only checks specification structure, mapping and dependency
coherence plus official strict validation; none of L1-L4/B1-B2 is passed here.
