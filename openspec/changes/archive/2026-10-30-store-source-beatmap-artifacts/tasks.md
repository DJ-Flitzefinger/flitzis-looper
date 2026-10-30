## Implementation and verification
- [x] Extend typed paths and original native async map input/output owners.
- [x] Implement guarded immutable store and bounded fresh restore on the same lane.
- [x] Prove genuine source-qualified roundtrip, legacy/readable/M-ID paths and receipt strictness.
- [x] Prove wrong source/backing, cancellation/Drop/shutdown, exactbirth cleanup and genuine delivery fences.
- [x] Run affected integrated tests, Release/import/preflight, strict validation and one independent final diff review.

Delivery requires a coherent commit, normal push to existing main and clean
HEAD/tracking/fresh actual remote equality. The execution receipt is recorded
in the existing workspace test log after those operations actually succeed.

Full compact selection/Config/Copy/history/Portable migration integration and
all original pre-Rust/Human/final gates remain open after this prerequisite.
