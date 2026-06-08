## 1. Specification
- [x] 1.1 Add OpenSpec deltas for bounded stem mode/mask transitions.
- [x] 1.2 Run official strict OpenSpec validation for this change.

## 2. Rust Mixer
- [x] 2.1 Add fixed-size Rust transition state for accepted stem source-selection changes.
- [x] 2.2 Apply the transition only to full-mix/all-stems mode and component-mask changes.
- [x] 2.3 Preserve source-frame playhead and loop wrapping while the transition is active.

## 3. Tests
- [x] 3.1 Add focused Rust tests for full-mix to all-stems crossfade behavior.
- [x] 3.2 Add focused Rust tests for stem mask crossfade behavior and playhead continuity.
- [x] 3.3 Add focused Rust tests that inactive pads do not retain stale transitions.

## 4. Documentation And Handoff
- [x] 4.1 Update architecture and loop/source/stem docs with Stage 7A results.
- [x] 4.2 Update local `codex-meta/handoff/next-step.md`.

## 5. Validation
- [x] 5.1 Run focused Rust tests for the changed mixer behavior.
- [x] 5.2 Run the required uv-managed Rust/Python validation sequence for behavior changes.
