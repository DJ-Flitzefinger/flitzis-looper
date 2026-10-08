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
complete ReferenceSeal before preparing historical selections. `score` first
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

Actual byte/semantic inspection finds complete bit-exact publication lineage
only for T04/T05. T01-T03 raw/component/snapshot arrays agree, but their original
version-1 completion events changed some binary64 logits during the old native
JSON roundtrip. Those actual profiles reject with a raw-parity mismatch; they
need fresh complete native candidate lineage before scoring. Their snapshots
and failed events remain unchanged. A draft lists import attempts, not certified
valid candidates. Selecting only T04/T05 can yield partial diagnostics after
genuine full reference sealing; it cannot close the five-track B2 gate.

The existing strict worker and versioned envelope readers validate both final
versions. Duplicate-safe bounded parsing precedes each reader. Full frontend
logit counts are checked against complete PCM extent; raw/component/published
arrays must agree without a removed prefix/tail, downcast, fitted offset or new
exclusion. Reports preserve all raw positions/logits, identities and lineage
digests. Original failed attempts remain distinct private evidence. Unapproved
metadata, worker-only decodes and generic self-hashed claims reject. Fresh native
and corrected-legacy imports require their own supported provenance contracts.

Private JSON remains bounded to 16 MiB; unchanged worker-response, final-envelope
and array limits apply. Oversize output fails without truncation or overwriting.
All work remains outside realtime and leaves live audio/project/default state.

## Actual remaining acceptance

The current T01-T05 inputs have no complete human ReferenceSeal; actual musical
scores remain unavailable. Current inventory/missing-reference probes are input
evidence only. Full independent count/meter/bar/critical/split/class/uncertainty
labels, actual scores/remediation, corrected-legacy evidence and six balanced
paired human sessions remain open, including absolute caps, 20% operations/time
and zero-baseline gates. Full selected-backend BPM and representative-region
policy/evaluation, gated new-analysis cutover, saved/manual/TAP/accepted/cache
preservation, unavailable/atomic/freshness/end-to-end tests and setup/rollback
docs remain open. Human/device hearing and corrections are deferred to the final
pre-port phase. No temporal score promotes an analyzer default.
