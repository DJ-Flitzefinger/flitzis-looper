# Private B2 seal-bound scoring workflow

`python -m flitzis_looper.analysis.beat_scoring_cli` connects strict
[reference receipts](beat-this-reference-inputs.md), approved retained native
diagnostics and [pure temporal metrics](beat-this-temporal-scoring.md). It reads
private workspace files and creates a new private report exclusively. It runs
no inference and loads no native engine. Every metric retains
`unchecked_by_metric_core`; musical acceptance stays `pending`, default adoption
`blocked`, count/bar identity and paired correction burden `pending`. A content
receipt does not establish human honesty or musical completeness.

## Explicit original-source aliases

The frozen manifest remains unchanged. A caller-selected private JSON file can
identify renamed T01/T02 sources. It contains `schema_version: 1`, the original
`manifest_sha256` and an `aliases` array. Each entry has `track_id`,
`original_source_relative` spelled exactly as in the original manifest, and
`actual_source_path` (currently `test-audio/1.mp3` and `test-audio/2.wav`).

The validator allows only T01/T02, rejects duplicates, outside/repository paths,
wrong historical origins and redundant aliases, and checks actual size/SHA-256
against the original frozen track on each use. No filename guessing or decode
equivalence is used. The scoring report binds the exact alias-file digest and
retains both paths. Reference receipt/order/correction commands also accept
`--source-aliases`; original manifest, bundle and receipt identities remain.

## Reference-first commands

Run from the repository root with fresh output filenames:

```powershell
$workspace = "D:\NEUES\Windows Dokumente\Dokumente\CODING-PROJEKTE\flitzis-looper"
uv run --no-sync python -m flitzis_looper.analysis.beat_scoring_cli inventory --workspace "$workspace" --identities "scratch/b2-reference/loaded-identities.json" --source-aliases "scratch/b2-scoring/source-aliases.json" --output "scratch/b2-scoring/inventory.json"
uv run --no-sync python -m flitzis_looper.analysis.beat_scoring_cli draft --workspace "$workspace" --reference-seal "scratch/b2-reference/reference-seal-v1.json" --source-aliases "scratch/b2-scoring/source-aliases.json" --output "scratch/b2-scoring/plan.json"
uv run --no-sync python -m flitzis_looper.analysis.beat_scoring_cli score --workspace "$workspace" --reference-seal "scratch/b2-reference/reference-seal-v1.json" --input "scratch/b2-scoring/plan.json" --source-aliases "scratch/b2-scoring/source-aliases.json" --output "scratch/b2-scoring/timing.json"
```

`inventory` validates protocol/inventory bytes and actual full finite listening
PCM and original sources. It opens no predictions, creates no seal and lists
all five independent sealed references as unsupplied. `draft` requires a valid
complete ReferenceSeal before preparing supported selections. `score` first
revalidates seal/bundle bytes, inventory, every source and full loaded PCM,
independence declarations, labels/split and recomputed coverage. Only then does
it open the plan or candidates. Missing seal/material or plan produces an
explicit blocked missing-input report with no scores; invalid bytes or coverage
reject. Neither condition reads candidates before reference validation.

The strict plan has schema version 1, status `ready_for_temporal_scoring`, exact
`reference_seal_sha256` and `candidates` entries with `track_id` and `profile_id`.
Duplicates, unknown profiles, wrong profile/track pairs, extra/duplicate JSON
fields and a changed seal digest reject. Omitted candidates and absent selected
artifacts are explicit missing inputs. A partial report is marked
`incomplete_temporal_diagnostics`; it cannot establish complete B2 acceptance.

## Supported complete native lineage

The reader supports five explicitly approved historical import profiles. Their
summary identities were bound by the independently checked frozen listening
inventory; the reader additionally pins every retained lineage artifact. It
checks original source, loaded rate/frames/origin/hash, selected
model/frontend/environment, native request/source/generation, complete raw
worker arrays, component and final envelope. T01-T03 reconstruct the retired
request only from approved byte-verified summary/export metadata. T04/T05 also
retain actual request, export, native finish/publication and completion bytes.
The original temporary export retired; actual independently materialized
listening PCM must match its complete identity. This records historical lineage
and does not claim a new request or export.

Within the historical profiles, complete bit-exact publication lineage passes
only for T04/T05. T01-T03 raw/component/snapshot arrays agree, but their original
version-1 completion events changed some binary64 logits during the old native
JSON roundtrip. Those historical profiles still reject with a raw-parity mismatch.
Their snapshots and failed events remain unchanged; the separate fresh profiles
below provide the corrected engineering lineage. A supported selection does not
certify musical labels. Selecting only T04/T05 can yield partial diagnostics
after genuine full reference sealing; it cannot close the five-track B2 gate.

### Fresh fixed native v2 profiles

Fresh T01-T03 profiles have a separate `fresh_native_v2` lineage. The producer is
an explicitly ignored native test, `b2_fresh_native_candidate_probe`, with a private
`FLITZIS_B2_NATIVE_CONFIG`. It uses real copy-first cold loading and finite-bank
command ACK, scans the complete immutable native source through its production
reader, then runs the existing service, configured CPU worker and actual native
finish/completion. It opens no stream, app, GUI, audio device or recorder.

The fixed approval registry pins complete producer packets independently of
candidate-supplied hashes. Imports rehash original and sealed copied source,
complete finite interleaved PCM, full native mono export and listening PCM,
cold manifest/transform, config, executing native test binary, producer copies,
model, installed worker/interpreter, lock and setup identities. Five native
complete-source metadata fields remain present at every observed stage. The
finite resident window is separate from full analysis extent and does not crop
inference. Original paths, explicit aliases, full arrays and all rejected
historical evidence remain intact.

Only successful native finish, one outer-and-inner identity-matched ready-v2
completion and exact full raw/component/finish/snapshot/event binary64 parity
pass. Retirement and a subsequent request preserve the same source generation;
request advancement never relabels that source. The request keeps its retired
temporary export path; the retained export is a separate binding. The embedded
probe executes the native test executable whose complete bytes are separately
retained and bound. Its separately copied installed PYD is reported with
`native_extension_used_by_candidate: false`; application suites test their actual
installed extension separately.

All six actual T01-T03 probes passed: Debug under `probes/debug-v2/` and Release
under `probes/release/`. The seven retained representations preserve all four
complete prediction arrays bit-exactly, and actual KeyNet outcomes are ready.
The first Debug T01 attempt under `probes/debug/` completed native source/PCM
verification but failed the embedded Python import. Its original producer,
runtime, source proof and failure log remain retained. The revised producer
checks its verified repository Python imports before expensive cold preparation
and uses the repository working directory for the ordinary KeyNet resolver.

The registered Release identifiers are `T01-native-fresh-v2-release`,
`T02-native-fresh-v2-release` and `T03-native-fresh-v2-release`. The CLI draft
selects these for T01-T03. Explicit old selections remain available and keep
their original version-1 parity rejection. Recorded Release `job_wall_seconds`
are 20.2726, 14.8532 and 18.3885 for T01, T02 and T03 respectively. These probes
did not measure live-process RSS and establish no resource or timing acceptance.

Reference validation still precedes every plan/candidate read. No probe packet
or approval registry creates independent annotations, a ReferenceSeal, a musical
score, resource acceptance, human acceptance or a default switch. Actual fresh
producer data remains private under `scratch/b2-fresh-lineage-20261008/`.

The existing strict worker and versioned envelope readers validate both final
versions. Duplicate-safe bounded parsing precedes each reader. Full frontend
logit counts are checked against complete PCM extent; raw/component/published
arrays must agree without a removed prefix/tail, downcast, fitted offset or new
exclusion. Reports preserve all raw positions/logits, identities and lineage
digests. Original failed attempts remain distinct private evidence. Unapproved
metadata, worker-only decodes and generic self-hashed claims reject. The fresh
native contract above remains distinct from the corrected QM comparator contract
described below.

Private JSON remains bounded to 16 MiB; unchanged worker-response, final-envelope
and array limits apply. Oversize output fails without truncation or overwriting.
All work remains outside realtime and leaves live audio/project/default state.

## Corrected legacy comparisons

[Corrected legacy provenance](beat-this-corrected-legacy.md) uses the actual
native QM pipeline through a reserved `OfflineAnalysisJob`, complete source/mono/
44100-Hz analyzer input and a diagnostic-only `corrected-qm-native-v1` finish.
Its fixed reviewed profiles retain all binary64 detector frames/seconds,
unsigned downbeat indices and the separate binary32 compatibility projection.
QM has no Beat This model, Python inference worker or logits. Executing native
EXE, used dependencies, retained producer sources and unused installed PYD are
separate identities; historical legacy arrays and candidate errors remain intact.

Supported corrected comparators are an explicit additional plan selection.
They do not join the candidate registry or replace a candidate in `draft`.
The optional `comparators` array contains at most five distinct `track_id` /
`profile_id` selections; omitted comparators mean no legacy inspection. Drafting
includes them only with `--include-corrected-legacy`:

```powershell
uv run --no-sync python -m flitzis_looper.analysis.beat_scoring_cli draft --workspace "$workspace" --reference-seal "scratch/b2-reference/reference-seal-v1.json" --source-aliases "scratch/b2-scoring/source-aliases.json" --include-corrected-legacy --output "scratch/b2-scoring/plan-with-comparators.json"
```

`score` validates the genuine complete independent seal and its actual full
source/PCM material before opening the plan or either backend's artifacts. It
validates every candidate and comparator profile/track selection before the
first backend read. Each successfully read selected candidate can receive a
`corrected_legacy` report containing the complete comparator report, temporal
metrics against the same sealed reference and the symmetric engineering
comparison. If its candidate is omitted or unavailable, the workflow records
that missing input and does not inspect its comparator. A comparator without
a seal cannot supply its own labels or trigger musical scoring.

The separate pure engineering API is invoked by an explicit private harness,
not an engineering CLI command. That harness validates the existing unlabelled
source/PCM inventory and reads all five approved chains for mechanical
same-source comparisons. It reports complete-array identities, counts/extent
and numerical backend comparisons. Such results establish engineering observations only;
differences between backends are not labeled missing beats or musical errors.
Ordinal counts stay unverified. It neither supplies temporal musical scores nor
closes quarter/bar, human correction, resource or default gates. Every original
failed/rejected profile remains unchanged; no subset is called full B2 acceptance.

## Actual remaining acceptance

The current T01-T05 inputs have no complete human ReferenceSeal; actual musical
scores remain unavailable. Current inventory/missing-reference probes are input
evidence only. Full independent count/meter/bar/critical/split/class/uncertainty
labels, actual musical scores/remediation and six balanced
paired human sessions remain open, including absolute caps, 20% operations/time
and zero-baseline gates. The bounded complete selected-backend BPM and
representative-region metadata integration/evaluation is implemented with
unverified counts and rejected global/region gates; it does not establish
musical truth. Gated new-analysis cutover, saved/manual/TAP/accepted/cache
preservation, unavailable/atomic/freshness/end-to-end tests and setup/rollback
docs remain open. Human/device hearing and corrections are deferred to the final
pre-port phase. No temporal score promotes an analyzer default.
