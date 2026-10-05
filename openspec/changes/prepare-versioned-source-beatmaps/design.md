## Status and dependency

Planned, not implemented. Beat This! 1.1.0 final0/minimal is the selected analysis replacement.
Build this foundation against exact synthetic maps in parallel with its offline worker boundary;
accepting real maps requires the model/manual quality gates in the implementation design.
Repair the legacy downbeat bug before using it as a comparator, not as a prerequisite to this
schema. No result of the research arithmetic probe is audible acceptance.

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
