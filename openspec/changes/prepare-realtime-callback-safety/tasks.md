## 1. Specification
- [x] 1.1 Add OpenSpec deltas for bounded callback command handling, oversized block safety, and deferred buffer retirement.
- [x] 1.2 Run official strict OpenSpec validation for this change.

## 2. Rust Callback Safety
- [x] 2.1 Add a fixed per-callback control-message budget.
- [x] 2.2 Add focused tests proving message bursts are not drained unboundedly.
- [x] 2.3 Split oversized render segments so preallocated stretch-buffer bounds are preserved.
- [x] 2.4 Add focused tests for oversized render blocks and scheduled event offsets.
- [x] 2.5 Add deferred sample/prepared-stem retirement outside the callback.
- [x] 2.6 Add focused tests for retirement queue behavior and mixer unload/reject paths.

## 3. Documentation And Handoff
- [x] 3.1 Update architecture and message-passing docs with Stage 2 results.
- [x] 3.2 Update local `codex-meta/handoff/next-step.md`.

## 4. Validation
- [x] 4.1 Run focused Rust tests for the changed audio-engine modules.
- [x] 4.2 Run the required uv-managed Rust/Python validation sequence for behavior changes.
