## Status and dependency

The first offline/control-side foundation is implemented: the registered Rust `SourceBeatmap`
provides immutable versioned validation, forward/inverse queries, local BPM and supported-grid
snapping, with Native API typing and a defensive Python persisted-envelope adapter. Its bounded
synthetic/manual checks do not establish source custody, current-content acceptance or Project
integration. Source-bound model/loader/persistence/copy integration remains open. Beat This!
1.1.0 final0/minimal remains the selected analysis replacement subject to its existing gates.
Exact synthetic and explicitly reviewed manual maps can establish this foundation without
pretending that raw detector output has passed model or musical acceptance. Reuse the delivered
legacy comparator repair and offline worker boundaries rather than rebuilding them.

This increment preserves the current acknowledged constant-timing authority described by
`adopt-native-constant-timing`. Its exact binary64 period, signed origin, current-source/intent
ownership, single master/output clock and pending-versus-effective distinctions remain valid.
The control map does not silently replace them. B4 editor integration and later prepared live
map/trajectory adoption, continuous SYNC, restore, resource, acoustic, device and Human acceptance
remain required by [historical beatmap-sync-design.md](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/27ef7f878f55c0955835ca536af8df6ef4a89a28/docs/beatmap-sync-design.md) and the full pre-Rust objective. The temporary
offline/editor-only boundary is an implementation stage, not a narrowed final variable-map goal.

## Data and ownership

Persist anchors in f64 source seconds relative to canonical decoded audio, with beat indices,
explicit beat unit, optional downbeat/meter labels and coverage/quality state. Derive loaded-frame
anchors outside the callback for the actual output rate; original file frames, analysis hops,
loaded frames and output frames are distinct. A positive local tempo curve is derived from
adjacent anchors rather than independently stored as another timing authority.

Include schema version, source content fingerprint plus decode/preprocessing identity, immutable
map revision, detector/code/config identity, model checksum when applicable, interpolation
version and manual edit lineage. Do not treat a source path or matching duration as identity.
Preserve unmodified detector output and user corrections separately.
Raw model positions have about 20-ms frame spacing; preserve that provenance instead of labeling
sample-rounded predictions exact musical truth. Accepted sample-domain edits additionally retain
their chosen source sample index/rate and evidence/author. A local refinement is optional, bounded,
versioned and individually reversible; ambiguous evidence cannot become a trusted anchor merely
because it was moved to an onset. Beat-count and meter corrections are separate operations.

Start with piecewise affine monotone interpolation and its algebraic inverse. Validate finite
strictly increasing source positions and beat indices, minimum spacing, source extent and
bounded record count. A gap without trusted beat count is an unsupported segment, not proof
that its endpoints are consecutive beats. Source-extent validation applies to raw physical detections before alignment;
signed alignment may place corrected anchors outside the audio as virtual references. Such
references are not readable indices; any physical lookup must separately validate/wrap its
source range. Endpoint extrapolation is marked unverified and is
not automatically eligible for future SYNC. Interpolation smoothness is a later rendering issue.

Reuse a focused Rust evaluator via the non-realtime native boundary, not independent Python
mapping formulas. Validate/construct once; a renderer can later compile bounded segment state.
Public UI snapshots need only visible-range coordinates and a revision. This foundation does
not enqueue full maps into the audio callback.

## Stable identity and actual source custody

Use the existing cold-store canonical decoder descriptor and its complete `decoder_identity`.
The descriptor binds original bytes, complete decoded PCM, original decoded rate/channels/extent,
decoder policy and source zero. It is separate from playback-cache identity, output conversion,
loaded-frame rate and current pad/request/lifetime. A supported output-rate change derives new
loaded-frame coordinates from unchanged source seconds and retains the same decoder/map identity.
Analysis-input PCM rate/hash/extent and preprocessing remain separately recorded raw provenance.

The production reuse seam is `cold_store/streaming.rs`'s decoder descriptor and the existing
`cold_store::canonical`/`descriptor_digest` implementation. A narrowly exposed helper can serve
the map codec; do not add another canonicalization/hash authority or a decoder pipeline. A JSON
descriptor and matching digest certify only internal data consistency. Actual source-bound map
acceptance requires the genuine verified decoder metadata/readable immutable producer custody
and the current content/source/request/lifetime binding. Path, duration, a saved token, a matching
hash string or a historical constant-timing ticket cannot supply that authority.

Control-side restoration must verify that binding again. Persisted source records remain
historical until verified; map geometry acceptance is distinct from current audible adoption.
An accepted editor map never grants a callback ACK, live source permit or native preparation
permission. Preserve the existing actual decoded-data verification and stale/ABA protections.

## Frozen record and resource bounds

The additive envelope is schema version `1`, encoding `source-beatmap-pwa-v1`, with a strict
`record` object. Its fields have these responsibilities:

| Record field | Stored responsibility |
| --- | --- |
| `source` | Complete `decoder_identity` and canonical decoder descriptor. |
| `raw` | Immutable raw revision, detector/code/config/model/checksum identities, analysis-input PCM and preprocessing identity, raw resolution, beat/downbeat seconds and uncertainty. |
| `accepted` | Complete map revision and optional parent revision, quarter-note beat unit, interpolation/boundary policy, anchors, interval coverage/meter and correction lineage. |

Represent every authoritative binary64 coordinate/evidence value by its exact 16-digit
hexadecimal bits. A complete revision is a full 64-digit lowercase SHA-256, never a truncated
label. Compute raw and map revisions from their canonical complete payloads excluding only
their respective self-revision fields; the map payload binds source, raw provenance and the
complete accepted state. Validate supported versions, required fields, digest syntax and
consistency. Structural parsing or recomputing these digests does not verify actual source
custody or musical truth. Python recognizes the supported immutable envelope; Rust owns the
geometric/numeric/digest contract.

Accept 2 through 250,000 accepted anchors, with exactly N-1 explicit interval records. Raw beat
and downbeat arrays each retain the existing 250,000-event bound; an explicitly manual map may
have no raw detections. Bound lineage to 250,000 operations and the complete encoded envelope
to 64 MiB (67,108,864 bytes). Enforce the byte bound before full decoding and the count bounds
before allocating derived storage. Projection requests return at most 2,048 positions. Exceeding
a bound rejects the new operation with a reason and retains the previous state; never truncate
raw evidence, anchors or lineage to manufacture acceptance. Record actual map-size/query costs
at implementation acceptance; these software bounds are not process-RAM or realtime proof.
The retained P6/process-RAM, simultaneous preparation, responsiveness and scratch-resource
acceptance gates require actual measurements; a 64-MiB record limit is not a 1-GiB memory proof.
Heavy construction/validation belongs on an existing off-realtime preparation/control worker
when needed, not ordinary UI-frame work or a new executor/supervisor pipeline.

Anchor IDs are stable within their lineage. Accepted anchors store source-seconds and explicit
quarter-note coordinates, raw associations when available, evidence/review state and optional
chosen sample index/rate. Position, beat-count, downbeat/meter, whole-map translation and optional
refinement are distinct recorded operations. Translation creates a validated new anchor snapshot
and preserves intervals within binary64 precision; its provenance is not another transform
applied on top of the authoritative coordinates. Raw detections are never overwritten.

Immutable revisions may share storage. Each movable content instance independently owns its
selected revision/correction intent, consistent with `add-rearrange-shared-pad-content`.
Copy shares verified immutable records and copies selection intent into independent stopped
content; later edits do not fan out to equal-source copies. Move/Swap carries that content's
selection. Selected stems refer to the same parent map, not new independently detected maps.
Same-source reanalysis preserves explicit accepted corrections unless deliberately replaced;
source replacement cannot inherit them merely through a reused path or duration.

## Checked numerical and coverage policy

Both anchor axes must be finite and strictly increasing. For every adjacent pair require finite
positive differences at least `16 * max(ulp(left), ulp(right))` in their respective units. Here
`ulp(x)` is the distance from `abs(x)` to the next greater finite binary64 value, using the
previous finite value at the largest finite magnitude and the smallest positive subnormal at
zero. Reject overflow of the spacing bound. Require finite positive `delta_beat/delta_seconds`,
its inverse and `60 * (delta_beat/delta_seconds)`. This conditioning rule does not define a new
musical BPM range. Invalid arithmetic is unavailable/rejected, never hidden by clipping.

Policy `pwa-source-seconds-quarter-v1` uses piecewise-affine interpolation and its algebraic
inverse with O(log N) interval lookup. An exact source or beat anchor query returns the stored
counterpart bits directly, without an interpolation round trip. Inside a trusted interval
evaluate from the endpoint differences, then check the result. At ordinary internal anchors,
local BPM uses the right interval; the final anchor uses the left interval. If an anchor touches
any uncertain or uncovered interval, its exact coordinate remains queryable but local BPM is
unavailable. A gap edge never supplies a tempo bridge.

Each interval explicitly distinguishes `trusted`, `uncertain` and `gap_unknown_count` coverage;
meter certainty is separate. Raw array order never assigns trusted beat counts or meter.
Ordinary supported-coordinate queries do not interpolate across uncertain/unknown-count gaps.
Exact gap-boundary coordinates and their evidence remain queryable without certifying the gap.
Ordinary queries outside map coverage are unavailable. Explicitly named diagnostic extrapolation
may use the adjacent supported endpoint interval only and always returns unverified status.
Neither extrapolation nor signed virtual references become physical reads or future SYNC
eligibility. Readability of `[0, source_duration)` and physical marker bounds remain separate.

Continuous coordinates retain fractional frames. Convert seconds to loaded frames once at the
physical boundary. Preserve the current scalar nearest-even grid/physical-marker conventions.
Future variable snapping uses the separately versioned
`nearest-quarter-subdivision-later-tie-v1`: choose the nearest musical point and choose the later
point at an exact halfway tie, as required by `share-editor-beatmap-coordinates`. Do not apply a
broad epsilon to move near-halfway requests. This difference does not change scalar behavior.
For supported loaded projections, target at most one loaded frame of mapping/round-trip error
at representable anchors, with exact equality for integer fixtures. A numerically unsafe
projection reports unavailable instead of claiming sample accuracy.

## Control-side API and integration seams

The frozen immutable native surface is `SourceBeatmap.from_json(record_json)`, `record_json()`,
`revision()`, `source_identity()`, `evaluate_source(source_seconds)`, `evaluate_beat(beat)`,
`local_tempo_at_source(source_seconds)`, `nearest_grid_source(source_seconds, step_beats)`,
`project_beats(first_beat, last_beat, step_beats, max_points)`, and separately named
`diagnostic_extrapolate_source`/`diagnostic_extrapolate_beat`. Parsing certifies structure and
digest consistency only. Evaluation results carry the full revision, optional source/beat/local
BPM values, coordinate and interval status, segment index and physical-domain status. Invalid,
gap, uncertain, outside and unverified-extrapolation results cannot masquerade as supported
coordinates. Projection limits are explicit rather than an unlabeled partial success.

Keep this module on the control side. Native registration, supported Python envelope typing,
`SampleAnalysis`/project persistence, same-source reanalysis preservation, strict Copy/Bank
snapshot validation and source-bound restore are required integration seams, not presumed
delivered behavior. Use existing atomic project saves and current source/lifetime authorities.
Keep the current constant-timing envelope and resolver intact. B4 later routes visible grids,
snap/auto-loop and local BPM through this evaluator and distinguishes displayed from audible
revisions. Later B5/B6/B7 extend existing preparation, one SourcePlayback trajectory, BPMLOCK,
captured input T, output clock, scheduler and ACK ownership for live variable behavior; they do
not acquire a second rate multiplier, Python scheduler or timing authority.

## Compatibility

Legacy scalar mode remains exactly representable by effective BPM and signed origin. Restoring
a legacy project preserves that mode and its markers, including manual BPM overrides. Existing
f32 beat timestamps retain their actual precision; converting to f64 does not restore lost
precision. Optional maps are additive and ignored safely by old-format migration code only if
the actual compatibility test confirms it; do not promise arbitrary older app compatibility.

Map edits create a new revision, never overwrite active immutable state. Failed validation,
stale source completion or persistence failure retains the last accepted state. Stems refer to
the parent source's map, not independently detected grids. A later cache for warped audio must
also include map/loop/render settings and engine version.
Neither future pad semitone changes nor KEYLOCK toggles alter the source map. Rendered derivatives
include pitch trajectory/revision; raw source analysis and prepared stem caches remain reusable.

## Validation and rollback

Test strict monotonicity, missing beats, meter uncertainty, signed origin, seconds/frame changes,
round trips, loop edges, stale completion and legacy restore. Use the same synthetic anchors
from the research arithmetic as one independent expected-value fixture. Full project checks
apply when implementing this shared/persistence boundary. Rollback selects the original scalar
mode and leaves new records intact; no destructive migration is required.

Add independent expectations for exact stored-bit anchors, 16-ULP/overflow/underflow guards,
forward/inverse/local-BPM finiteness, unequal intervals, one-sided slope and unavailable gap-edge
tempo, diagnostic extrapolation, count/byte/projection refusal, full revision/source mismatch,
44.1/48/96-kHz projections, long coordinates, sample-edit provenance, correction retention on
reanalysis, per-content copy isolation and persistence failure. Later B4/audio integration must
still prove shared coordinates, stable markers/master/other pads, safe adoption/retirement,
actual variable rate/pitch/render behavior and all retained resource/musical/Human gates.
