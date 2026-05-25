## 1. Specification
- [x] 1.1 Add OpenSpec deltas for durable state ownership and controller-owned telemetry dispatch.
- [x] 1.2 Run official strict OpenSpec validation for this change.

## 2. State Boundary
- [x] 2.1 Document the durable Python state, transient Python session projection, and live Rust
  audio state ownership table.
- [x] 2.2 Document project restore ordering and acknowledgement/reconciliation expectations.
- [x] 2.3 Move audio telemetry dispatch from the UI context into controller-owned runtime polling.
- [x] 2.4 Add focused tests for controller-owned audio telemetry dispatch and UI delegation.

## 3. Documentation And Handoff
- [x] 3.1 Update architecture and message-passing docs with Stage 4 results.
- [x] 3.2 Update local `codex-meta/handoff/next-step.md`.

## 4. Validation
- [x] 4.1 Run focused Python tests for the changed controller/UI boundary.
- [x] 4.2 Run the required uv-managed validation sequence for this behavior change.
