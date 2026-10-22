## Authority and reuse

Use the current immutable selected Map2/Raw1 or Map3/Raw2 and its complete embedded
capture. Legacy Map1 has no complete captured object and reports this feature
unavailable; existing playback and edits remain compatible. A readback or numeric
parse supplies no musical, source-current or audio authority.

Use SourceBeatmapSelection.edit → existing _admit/begin_source_beatmap_edit_async
→ MapInput::Edit → genuine original-source verification → artifact store and owned
completion. Do not introduce a new Raw lease, worker queue or pending/ACK state.
The restored base must match the explicitly expected map and capture revisions.
Normal current-source/request/cancel/retirement and final publication fences remain.

Put the splice, read index and shared budget logic in focused Native modules; keep
the new GUI in source_beatmap_review_window.py with small existing-editor glue.

## Frozen edit wire

The compact strict JSON intent remains at most8192 UTF-8 bytes:

```json
{
  "schema_version":1,
  "operation":"review_raw_window",
  "map_revision":"<64 lowercase hex>",
  "capture_revision":"<64 lowercase hex>",
  "mode":"replace_interior",
  "first_anchor_index":12,
  "last_anchor_index":30,
  "rows":[
    {"kind":"existing","anchor_index":16,"quarter_bits":"4021000000000000"},
    {"kind":"raw","raw_index":42,"quarter_bits":"4024000000000000"}
  ]
}
```

All structs deny unknown fields. Modes are exactly extend_end, extend_start and
replace_interior. Indices are strict bounded integers, never booleans. Quarter
coordinates are explicit finite Binary64 values encoded as16 lowercase hex digits,
as in the existing review builder. Native resolves all source positions; the
client cannot submit source times, trust, meter or sample-edit provenance.

Extend end/start fixes the current last/first anchor respectively, with first=last
equal to that outer index and1–63 new Raw rows. Every old row and interval remains
unchanged. Replace interior fixes two existing endpoints, with first<last, at most
62 old interior anchors and0–62 requested interior rows. Only omission in this
mode deletes a selected interior anchor. Exact resulting anchor rows are refused
without churn, including an Apply that would only discard existing qualification.

Existing references are unique selected interior indices. Copy their id, source
bits, Raw index, review and complete sample_edit exactly, changing only explicitly
provided beat_bits. Fixed endpoint rows remain wholly exact, including signed-zero
bits. New Raw rows resolve from the actual capture, are estimated with no
sample_edit, and receive deterministic collision-checked IDs. Reject duplicate
observation use, invalid/outside new Raw positions and any resulting source/quarter
order conflict atomically. No epsilon fitting, silent sorting or ordinal labels.

All untouched exterior anchor/interval rows remain exact. No suffix quarter shift
occurs. A changed endpoint total count uses the existing separate SetCount action
first. Every new/replaced interval starts uncertain with unknown meter and phase;
external trusted/meter/phase evidence is not copied across the changed window.
Existing separate coverage/count/meter/downbeat actions remain authoritative.

## Bounded immutable readback API

SourceBeatmap.EDITOR_CORRECTION_API_VERSION becomes3. The two new methods are:

```python
editor_review_info()
editor_review_page(first_anchor_index, last_anchor_index, start_index, max_events=128)
```

Info fields: status ready|unavailable, reason str|None, map_revision, source_identity,
capture_revision str|None, capture_encoding str|None, backend legacy_qm|beat_this|None,
event_count, anchor_count, source_sample_rate_hz and source_frame_count.
Use capture_revision for the inner capture digest; it is not the enclosing
record.raw.raw_revision recalculated by seal.

Page fields: status, reason, map_revision, source_identity, capture_revision,
first_anchor_index, last_anchor_index, start_index, returned_count,
total_event_count, has_more, anchors and events. Invalid indices, limits and
oversized accepted ranges raise a clear error. Valid legacy requests return
unavailable. The accepted range may be a single outer anchor for extension and
contains at most64 rows inclusive. Raw pages contain at most128 events.

Each accepted row contains index, source_seconds, source_seconds_bits, beat,
beat_bits, raw_beat_index, review, sample_index and has_sample_edit. Do not expose
unbounded id/evidence strings. Each Raw row contains raw_index, source_seconds,
source_seconds_bits, detected_downbeat and physical_domain inside|exclusive_end|
outside. A detected downbeat is an unverified observation, never a bar claim.

Build a compact immutable derived observation/accepted-metadata index once during
off-RT parsing from the validated capture and typed rows, charging its actual
storage. It does not replace or mutate encoded Raw, retain another independent
lease or become a second timing authority. Queries never reparse full JSON or
decode full arrays per GUI frame; Python never parses the complete record.

## Closed Undo and resource admission

Use existing refinement lineage and an exact bounded window inverse. Extend
parse_intent/editor_operation/inverse_shape/apply_inverse/undo_operation recognition
with deterministic replay from the reconstructed predecessor. Compare exact rows,
intervals, inverse and predecessor digest; arbitrary lineage/inverse JSON cannot
grant Undo authority. Preserve old closed recognizers and sequence through Copy
and model-free restore.

Generalize the existing interpretation::budget checked512-MiB peak admission.
Preserve its existing Raw-review guard values, with explicit additional index
charging at real allocation sites. Structural64-MiB/250k caps remain independent
of simultaneous-memory admission; count/byte caps alone are not a RAM proof.

Before edit receipt restoration, use bounded receipt evidence and actual held
source bytes to admit read/parse/index allocation. Before the full Value tree and
before splice/inverse/lineage growth, charge actual base/index plus conservative
prospective output and inverse sizes, held source, serialization/hash/reparse and
coexisting old/new maps. Count serialized inverse/output with a bounded writer
before large copies; an8-KiB intent does not bound existing provenance strings.
Apply the shared guard to all affected edit operations and parse/index paths,
including Undo and whole-array interval inverses. Overflow or rejection preserves
the old map. Final seal count/hash/geometry checks remain additional defenses.

The existing Validate/Store/Restore/Create jobs also charge their actual held
source before the affected read/parse/index work. Create admits the established
64-KiB source descriptor plus a fixed128-KiB two-anchor/envelope allowance; the
existing descriptor validator checks the borrowed tree before cloning it.
Closed Undo validates the actual reconstructed predecessor metadata after its
parent and prior lineage are installed, before trusting its digest. Self-consistent
hashes cannot authorize an impossible predecessor parent type or self-reference.

Generic durable JSON also requires shape-sensitive admission before typed parsing.
The historical Raw-review32-times-serialized arithmetic remains unchanged, but is
not a proof for arbitrary lineage payload arrays or tiny objects. A streaming
counting visitor scans the complete JSON without constructing a Value tree. It
charges array growth/minimum capacity, decoded strings and bounded BTreeMap node
capacity using the pinned64-bit serde/standard-library representation. Layout
checks and small allocation-density regressions protect these assumptions.

For serialized bytes S and a conservative single-tree heap bound T, the generic
working estimate is max(32*S,6*T+12*S), plus fixed16 MiB, held source, retained old
map/index and the prospective new index. Six tree equivalents cover caller,
current/typed record, Undo preview, predecessor, canonical hash and one additional
typed/external temporary;12*S reserves encoded/hash/decoded-array/key scratch.
All arithmetic is checked and admission is rechecked during the scan. Existing
tree/inverse counting uses borrowed data before copies; final actual output is
checked again before seal/reparse. Unsupported layout or an overbudget shape
refuses instead of falling back to the byte-only estimate. Raw-review construction
also admits its borrowed generic source descriptor plus bounded typed Raw/array/
label fields before embedded capture or full-envelope Value construction; the
historical fixed allowance is not used as a substitute for that shape proof. This is a conservative
software allocation admission bound, not a measured whole-process RSS result.

At that pre-embedding boundary the known typed QM serializer provides exact-size
arrays of16-hex strings and bounded numeric downbeat indices, with index count no
greater than beat count. Its single-tree estimate uses6 Raw serialized extents;
BeatThis's four base64 strings use3 extents. Fixed metadata/array headers receive
512 KiB and explicit labels receive8192 bytes each; both descriptor occurrences
are counted from borrowed storage. Later arbitrary Value parsing always uses the
streaming generic shape calculation rather than these typed-only estimates.

## User flow and fences

The selected Source Map editor offers Extend end, Extend start and Replace
interior; displays fixed endpoint positions/counts; pages embedded Raw; and edits
an explicit bounded draft of Raw or Existing references with quarter labels.
Existing-row selection preserves corrected positions. Removing a draft row is
distinct from applying an interior replacement. Only Apply window submits intent.
No automatic musical labels or implicit coverage promotion are added.

Use the existing selected span for replacement and the actual outer anchor for
extension. A completed new revision clears stale draft/readback. Busy/unavailable
and conflicting labels are visible refusals. Bind the entire gesture to pad/source,
map revision, capture revision, mode, endpoint indices/bits, page range and exact
draft rows. A changed press/release target cannot edit a different window.

Update stubs, launcher/bootstrap capability checks, Portable documentation and
real compatibility tests together. Actual Native capability, signature and fresh
installed import checks are required after the productive change.

## Evidence boundaries

Prove successive64→127→190 anchors, skipped extra observations, bounded deletion
and non-affine quarter repair, corrected-source preservation, unchanged exterior
trust/meter/phase, complete Raw arrays/provenance, closed Undo/Copy/fresh restore,
stale/cancel/save failures, memory boundaries and real GUI gestures. Only separate
coverage qualification enables local-tempo use on new uncertain spans. Fully
missed-beat sample insertion, automatic full-span musical qualification, B2,
renderer/audio adoption and Human acceptance remain open.
