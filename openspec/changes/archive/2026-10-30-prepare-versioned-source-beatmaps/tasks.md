## Prerequisites

- [ ] Freeze exact synthetic map fixtures and the shared coordinate/identity contract.
- [ ] Coordinate raw Beat This provenance with adopt-beat-this-analysis; the backend is selected.
- [ ] Repair legacy downbeat timing before using legacy output as a quality comparator, without blocking the map schema.

## Implementation

Bounded foundation checkpoint: registered immutable Rust `SourceBeatmap` and its
schema/provenance validation, forward/inverse/local-BPM/snap queries are implemented.
Nineteen synthetic/manual geometry tests and fifteen Python envelope tests pass;
the independent review includes the reproduced and corrected final-anchor snap
failure. Native typing is present, and Python serialization returns defensive
copies. This completes those standalone components, not the full checklist items
that also require verified source identity, stale completion, legacy migration,
Project persistence/restore and current-content adoption. Reuse these components;
do not rebuild a second evaluator from the unchecked integration tasks below.

- [ ] Define additive schema/provenance and legacy scalar migration with source identity checks.
- [ ] Implement one focused Rust offline map validator/evaluator and checked inverse.
- [ ] Expose versioned control-side evaluation and persist raw analysis plus manual edit lineage.
- [ ] Preserve raw model resolution, accepted sample-index/rate edits and uncertainty without asserting perfect detected beats.
- [ ] Keep source-map identity independent of pitch; include pitch revision only in rendered derivatives.
- [ ] Add tests for invalid maps, gaps, round trips, sample rates, stale completion and restore.
- [ ] Update architecture, native API typing and development documentation.

## Acceptance

- [ ] Run full Rust/Python/build/lint/type validation and strict validation of this change.
- [ ] Verify old projects preserve scalar grid/markers and that live playback is unchanged.
- [ ] Record map-size/evaluation costs and explicitly keep unverified maps out of live SYNC.
