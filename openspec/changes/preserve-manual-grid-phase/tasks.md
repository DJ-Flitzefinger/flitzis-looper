## 1. Context and contract

- [x] 1.1 Inspect current editor/persistence/reference, transport/source timing and tests.
- [x] 1.2 Create official focused proposal/design/deltas for signed phase foundations and activation gate.

## 2. Signed source and master mapping

- [x] 2.1 Preserve signed editor origin through native metadata, reuse effective BPM/editor helpers and restore.
- [x] 2.2 Implement focused bounded Rust source beat/bar, loop-start and master-to-loop mapping.
- [x] 2.3 Validate whole-bar/evenly tiling short-loop compatibility with at most one source frame of rounding tolerance.
- [x] 2.4 Preserve complete master musical position across valid BPM changes without source-position jumps.
- [x] 2.5 Preserve long-position loop-frame markers with f64 seconds through ordinary/direct-MIDI paths under the existing loop-region contract.

## 3. Bootstrap and pending launches

- [x] 3.1 Publish a dedicated bootstrap request through shared selected-pad/BPMLOCK enable/restore reference setup.
- [x] 3.2 Implement first-reference latch, availability retries, incomplete-reference unload clearing and one-time completion.
- [x] 3.3 Preserve explicit deliberate sync and prove no bootstrap rearm after silence, wraps or stem updates.
- [x] 3.4 Retain accepted pending targets/order/timestamps across BPM/metadata updates and use latest loop at execution.
- [x] 3.5 Prove current immediate/future-grid loop-start launches remain active and mapping remains diagnostic.
- [x] 3.6 Invalidate loop-edited voice anchors and share normalized bootstrap/render source-playhead handling.

## 4. Verification and documentation

- [x] 4.1 Cover signed origins, long frame markers, source-rate conversion, overrides/restore, compatible/incompatible loops and bootstrap failures.
- [x] 4.2 Update maintained architecture/native/UI documentation and review callback safety/final diff.
- [x] 4.3 Pass official strict validation, native build/check/tests, Python tests, Ruff/mypy and format checks.
- [x] 4.4 Record actual execution state, measured limits and next pending audible-DSP slice in workspace handoff.
