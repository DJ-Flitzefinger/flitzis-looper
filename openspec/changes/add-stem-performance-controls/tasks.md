## 1. Specification And Planning
- [x] 1.1 Create the OpenSpec proposal, design, tasks, and spec deltas for performer-facing stem controls.
- [x] 1.2 Update repository planning documentation without implementing production feature code.
- [x] 1.3 Run `openspec validate add-stem-performance-controls --strict`.

## 2. Future UI And Controller Implementation
- [x] 2.1 Add UI selectors for per-pad stem availability, generation progress, generation errors, and durable stem mix mode.
- [x] 2.2 Route the selected-pad "Generate Stems" action through the stem controller and existing inactive-pad gating.
- [x] 2.3 Render selected-sidebar stem availability, generation progress, blocked state, and error indicators without file I/O or blocking work in the render loop.
- [ ] 2.4 Render pad-grid stem availability indicators without file I/O or blocking work in the render loop.
- [x] 2.5 Add a selected-pad full-mix/all-stems mode control.
- [ ] 2.6 Add future-ready per-stem mute/solo/toggle entry points.
- [x] 2.7 Add Python tests for selectors, controller actions, blocked/generating/error states, and UI action wiring.

## 3. Future Persistence Implementation
- [x] 3.1 Persist per-pad durable stem mix mode with a full-mix default for new and older projects.
- [x] 3.2 Restore persisted stem cache metadata only after current-source and complete-cache revalidation.
- [x] 3.3 Keep generation progress, last errors, blocked reasons, and momentary mute/solo gestures out of persisted project state.
- [x] 3.4 Add model and persistence tests for defaults, round-trip behavior, stale cache fallback, and older project loading.

## 4. Future Audio-Thread Control Implementation
- [x] 4.1 Add fixed-size Rust control messages for full-mix/all-stems stem mix mode.
- [x] 4.2 Store full-mix/all-stems mode in bounded audio-thread-owned state.
- [x] 4.3 Apply full-mix/all-stems mode without changing pad voice timing, loop-region behavior, BPM-lock, key-lock, or full-mix fallback.
- [x] 4.4 Treat audio-thread disk I/O, Python/GIL access, logging, blocking, heap allocation, neural inference, or long-running work as blockers.
- [x] 4.5 Add deterministic Rust tests for full-mix/all-stems switching, fallback, and real-time-safe state transitions.
- [ ] 4.6 Add bounded per-stem mask messages/state/tests when mute, solo, and toggle controls are implemented.

## 5. Validation
- [x] 5.1 Run official OpenSpec validation for this change before implementation is considered complete.
- [x] 5.2 Run focused Rust/Python tests for any future implementation slice that changes code.
- [x] 5.3 Run full uv-managed Rust/Python validation before merging production stem-control behavior.
